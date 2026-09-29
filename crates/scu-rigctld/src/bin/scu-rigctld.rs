//! `scu-rigctld` — serve a SCU-LAN10 session to Hamlib rigctld clients.
//!
//! ```text
//! scu-rigctld --host 192.168.1.100 --user defaultuser --pass defaultuser
//! ```
//!
//! Then point WSJT-X / fldigi / N1MM at "Hamlib NET rigctl" on
//! `127.0.0.1:4532`.

use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use scu_client::{ConnectConfig, Event, ScuClient};
use scu_rigctld::{RadioState, Rigctld, DEFAULT_PORT};

struct Args {
    config: ConnectConfig,
    seconds: u64,
    port: u16,
}

fn parse_args() -> Result<Args, String> {
    let mut config = ConnectConfig::default();
    let mut seconds = 0u64;
    let mut port = DEFAULT_PORT;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--host" | "-H" => config.host = args.next().ok_or("--host needs a value")?,
            "--port" | "-p" => {
                config.base_port = args
                    .next()
                    .ok_or("--port needs a value")?
                    .parse()
                    .map_err(|_| "--port must be a number")?
            }
            "--user" | "-u" => config.username = args.next().ok_or("--user needs a value")?,
            "--pass" | "-P" => config.password = args.next().ok_or("--pass needs a value")?,
            "--seconds" | "-s" => {
                seconds = args
                    .next()
                    .ok_or("--seconds needs a value")?
                    .parse()
                    .map_err(|_| "--seconds must be a number")?
            }
            "--rigctld-port" | "-t" => {
                port = args
                    .next()
                    .ok_or("--rigctld-port needs a value")?
                    .parse()
                    .map_err(|_| "--rigctld-port must be a number")?
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(Args {
        config,
        seconds,
        port,
    })
}

fn print_help() {
    println!(
        "scu-rigctld — serve a SCU-LAN10 as a Hamlib rigctld endpoint

USAGE:
    scu-rigctld [OPTIONS]

OPTIONS:
    -H, --host <IP>          Radio/SCU-LAN10 address (default 192.168.1.100)
    -p, --port <PORT>        Base UDP port (default 50000)
    -u, --user <USER>        Username (default defaultuser)
    -P, --pass <PASS>        Password (default defaultuser)
    -s, --seconds <N>        How long to serve (default 0 = until Ctrl-C)
    -t, --rigctld-port <P>   rigctld TCP listen port (default 4532)
    -h, --help               Show this help"
    );
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = match parse_args() {
        Ok(args) => args,
        Err(error) => {
            eprintln!("error: {error}\n");
            print_help();
            return ExitCode::from(2);
        }
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("failed to start runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    runtime.block_on(serve(args))
}

async fn serve(args: Args) -> ExitCode {
    println!(
        "connecting to {} (ports {}-{}) as {}...",
        args.config.host,
        args.config.ctrl_port(),
        args.config.scope_port(),
        args.config.username
    );

    let mut client = match ScuClient::connect(args.config).await {
        Ok(client) => client,
        Err(error) => {
            eprintln!("connect failed: {error}");
            return ExitCode::FAILURE;
        }
    };

    let state = Arc::new(Mutex::new(RadioState::default()));
    let server = match Rigctld::start(client.handle(), Arc::clone(&state), args.port) {
        Ok(server) => server,
        Err(error) => {
            eprintln!("failed to start rigctld server: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "rigctld listening on 0.0.0.0:{} — configure software as Hamlib NET rigctl",
        server.port()
    );

    // Populate the cache once, then keep it fresh from the response stream.
    for command in [
        "ID;", "FA;", "FB;", "FR;", "MD0;", "MD1;", "SM0;", "TX;", "PC;", "AC;",
    ] {
        client.handle().send_cat(command);
    }

    let deadline = (args.seconds > 0).then(|| Instant::now() + Duration::from_secs(args.seconds));
    loop {
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            break;
        }
        match tokio::time::timeout(Duration::from_millis(250), client.recv()).await {
            Ok(Some(Event::Cat(frame))) => state.lock().unwrap().apply(&frame),
            Ok(Some(Event::Radio(model))) => {
                let frame = format!("ID{:04};", model.id());
                state.lock().unwrap().apply(&frame);
            }
            Ok(Some(Event::Disconnected(reason))) => {
                eprintln!("disconnected: {reason}");
                break;
            }
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(_) => {}
        }
    }

    drop(server);
    ExitCode::SUCCESS
}
