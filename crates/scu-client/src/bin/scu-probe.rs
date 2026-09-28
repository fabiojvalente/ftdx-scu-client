//! Minimal end-to-end harness: connect to a SCU-LAN10, print decoded traffic.
//!
//! ```text
//! scu-probe --host 192.168.1.100 --user defaultuser --pass defaultuser --seconds 10
//! ```

use std::time::{Duration, Instant};

use scu_client::{ConnectConfig, Event, ScuClient};

struct Args {
    config: ConnectConfig,
    seconds: u64,
    command: Option<String>,
    verbose: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut config = ConnectConfig::default();
    let mut seconds = 10u64;
    let mut command = None;
    let mut verbose = false;

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
            "--command" | "-c" => command = Some(args.next().ok_or("--command needs a value")?),
            "--verbose" | "-v" => verbose = true,
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
        command,
        verbose,
    })
}

fn print_help() {
    println!(
        "scu-probe — connect to a SCU-LAN10 and dump decoded traffic

USAGE:
    scu-probe [OPTIONS]

OPTIONS:
    -H, --host <IP>        Radio/SCU-LAN10 address (default 192.168.1.100)
    -p, --port <PORT>      Base UDP port (default 50000)
    -u, --user <USER>      Username (default defaultuser)
    -P, --pass <PASS>      Password (default defaultuser)
    -s, --seconds <N>      How long to listen (default 10)
    -c, --command <CAT>    Send an extra CAT command, e.g. 'FA;'
    -v, --verbose          Log every CAT frame
    -h, --help             Show this help"
    );
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}\n");
            print_help();
            std::process::exit(2);
        }
    };

    println!(
        "connecting to {} (ports {}-{}) as {}...",
        args.config.host,
        args.config.ctrl_port(),
        args.config.scope_port(),
        args.config.username
    );

    let mut client = ScuClient::connect(args.config).await?;
    let handle = client.handle();

    if let Some(cmd) = &args.command {
        println!("sending CAT command: {cmd}");
        handle.send_cat_now(cmd).await?;
    }

    let started = Instant::now();
    let mut audio_frames = 0u64;
    let mut scope_frames = 0u64;
    let mut cat_frames = 0u64;
    let mut last_scope = Instant::now();
    let mut last_audio = Instant::now();

    println!("listening for {}s (Ctrl-C to stop)...\n", args.seconds);

    while started.elapsed() < Duration::from_secs(args.seconds) {
        let event = match tokio::time::timeout(Duration::from_millis(250), client.recv()).await {
            Ok(Some(ev)) => ev,
            Ok(None) => break,
            Err(_) => continue,
        };

        match event {
            Event::Connected { session_id } => {
                println!("connected: session 0x{session_id:02X}");
            }
            Event::Radio(model) => println!("radio: {}", model.name()),
            Event::Cat(frame) => {
                cat_frames += 1;
                if args.verbose {
                    println!("CAT  {frame}");
                }
            }
            Event::Audio(frame) => {
                audio_frames += 1;
                last_audio = Instant::now();
                if args.verbose {
                    println!(
                        "AUDIO seq={} {}Hz ch={} samples={}",
                        frame.seq,
                        frame.sample_rate,
                        frame.channels,
                        frame.len()
                    );
                }
            }
            Event::Scope(body) => {
                scope_frames += 1;
                last_scope = Instant::now();
                if args.verbose {
                    let line = scu_scope::decode(&body);
                    println!("SCOPE bins={} boundary={}", line.bin_count(), line.boundary);
                }
            }
            Event::Error(msg) => eprintln!("error: {msg}"),
            Event::Disconnected(reason) => {
                eprintln!("disconnected: {reason}");
                break;
            }
        }
    }

    println!("\n--- summary ---");
    println!("CAT frames:   {cat_frames}");
    println!("audio frames: {audio_frames}");
    println!("scope frames: {scope_frames}");
    println!(
        "last audio:   {:.1}s ago",
        last_audio.elapsed().as_secs_f32()
    );
    println!(
        "last scope:   {:.1}s ago",
        last_scope.elapsed().as_secs_f32()
    );

    Ok(())
}
