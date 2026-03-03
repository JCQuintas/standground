#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
APP_NAME="StandGround"
BUNDLE_ID="com.standground.standground"
APP_DIR="$PROJECT_DIR/target/${APP_NAME}.app"
VERSION="$(grep '^version' "$PROJECT_DIR/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')"

echo "Building StandGround v${VERSION}..."
cargo build --release --manifest-path "$PROJECT_DIR/Cargo.toml"

echo "Creating app bundle at $APP_DIR..."
rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"

# Copy binary
cp "$PROJECT_DIR/target/release/standground" "$APP_DIR/Contents/MacOS/standground"

# Copy Info.plist and stamp version
cp "$PROJECT_DIR/Info.plist" "$APP_DIR/Contents/Info.plist"
sed -i '' "s|<string>0\.1\.0</string>|<string>${VERSION}</string>|g" "$APP_DIR/Contents/Info.plist"

# Add CFBundleExecutable to Info.plist if not present
if ! grep -q "CFBundleExecutable" "$APP_DIR/Contents/Info.plist"; then
    sed -i '' 's|</dict>|    <key>CFBundleExecutable</key>\n    <string>standground</string>\n</dict>|' "$APP_DIR/Contents/Info.plist"
fi

# Copy icon
if [ -f "$PROJECT_DIR/assets/icon.svg" ]; then
    cp "$PROJECT_DIR/assets/icon.svg" "$APP_DIR/Contents/Resources/icon.svg"
fi

echo ""
echo "App bundle created: $APP_DIR"
echo ""
echo "To install, run:"
echo "  cp -r \"$APP_DIR\" /Applications/"
echo ""
echo "Or open the containing folder:"
echo "  open \"$(dirname "$APP_DIR")\""
