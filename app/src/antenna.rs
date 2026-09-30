//! Minimal HTTP client for the LAN antenna switch ("Antenna Switch V3").
//!
//! The switch is a tiny web server: `GET /` returns an HTML status page and the
//! antenna is moved with `/ant/up`, `/ant/down` or `/ant?val=N`. Access is
//! guarded by HTTP basic auth. Everything runs on a short-lived worker thread so
//! a slow or unreachable switch never stalls the UI.

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use eframe::egui;
use web_time::Instant;

/// Connection properties for one antenna switch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AntennaConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

impl Default for AntennaConfig {
    fn default() -> Self {
        Self {
            host: "192.168.1.165".into(),
            port: 80,
            username: String::new(),
            password: String::new(),
        }
    }
}

/// A request sent to the switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AntennaAction {
    /// Read the current antenna without changing it.
    Poll,
    /// Move one port up.
    Up,
    /// Move one port down.
    Down,
    /// Select a specific antenna port.
    Set(u8),
}

/// Decoded state of the switch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AntennaStatus {
    pub current: u8,
    pub band: String,
}

/// Result of a worker request, delivered back to the UI thread.
#[derive(Debug, Clone)]
pub enum AntennaResult {
    Status(AntennaStatus),
    /// A move was accepted but the response carried no state; a confirmation
    /// poll will follow to read the settled port.
    Done,
    Error(String),
}

/// UI-side handle: owns the worker channel and the last known state.
pub struct AntennaState {
    tx: Sender<AntennaResult>,
    rx: Receiver<AntennaResult>,
    /// A request is in flight; suppress new ones until it completes.
    in_flight: bool,
    /// Wall-clock time of the last dispatched request.
    last_attempt: Option<Instant>,
    /// When set, a confirmation poll is due. Used right after a move so the
    /// settled state is read back once the switch has caught up.
    confirm_at: Option<Instant>,
    /// The antenna reported by the switch, if known.
    pub current: Option<u8>,
    /// The band label reported by the switch ("MANUAL", ...).
    pub band: String,
    /// Last error, cleared on a successful poll.
    pub error: Option<String>,
}

impl Default for AntennaState {
    fn default() -> Self {
        Self::new()
    }
}

impl AntennaState {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            tx,
            rx,
            in_flight: false,
            last_attempt: None,
            confirm_at: None,
            current: None,
            band: String::new(),
            error: None,
        }
    }

    /// True while a request is outstanding.
    pub fn in_flight(&self) -> bool {
        self.in_flight
    }

    /// Time since the last request, or `None` if none has been sent.
    pub fn since_last_attempt(&self) -> Option<Duration> {
        self.last_attempt.map(|t| t.elapsed())
    }

    /// Schedule a one-off confirmation poll shortly from now.
    pub fn confirm_soon(&mut self) {
        self.confirm_at = Some(Instant::now() + Duration::from_millis(1200));
    }

    /// Consume a due confirmation poll, if any.
    pub fn take_confirm_due(&mut self) -> bool {
        match self.confirm_at {
            Some(at) if Instant::now() >= at => {
                self.confirm_at = None;
                true
            }
            _ => false,
        }
    }

    /// Dispatch a request on a worker thread. No-op while one is in flight.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn request(&mut self, config: AntennaConfig, action: AntennaAction, ctx: egui::Context) {
        if self.in_flight {
            return;
        }
        self.in_flight = true;
        self.last_attempt = Some(Instant::now());
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(run(&config, action));
            // Wake the UI thread so the result is applied without waiting for
            // the next input or animation tick.
            ctx.request_repaint();
        });
    }

    /// The browser build has no raw sockets and the switch sends no CORS
    /// headers, so remote antenna control is native-only.
    #[cfg(target_arch = "wasm32")]
    pub fn request(&mut self, _config: AntennaConfig, _action: AntennaAction, _ctx: egui::Context) {
        self.error = Some("not available in the browser build".into());
    }

    /// Apply any completed worker results.
    pub fn drain(&mut self) {
        while let Ok(result) = self.rx.try_recv() {
            self.in_flight = false;
            match result {
                AntennaResult::Status(status) => {
                    self.current = Some(status.current);
                    self.band = status.band;
                    self.error = None;
                }
                AntennaResult::Done => {
                    self.error = None;
                }
                AntennaResult::Error(err) => {
                    self.error = Some(err);
                }
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn run(config: &AntennaConfig, action: AntennaAction) -> AntennaResult {
    let path = match action {
        AntennaAction::Poll => "/".to_string(),
        AntennaAction::Up => "/ant/up".to_string(),
        AntennaAction::Down => "/ant/down".to_string(),
        AntennaAction::Set(port) => format!("/ant?val={port}"),
    };
    let html = match get(config, &path) {
        Ok(html) => html,
        Err(err) => return AntennaResult::Error(err.to_string()),
    };
    if matches!(action, AntennaAction::Poll) {
        return parse_status(&html)
            .map(AntennaResult::Status)
            .unwrap_or_else(|| AntennaResult::Error("could not parse antenna status".into()));
    }
    // The form action is a GET on the same page, so a move usually returns the
    // updated status HTML. If not, fall back to a confirmation poll.
    match parse_status(&html) {
        Some(status) => AntennaResult::Status(status),
        None => AntennaResult::Done,
    }
}

/// One HTTP/1.0 GET with optional basic auth. The switch may keep the socket
/// open, so the response is framed by headers/`Content-Length` rather than by
/// waiting for close (which would stall until the read timeout).
#[cfg(not(target_arch = "wasm32"))]
fn get(config: &AntennaConfig, path: &str) -> std::io::Result<String> {
    use std::io::Write;
    use std::net::{TcpStream, ToSocketAddrs};

    let host = config.host.trim();
    let addr = (host, config.port)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no address"))?;
    let timeout = Duration::from_secs(3);
    let mut stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;

    let mut request = format!("GET {path} HTTP/1.0\r\nHost: {host}\r\nConnection: close\r\n");
    if !config.username.is_empty() || !config.password.is_empty() {
        let creds = format!("{}:{}", config.username, config.password);
        request.push_str(&format!(
            "Authorization: Basic {}\r\n",
            base64(creds.as_bytes())
        ));
    }
    request.push_str("User-Agent: scu-client\r\n\r\n");
    stream.write_all(request.as_bytes())?;

    read_body(&mut stream)
}

/// Read an HTTP response and return only the body. A read timeout is treated as
/// end-of-body (the switch sometimes leaves the socket open), so a partial but
/// parseable page is still usable.
#[cfg(not(target_arch = "wasm32"))]
fn read_body(stream: &mut std::net::TcpStream) -> std::io::Result<String> {
    use std::io::Read;

    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];

    // Read up to the end of the header block.
    let header_end = loop {
        match stream.read(&mut chunk) {
            Ok(0) => break None,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
                    break Some(pos + 4);
                }
                if buf.len() > 64 * 1024 {
                    break None;
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break None,
            Err(err) => return Err(err),
        }
    };

    let Some(body_start) = header_end else {
        return Ok(String::from_utf8_lossy(&buf).into_owned());
    };

    let headers = String::from_utf8_lossy(&buf[..body_start]).to_ascii_lowercase();
    let content_length = headers.lines().find_map(|line| {
        line.strip_prefix("content-length:")
            .and_then(|value| value.trim().parse::<usize>().ok())
    });

    match content_length {
        Some(len) => {
            while buf.len() - body_start < len {
                match stream.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(err) => return Err(err),
                }
            }
        }
        None => loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(err) => return Err(err),
            }
        },
    }

    let body_end = body_start.saturating_add(content_length.unwrap_or(buf.len()));
    let body_end = body_end.min(buf.len());
    Ok(String::from_utf8_lossy(&buf[body_start..body_end]).into_owned())
}

