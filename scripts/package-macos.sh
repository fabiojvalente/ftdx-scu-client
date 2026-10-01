#!/usr/bin/env bash
# Build and package the native app as a macOS .app bundle and a .dmg.
#
# Everything used here ships with macOS: cargo/rustup, lipo, sips, iconutil
# and hdiutil. No third-party packaging tools are required.
#
# Usage:
#   scripts/package-macos.sh [--universal|--host] [--no-build]
#                            [--version <ver>] [--out <dir>]
#
#   --universal   build both aarch64 and x86_64 and lipo them (default)
#   --host        build only for the current machine's architecture
#   --app-only    stop after the .app bundle; don't create a .dmg
#   --no-build    skip cargo; package binaries that already exist
#   --version     version string for Info.plist / dmg filename
#                 (default: [workspace.package] version in Cargo.toml)
#   --out         output directory (default: dist)
set -euo pipefail

cd "$(dirname "$0")/.."

APP_NAME="SCU-LAN10 Client"
BIN_NAME="scu-app"
BUNDLE_ID="com.fabiojvalente.scu-app"
MIN_MACOS="10.15"
ICON_SRC="static/icon.png"

MODE="universal"
DO_BUILD=1
APP_ONLY=0
VERSION=""
OUT_DIR="dist"

while [ $# -gt 0 ]; do
  case "$1" in
    --universal) MODE="universal" ;;
    --host) MODE="host" ;;
    --app-only) APP_ONLY=1 ;;
    --no-build) DO_BUILD=0 ;;
    --version) VERSION="${2:?--version needs a value}"; shift ;;
    --out) OUT_DIR="${2:?--out needs a value}"; shift ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

if [ "$(uname -s)" != "Darwin" ]; then
  echo "error: macOS app packaging requires Darwin (got $(uname -s))." >&2
  exit 1
fi

if [ -z "$VERSION" ]; then
  VERSION="$(awk '/^\[workspace.package\]/{f=1} f&&/^version/{gsub(/[",]/,"",$3);print $3;exit}' Cargo.toml)"
fi
BARE_VERSION="${VERSION#v}"

case "$MODE" in
  universal)
    ARCHES=(aarch64-apple-darwin x86_64-apple-darwin)
    PLATFORM="macos-universal"
    ;;
  host)
    ARCHES=()
    PLATFORM="macos-$(uname -m)"
    ;;
esac

if [ "$DO_BUILD" = 1 ]; then
  if [ "$MODE" = universal ]; then
    installed="$(rustup target list --installed 2>/dev/null || true)"
    for arch in "${ARCHES[@]}"; do
      if ! printf '%s\n' "$installed" | grep -qx "$arch"; then
        echo "error: rust target $arch is not installed." >&2
        echo "  run: rustup target add ${ARCHES[*]}" >&2
        exit 1
      fi
    done
    for arch in "${ARCHES[@]}"; do
      cargo build --release --locked -p "$BIN_NAME" --target "$arch"
    done
  else
    cargo build --release --locked -p "$BIN_NAME"
  fi
fi

# Resolve the built binary path(s).
binaries=()
if [ "$MODE" = universal ]; then
  for arch in "${ARCHES[@]}"; do
    bin="target/$arch/release/$BIN_NAME"
    if [ ! -f "$bin" ]; then
      echo "error: missing $bin (build it or drop --no-build)." >&2
      exit 1
    fi
    binaries+=("$bin")
  done
else
  bin="target/release/$BIN_NAME"
  if [ ! -f "$bin" ]; then
    echo "error: missing $bin (build it or drop --no-build)." >&2
    exit 1
  fi
  binaries+=("$bin")
fi

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

STAGE="$OUT_DIR/.dmg-stage"
BUNDLE="$STAGE/$APP_NAME.app"
mkdir -p "$BUNDLE/Contents/MacOS" "$BUNDLE/Contents/Resources"

if [ "$MODE" = universal ]; then
  lipo -create "${binaries[@]}" -output "$BUNDLE/Contents/MacOS/$BIN_NAME"
else
  cp "${binaries[0]}" "$BUNDLE/Contents/MacOS/$BIN_NAME"
fi
chmod +x "$BUNDLE/Contents/MacOS/$BIN_NAME"

# Build AppIcon.icns from the source PNG with the built-in sips + iconutil.
if [ -f "$ICON_SRC" ]; then
  ICONSET="$OUT_DIR/.AppIcon.iconset"
  rm -rf "$ICONSET"
  mkdir -p "$ICONSET"
  for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$ICON_SRC" \
      --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
    dbl=$((size * 2))
    sips -z "$dbl" "$dbl" "$ICON_SRC" \
      --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
  done
  iconutil -c icns "$ICONSET" -o "$BUNDLE/Contents/Resources/AppIcon.icns"
  rm -rf "$ICONSET"
  ICON_KEY='  <key>CFBundleIconFile</key>
  <string>AppIcon</string>'
else
  echo "warning: $ICON_SRC not found; packaging without an icon." >&2
  ICON_KEY=""
fi

cat > "$BUNDLE/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>${APP_NAME}</string>
  <key>CFBundleDisplayName</key>
  <string>${APP_NAME}</string>
  <key>CFBundleIdentifier</key>
  <string>${BUNDLE_ID}</string>
  <key>CFBundleExecutable</key>
  <string>${BIN_NAME}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>${BARE_VERSION}</string>
  <key>CFBundleVersion</key>
  <string>${BARE_VERSION}</string>
  <key>LSMinimumSystemVersion</key>
  <string>${MIN_MACOS}</string>
  <key>NSHighResolutionCapable</key>
  <true/>
${ICON_KEY}
</dict>
</plist>
EOF

APP_BUNDLE="$OUT_DIR/$APP_NAME.app"
if [ "$APP_ONLY" = 1 ]; then
  mv "$BUNDLE" "$APP_BUNDLE"
  rm -rf "$STAGE"
  echo "built:"
  echo "  $APP_BUNDLE"
  exit 0
fi

DMG="$OUT_DIR/${BIN_NAME}-${VERSION}-${PLATFORM}.dmg"
ln -s /Applications "$STAGE/Applications"
hdiutil create \
  -volname "$APP_NAME" \
  -srcfolder "$STAGE" \
  -ov -format UDZO \
  "$DMG" >/dev/null
rm -rf "$STAGE"

echo "built:"
echo "  $DMG"