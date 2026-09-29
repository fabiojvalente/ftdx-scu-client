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

- **Waterfall / panadapter** — decoupled decode pipeline, circular texture with
  partial uploads, colormaps, span, and follow-VFO.
- **Full CAT rig control** — VFO-A/B, RX/TX VFO select, split, swap, RIT/XIT,
  all 10 Yaesu modes, filters, DSP (NB/NR/auto-notch/narrow), AGC, RF gain,
  squelch, click-to-tune, and a raw CAT console.
- **Meters** — S-meter plus selectable `RM` meters.
- **RX/TX audio** — playback, volume/mute/stereo, resampling, PTT, TX audio,
  power, mic gain, ATU.
- **Headless probe** (`scu-probe`) and a reusable protocol core for building
  external bridges.

## Workspace layout

```
ftdx10-scu-client/
├── crates/
│   ├── scu-protocol/   # framing, 8-byte header, gTable, XOR codec (pure, sync)
│   ├── scu-cat/        # CAT encode/parse: FA FB IF MD SM SS ID RM ...
│   ├── scu-scope/      # boundary detection, bin//2, normalize, colormaps
│   ├── scu-audio/      # inner-header decode, ring buffer, resample, cpal/WebAudio
│   ├── scu-client/     # executor-agnostic session state machine + channel tasks
│   └── scu-bridge/     # WebSocket ⇄ UDP relay for the browser build
└── app/                # egui/eframe GUI (native + wasm)
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