/// Index of the first occurrence of `needle` in `haystack`.
#[cfg(not(target_arch = "wasm32"))]
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Extract the current antenna and band from the status page.
#[cfg(not(target_arch = "wasm32"))]
pub fn parse_status(html: &str) -> Option<AntennaStatus> {
    let current = extract_tag(html, "Current antenna:", "strong")?;
    let band = extract_text(html, "Band:").unwrap_or_default();
    current
        .parse()
        .ok()
        .map(|current| AntennaStatus { current, band })
}

/// Text between `<tag>` and `</tag>` following `marker`.
#[cfg(not(target_arch = "wasm32"))]
fn extract_tag(html: &str, marker: &str, tag: &str) -> Option<String> {
    let start = html.find(marker)? + marker.len();
    let rest = &html[start..];
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let content_start = rest.find(&open)? + open.len();
    let content_end = rest[content_start..].find(&close)? + content_start;
    Some(rest[content_start..content_end].trim().to_string())
}

/// Plain text following `marker`, up to the next `<`.
#[cfg(not(target_arch = "wasm32"))]
fn extract_text(html: &str, marker: &str) -> Option<String> {
    let start = html.find(marker)? + marker.len();
    let rest = &html[start..];
    let end = rest.find('<').unwrap_or(rest.len());
    Some(rest[..end].trim().to_string())
}

/// Standard base64, no padding crate required.
#[cfg(not(target_arch = "wasm32"))]
fn base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(b2 & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = "<!doctype html>\n<html><body>\n\
        <h2>Antenna Switch V3</h2>\n\
        <p>Band: MANUAL</p>\n\
        <p>Current antenna: <strong>2</strong></p>\n\
        </body></html>";

    #[test]
    fn parses_status_page() {
        let status = parse_status(PAGE).expect("status");
        assert_eq!(status.current, 2);
        assert_eq!(status.band, "MANUAL");
    }

    #[test]
    fn rejects_unrelated_page() {
        assert!(parse_status("<html><body>nope</body></html>").is_none());
    }

    #[test]
    fn reads_body_by_content_length_without_waiting_for_close() {
        use std::io::Write;
        use std::net::{TcpListener, TcpStream};

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().expect("accept");
            let body = "<p>Current antenna: <strong>3</strong></p>";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            sock.write_all(response.as_bytes()).expect("write");
            // Hold the connection open; the reader must not wait for close.
            std::thread::sleep(Duration::from_millis(700));
        });

        let mut stream = TcpStream::connect(addr).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("timeout");
        let started = std::time::Instant::now();
        let body = read_body(&mut stream).expect("body");
        assert!(body.contains("<strong>3</strong>"));
        // Without Content-Length framing this would block for the full timeout.
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "waited for close"
        );
        server.join().expect("server");
    }

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"user:pass"), "dXNlcjpwYXNz");
    }
}
