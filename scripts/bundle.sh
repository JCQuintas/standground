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

# Copy shim binary
cp "$PROJECT_DIR/target/release/standground" "$APP_DIR/Contents/MacOS/standground"

# Copy core dylib into Resources (fallback for first run)
cp "$PROJECT_DIR/target/release/libstandground_core.dylib" "$APP_DIR/Contents/Resources/libstandground_core.dylib"

# Copy Info.plist and stamp version
cp "$PROJECT_DIR/Info.plist" "$APP_DIR/Contents/Info.plist"
sed -i '' "s|<string>0\.1\.0</string>|<string>${VERSION}</string>|g" "$APP_DIR/Contents/Info.plist"

# Add CFBundleExecutable to Info.plist if not present
if ! grep -q "CFBundleExecutable" "$APP_DIR/Contents/Info.plist"; then
    sed -i '' 's|</dict>|    <key>CFBundleExecutable</key>\n    <string>standground</string>\n</dict>|' "$APP_DIR/Contents/Info.plist"
fi

# Copy icon
cp "$PROJECT_DIR/assets/icon.icns" "$APP_DIR/Contents/Resources/icon.icns"

# Sign the app bundle. Prefer the self-signed "StandGround Dev" certificate
# (stable identity — TCC permissions survive rebuilds). Falls back to ad-hoc.
SIGN_IDENTITY="-"
if security find-identity -v -p codesigning 2>/dev/null | grep -q "StandGround Dev"; then
    SIGN_IDENTITY="StandGround Dev"
fi
codesign --force --deep --sign "$SIGN_IDENTITY" "$APP_DIR"
echo "Signed with: $SIGN_IDENTITY"

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
