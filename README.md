# ftdx10-scu-client

A cross-platform Rust GUI client for the Yaesu **SCU-LAN10** network remote
control protocol, targeting the **FTDX10** (and the FTDX101 / FT-710, which
share the wire format). The headline feature is a fast, real-time spectrum
waterfall.

It runs as a native desktop app (egui/eframe) or in the browser (WebAssembly),
sharing one codebase.

Protocol reference: [CarrierWaveApp/sculan10-protocol](https://github.com/CarrierWaveApp/sculan10-protocol)
(reverse-engineered UDP wire format; CAT commands come from Yaesu's CAT
Operation Reference manuals).

## Features

- **Dockable panel UI** — every control group is a panel you can drag, tab,
  split, resize, close and reopen. Arrangements are saved as named presets
  (default, dashboard, or your own) and restored on the next launch.
- **Themes & accessibility** — Dark, Light, Yaesu and Neon palettes; five UI
  size steps; and high-contrast / large-target options for low-vision use.
- **Waterfall / panadapter** — decoupled decode pipeline, circular texture with
  partial uploads, colormaps, span, and follow-VFO.
- **Full CAT rig control** — VFO-A/B, RX/TX VFO select, split, swap, RIT/XIT,
  all 15 Yaesu modes, filters, DSP (NB/NR/auto-notch/narrow), AGC, RF gain,
  squelch, click-to-tune, and a raw CAT console.
- **Meters** — S-meter plus selectable `RM` meters.
- **RX/TX audio** — playback, volume/mute/stereo, resampling, PTT, TX audio,
  power, mic gain, ATU, plus VOX (audio keying).
- **External-software bridge** — a built-in rigctld-protocol server (no Hamlib
  binary needed) lets WSJT-X / fldigi / N1MM / Log4OM drive the radio, plus
  loopback audio routing (BlackHole, Loopback, Common-Radio).
- **Headless probe** (`scu-probe`), a headless rigctld server (`scu-rigctld`)
  and a reusable protocol core for building external bridges.

## User interface

The window is a thin connection/command bar over a dockable panel workspace
(built on [`egui_tiles`](https://github.com/rerun-io/egui_tiles)). Drag a tab to
move or split a panel, drag the gaps to resize, and use the tab close button or
the **Panels** menu to show and hide panels.

- **Layouts** menu — switch between the built-in **Default** and **Dashboard**
  arrangements, save the current one as a named preset, rename or delete
  presets, and reset to the default. The working arrangement and presets are
  stored in `~/.config/scu-client/layouts.json` (native) or browser
  `localStorage` (wasm) and restored on launch.
- **Pop-out** — the **Pop** button in any tab bar moves that panel into its own
  OS window (native builds). Use **Dock** in the panel's title bar (or close the
  window) to return it; floating panels are remembered across launches. On the
  web build a pop-out falls back to an embedded floating window.
- **Theme** menu — Dark, Light, Yaesu and Neon. The active palette is also
  selectable in Settings.
- **Scale** menu — Extra small … Extra large. Scaling changes fonts, spacing and
  hit targets together; it never moves panels.
- **Settings** — a tabbed window: appearance (theme, UI size, high-contrast and
  large-target options), visible meters, optional panes, and the console log
  level. The log level is applied live to the running app (native builds).

## Workspace layout

```
ftdx10-scu-client/
├── crates/
│   ├── scu-protocol/   # framing, 8-byte header, gTable, XOR codec (pure, sync)
│   ├── scu-cat/        # CAT encode/parse: FA FB IF MD SM SS ID RM ...
│   ├── scu-scope/      # boundary detection, bin//2, normalize, colormaps
│   ├── scu-audio/      # inner-header decode, ring buffer, resample, cpal/WebAudio
│   ├── scu-client/     # executor-agnostic session state machine + channel tasks
│   ├── scu-rigctld/    # native rigctld-protocol TCP server (Hamlib clients)
│   └── scu-bridge/     # WebSocket ⇄ UDP relay for the browser build
└── app/                # egui/eframe GUI (dockable flex panels, native + wasm)
```

The SCU-LAN10 exposes four UDP ports that share one framing + XOR scheme:

| Offset | Port (default) | Channel | ID |
|---|---|---|---|
| +0 | 50000 | CTRL (auth, setup, keepalives) | `0xFC` |
| +1 | 50001 | CAT (bidirectional) | `0xFD` |
| +2 | 50002 | AUDIO (RX/TX) | `0xFE` |
| +3 | 50003 | SCOPE (server → client) | `0xFA` |

The session layer is executor-agnostic: native uses Tokio UDP, the browser uses
one multiplexed WebSocket through `scu-bridge`.

## Build & run

Requires a recent stable Rust toolchain.

A `Makefile` wraps the common tasks (run `make` to list them):

```sh
make run        # build and run the native desktop app
make web        # build the browser (WebAssembly) bundle into app/dist/
make serve      # build the web bundle and serve it on :8080
make bridge     # run the WebSocket <-> UDP bridge the browser build needs
make test       # run the workspace test suite
```

### Native desktop app

```sh
cargo run -p scu-app --release
```

### Headless probe

Connects to a SCU-LAN10 and dumps decoded traffic — the manual end-to-end
harness, useful without the GUI.

```sh
cargo run -p scu-client --bin scu-probe -- \
    --host 192.168.1.100 --user defaultuser --pass defaultuser --seconds 10

# send a CAT command and dump every frame
cargo run -p scu-client --bin scu-probe -- --host 192.168.1.100 -c 'FA;' -v
```

Run `scu-probe --help` for all options.

### External software (CAT + audio)

The app can present the radio to third-party software. In the side rail:

- **Radio Server (CAT)** — a built-in, native **rigctld-protocol** server (no
  Hamlib binary, no serial port, no GPL dependency). Configure the other program
  as **Hamlib NET rigctl** at `127.0.0.1:4532`; several clients can share the one
  session. Getters are answered from a cached radio state and a TX safety
  watchdog releases PTT if a client disappears mid-transmission.
- **Audio Streaming** — route RX to a loopback output device and take TX from a
  loopback input device (BlackHole, Loopback, Common-Radio, VB-Cable, …), so the
  external program's soundcard in/out is bridged to the radio.
- **VOX** — key the transmitter from the audio level as an alternative to CAT
  PTT.

For a headless server (no GUI), use the `scu-rigctld` binary:

```sh
cargo run -p scu-rigctld -- \
    --host 192.168.1.100 --user defaultuser --pass defaultuser
```

It serves Hamlib clients on `4532` until Ctrl-C. Run `scu-rigctld --help` for
all options.

### Browser (WebAssembly)

A browser cannot open raw UDP sockets, so the browser build talks to a small
native **bridge** that relays the four UDP sockets over one WebSocket.

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129   # must match Cargo.lock
./scripts/build-web.sh
python3 -m http.server 8080 --directory app/dist
```

Open <http://localhost:8080>. With [Trunk](https://trunkrs.dev) installed,
`cd app && trunk serve` is an equivalent workflow.

Start the bridge on the LAN next to the radio:

```sh
cargo run -p scu-bridge --release -- --listen 0.0.0.0:9000
```

Then set **Host** to the SCU-LAN10, **Port** to the base UDP port, and
**Bridge** to `ws://<bridge-host>:9000`.

> Serve the page over `http` for `ws://`; an `https` deployment needs `wss://`
> with TLS in front of the bridge.

## Testing

```sh
cargo test
```

Protocol, CAT, and scope decoding have unit tests and need no hardware. Use
`scu-probe` against a live radio for end-to-end verification.

## Radio setup

- Target hardware: Yaesu SCU-LAN10 + FTDX10 (FTDX101 / FT-710 share the format).
- CAT RATE 38400 bps (115200 over LAN), CAT TIMEOUT 10 ms, CAT RTS ON.
- Sessions are exclusive on the SCU-LAN10 — one client at a time.
- IPv6 and multi-client operation are out of scope.

## Credits

Wire protocol reverse engineering by the
[CarrierWaveApp/sculan10-protocol](https://github.com/CarrierWaveApp/sculan10-protocol)
project.

## License

MIT
