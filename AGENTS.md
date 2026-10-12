# ftdx10-scu-client — Agent instructions

## Build & run

```sh
make run          # native desktop app (cargo run --release -p scu-app)
make web          # WebAssembly bundle into app/dist/ (runs scripts/build-web.sh)
make serve        # web bundle + python3 http.server :8080
make bridge       # ws<->udp relay for browser build
make test         # cargo test --workspace
make fmt          # cargo fmt --all
make lint         # cargo clippy --workspace --all-targets -- -D warnings
make app          # macOS .app bundle (macOS only)
make dmg          # macOS .dmg installer (macOS only)
```

- Everything runs in **release mode** — test your changes with `--release`.
- The lint target uses `clippy` (intentional, not a typo of `clippy`).
- For a focused test: `cargo test -p <crate>` (test any single crate independently).

## Workspace structure

```
app/                       # egui/eframe GUI — native + wasm (package: scu-app)
crates/
  scu-protocol/            # framing, 8-byte header, gTable, XOR codec (pure, sync)
  scu-cat/                 # CAT encode/parse: FA FB IF MD SM SS ID RM ...
  scu-scope/               # boundary detection, bin//2, normalize, colormaps
  scu-audio/               # inner-header decode, ring buffer, resample, cpal/WebAudio
  scu-client/              # executor-agnostic session state machine + channel tasks
  scu-rigctld/             # native rigctld-protocol TCP server (Hamlib clients)
  scu-bridge/              # WebSocket ⇄ UDP relay for the browser build
```

Binary entrypoints:
- `scu-app` at `app/src/main.rs` — desktop GUI (eframe) or wasm web runner
- `scu-probe` at `crates/scu-client/src/bin/scu-probe.rs` — headless probe
- `scu-rigctld` at `crates/scu-rigctld/src/bin/scu-rigctld.rs` — headless rigctld server

## Platform quirks

- **Tokio** workspace dep is wasm-safe (`rt`, `time`, `sync`, `macros` only). `net` and `rt-multi-thread` are added per-target via `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`.
- **`std::time::Instant::now()`** panics on `wasm32-unknown-unknown`. Use `web-time::Instant` in code reachable by the browser build.
- **macOS packaging** (`scripts/package-macos.sh`) uses only OS-shipped tools. A universal build needs both `aarch64-apple-darwin` and `x86_64-apple-darwin` targets.
- **WebAssembly** needs `wasm-bindgen-cli` matching the `wasm-bindgen` crate version (currently `0.2.129` — verify in `Cargo.lock`).
- **DYLD_FALLBACK_LIBRARY_PATH** workaround for macOS wasm linker in `scripts/build-web.sh`.
- **macOS permissions**: the generated `Info.plist` in `scripts/package-macos.sh` must declare usage strings, or TCC denies/misbehaves. Currently:
  - `NSMicrophoneUsageDescription` — TX mic capture via `cpal` (`crates/scu-audio/src/input.rs`).
  - `NSLocalNetworkUsageDescription` — LAN access to the radio (UDP/TCP). Required on macOS 15 Sequoia+.
  - Global hotkeys use Carbon (`global-hotkey`), which needs **no** Accessibility/Input Monitoring grant.
  - If the app is ever sandboxed, add `com.apple.security.network.client` (and `.server` for the `scu-rigctld` listener) to an entitlements file.

## `cfg`-gated features

- `#[cfg(not(target_arch = "wasm32"))]` — `antenna.rs`, `cat_server.rs`, `hotkeys.rs`, `cpal` audio, `ringbuf`, `crossbeam-channel`
- `#[cfg(target_arch = "wasm32")]` — `wasm-bindgen`, `web-sys`, `js-sys`, `gloo-timers`, `tracing-wasm`, `getrandom` (js feature)
- See `app/Cargo.toml` and `crates/scu-client/Cargo.toml` for the exact split.

## Testing

- `cargo test` — all workspace tests. Protocol, CAT, and scope decoding have hardware-free unit tests.
- End-to-end testing requires a live SCU-LAN10 radio via `scu-probe`.
- No CI workflow for PRs — only a release workflow triggered on `main` push / tag / manual.

## Radio setup (for reference)

- Yaesu SCU-LAN10 + FTDX10 / FTDX101 / FT-710
- CAT RATE 38400 bps (115200 over LAN), CAT TIMEOUT 10 ms, CAT RTS ON
- Sessions are exclusive — one client at a time
- UDP ports: 50000 (CTRL), 50001 (CAT), 50002 (AUDIO), 50003 (SCOPE)

## Agent plans (`.agent/plans/`)

- `ROADMAP.md` — feature gap analysis against FT-Control for Yaesu reference app
- `WEBASSEMBLY.md` — browser port architecture details (cfg-gating, bridge, audio backends)