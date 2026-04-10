#!/bin/bash
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_NAME="TmuxBar"
APP_DIR="$PROJECT_ROOT/target/release/$APP_NAME.app"

# Build release binary
echo "Building release binary..."
cargo build --release --manifest-path "$PROJECT_ROOT/Cargo.toml"

# Clean previous bundle
rm -rf "$APP_DIR"

# Create .app structure
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"

# Copy files
cp "$PROJECT_ROOT/target/release/tmuxbar" "$APP_DIR/Contents/MacOS/tmuxbar"
cp "$PROJECT_ROOT/resources/Info.plist"    "$APP_DIR/Contents/Info.plist"
cp "$PROJECT_ROOT/resources/AppIcon.icns"  "$APP_DIR/Contents/Resources/AppIcon.icns"

echo ""
echo "Bundle created: $APP_DIR"
echo "To install: cp -r \"$APP_DIR\" /Applications/"
