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
    /// Write every raw scope body (4096 bytes each) to this path.
    dump_scope: Option<String>,
    /// Analyse a previously dumped scope file instead of connecting.
    analyze_scope: Option<String>,
    /// VFO frequency (Hz) at capture time, for the analyser.
    analyze_vfo: Option<f64>,
    /// Scope span (Hz) at capture time, for the analyser.
    analyze_span: Option<f64>,
}

fn parse_args() -> Result<Args, String> {
    let mut config = ConnectConfig::default();
    let mut seconds = 10u64;
    let mut command = None;
    let mut verbose = false;
    let mut dump_scope = None;
    let mut analyze_scope = None;
    let mut analyze_vfo = None;
    let mut analyze_span = None;

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
            "--dump-scope" => dump_scope = Some(args.next().ok_or("--dump-scope needs a value")?),
            "--analyze-scope" => {
                analyze_scope = Some(args.next().ok_or("--analyze-scope needs a value")?)
            }
            "--vfo" => {
                analyze_vfo = Some(
                    args.next()
                        .ok_or("--vfo needs a value")?
                        .parse()
                        .map_err(|_| "--vfo must be a number")?,
                )
            }
            "--span" => {
                analyze_span = Some(
                    args.next()
                        .ok_or("--span needs a value")?
                        .parse()
                        .map_err(|_| "--span must be a number")?,
                )
            }
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
        dump_scope,
        analyze_scope,
        analyze_vfo,
        analyze_span,
    })
}

/// Print where the strongest bins sit in a dumped scope file, so the bin <->
/// frequency mapping can be measured against a known carrier.
fn analyze_scope(
    path: &str,
    vfo: Option<f64>,
    span: Option<f64>,
) -> Result<(), Box<dyn std::error::Error>> {
    use scu_scope::WF1_BINS;

    let bytes = std::fs::read(path)?;
    let bodies: Vec<&[u8]> = bytes.chunks(4096).filter(|b| b.len() == 4096).collect();
    if bodies.is_empty() {
        println!("no complete 4096-byte scope bodies in {path}");
        return Ok(());
    }
    println!("{path}: {} scope bodies", bodies.len());

    // Average the normalized bins so a steady carrier stands out from noise.
    let mut avg = vec![0f64; WF1_BINS];
    for body in &bodies {
        let line = scu_scope::decode(body);
        for (slot, &m) in avg.iter_mut().zip(line.bins.iter()) {
            *slot += m as f64;
        }
    }
    for slot in &mut avg {
        *slot /= bodies.len() as f64;
    }

    let mut peaks: Vec<usize> = (0..WF1_BINS).collect();
    peaks.sort_by(|&a, &b| avg[b].partial_cmp(&avg[a]).unwrap());
    let mut sorted = avg.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let floor = sorted[WF1_BINS / 2];

    let to_hz = |bin: usize| {
        vfo.zip(span)
            .map(|(vfo, span)| vfo + (bin as f64 / (WF1_BINS - 1) as f64 - 0.5) * span)
    };
    println!(
        "centre bin ~{} (VFO should sit here in CENTER mode)",
        WF1_BINS / 2 - 1
    );
    println!("noise floor {floor:.4}, strongest bins (bin, magnitude, from-centre[, freq]):");
    for &bin in peaks.iter().take(8) {
        let from_centre = bin as i64 - (WF1_BINS as i64 / 2 - 1);
        match to_hz(bin) {
            Some(hz) => println!(
                "  bin {bin:4}  {:+.4}  {from_centre:+}  {:.3} MHz",
                avg[bin] - floor,
                hz / 1e6
            ),
            None => println!("  bin {bin:4}  {:+.4}  {from_centre:+}", avg[bin] - floor),
        }
    }
    Ok(())
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
        --dump-scope <PATH>  Write every raw 4096-byte scope body to PATH
        --analyze-scope <PATH>  Analyse a dumped scope file offline
        --vfo <HZ>         VFO frequency at capture (prints peak frequencies)
        --span <HZ>        Scope span at capture (prints peak frequencies)
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

    if let Some(path) = &args.analyze_scope {
        return analyze_scope(path, args.analyze_vfo, args.analyze_span);
    }

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

    let mut dump = match &args.dump_scope {
        Some(path) => {
            println!("dumping raw scope bodies to {path}");
            Some(std::fs::File::create(path)?)
        }
        None => None,
    };

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
                if let Some(file) = &mut dump {
                    use std::io::Write;
                    file.write_all(&body)?;
                }
                if args.verbose {
                    let line = scu_scope::decode(&body);
                    println!("SCOPE len={} bins={}", body.len(), line.bin_count());
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
