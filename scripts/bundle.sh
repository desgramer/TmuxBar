#!/bin/bash
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_NAME="TmuxBar"
VERSION=$(grep '^version' "$PROJECT_ROOT/Cargo.toml" | head -1 | sed 's/.*"\(.*\)"/\1/')
APP_DIR="$PROJECT_ROOT/target/release/$APP_NAME.app"
DMG_PATH="$PROJECT_ROOT/target/release/$APP_NAME-$VERSION.dmg"

# Build release binary
echo "Building release binary..."
cargo build --release --manifest-path "$PROJECT_ROOT/Cargo.toml"

# Clean previous bundle
rm -rf "$APP_DIR"
rm -f "$DMG_PATH"

# Create .app structure
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"

# Copy files
cp "$PROJECT_ROOT/target/release/tmuxbar" "$APP_DIR/Contents/MacOS/tmuxbar"
cp "$PROJECT_ROOT/resources/Info.plist"    "$APP_DIR/Contents/Info.plist"
cp "$PROJECT_ROOT/resources/AppIcon.icns"  "$APP_DIR/Contents/Resources/AppIcon.icns"

echo "Bundle created: $APP_DIR"

# Create DMG
echo "Creating DMG..."
hdiutil create -volname "$APP_NAME" \
    -srcfolder "$APP_DIR" \
    -ov -format UDZO \
    "$DMG_PATH"

echo ""
echo "DMG created: $DMG_PATH"
echo "To install: open the DMG and drag TmuxBar to Applications"
