#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROFILE="debug"
DO_RUN=0
CARGO_ARGS=()

for arg in "$@"; do
  case "$arg" in
    --release) PROFILE="release"; CARGO_ARGS+=(--release) ;;
    --run) DO_RUN=1 ;;
    *) echo "unknown arg: $arg" >&2; exit 2 ;;
  esac
done

ICONS="$ROOT/assets/icons"
BIN_DIR="$ROOT/target/$PROFILE"
APP="$BIN_DIR/Snack.app"
CONTENTS="$APP/Contents"
MACOS="$CONTENTS/MacOS"
RES="$CONTENTS/Resources"

echo "→ cargo build ${CARGO_ARGS[*]:-}"
cargo build --locked "${CARGO_ARGS[@]+"${CARGO_ARGS[@]}"}"

if [[ ! -x "$BIN_DIR/snack" ]]; then
  echo "error: expected binary at $BIN_DIR/snack" >&2
  exit 1
fi

rm -rf "$APP"
mkdir -p "$MACOS" "$RES"

cp "$BIN_DIR/snack" "$MACOS/snack"
chmod +x "$MACOS/snack"

if [[ -f "$ICONS/snack.icns" ]]; then
  cp "$ICONS/snack.icns" "$RES/snack.icns"
fi

if [[ -f "$ICONS/macos/Assets.car" ]]; then
  cp "$ICONS/macos/Assets.car" "$RES/Assets.car"
fi

cat > "$CONTENTS/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDisplayName</key>
  <string>Snack</string>
  <key>CFBundleExecutable</key>
  <string>snack</string>
  <key>CFBundleIdentifier</key>
  <string>com.echonet.snack</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>Snack</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>CFBundleVersion</key>
  <string>0.1.0</string>
  <key>CFBundleIconFile</key>
  <string>snack</string>
  <key>CFBundleIconName</key>
  <string>snack</string>
  <key>LSMinimumSystemVersion</key>
  <string>13.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key>
  <true/>
</dict>
</plist>
EOF

if command -v codesign >/dev/null 2>&1; then
  codesign --force --deep --sign - "$APP" >/dev/null 2>&1 || true
fi

if command -v touch >/dev/null 2>&1; then
  touch "$APP"
fi

echo "bundled $APP"

if [[ "$DO_RUN" -eq 1 ]]; then
  open "$APP"
fi
