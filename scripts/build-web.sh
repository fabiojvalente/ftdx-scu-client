#!/usr/bin/env bash
# Build the browser bundle into app/dist/.
#
# Prerequisites:
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli --version <version matching the wasm-bindgen crate>
#
# If you have Trunk installed, `cd app && trunk build --release` is an
# equivalent (and slightly more automated) alternative.
set -euo pipefail

cd "$(dirname "$0")/.."

# `cargo install` places binaries in $CARGO_HOME/bin (default ~/.cargo/bin),
# which is not always on PATH.
cargo_bin="${CARGO_HOME:-$HOME/.cargo}/bin"
if [ -d "$cargo_bin" ]; then
  export PATH="$cargo_bin:$PATH"
fi

# On some macOS rustup installs the wasm linker cannot locate libLLVM.dylib.
sysroot="$(rustc --print sysroot)"
if [ -d "$sysroot/lib" ]; then
  export DYLD_FALLBACK_LIBRARY_PATH="$sysroot/lib:${DYLD_FALLBACK_LIBRARY_PATH:-}"
fi

if ! command -v wasm-bindgen >/dev/null; then
  echo "error: wasm-bindgen CLI not found." >&2
  echo "Install the version matching the wasm-bindgen crate in Cargo.lock, e.g.:" >&2
  echo "  cargo install wasm-bindgen-cli --version 0.2.129" >&2
  exit 1
fi

cargo build -p scu-app --target wasm32-unknown-unknown --release

rm -rf app/dist
mkdir -p app/dist
cp web/index.html app/dist/index.html

wasm-bindgen \
  --target web \
  --out-dir app/dist \
  --out-name scu-app \
  --no-typescript \
  target/wasm32-unknown-unknown/release/scu-app.wasm

echo "built app/dist/ — serve it with: python3 -m http.server 8080 --directory app/dist"
