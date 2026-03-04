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
cp "$PROJECT_DIR/assets/icon.icns" "$APP_DIR/Contents/Resources/icon.icns"

# Ad-hoc sign so macOS doesn't reject the app as "damaged"
codesign --force --deep --sign - "$APP_DIR"

echo ""
echo "App bundle created: $APP_DIR"

# Create DMG with Applications symlink for drag-and-drop install
DMG_PATH="$PROJECT_DIR/target/StandGround.dmg"
DMG_STAGING="$PROJECT_DIR/target/dmg-staging"
rm -rf "$DMG_STAGING"
mkdir -p "$DMG_STAGING"
cp -r "$APP_DIR" "$DMG_STAGING/"
ln -s /Applications "$DMG_STAGING/Applications"
hdiutil create -volname "StandGround" -srcfolder "$DMG_STAGING" -ov -format UDZO "$DMG_PATH"
rm -rf "$DMG_STAGING"

echo ""
echo "DMG created: $DMG_PATH"
echo "  open \"$DMG_PATH\""
