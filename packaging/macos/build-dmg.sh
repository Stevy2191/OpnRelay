#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# packaging/macos/build-dmg.sh <universal-binary> <version> <outdir>
# Builds OpnRelay Recorder.app and a DMG. Signed ad hoc (free); see ADR 0007.
set -euo pipefail
BIN="$1"; VERSION="${2#v}"; OUT="$3"
WORK="$(mktemp -d)"
APP="$WORK/OpnRelay Recorder.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/opnrelay-recorder"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>OpnRelay Recorder</string>
  <key>CFBundleIdentifier</key><string>dev.opnrelay.recorder</string>
  <key>CFBundleExecutable</key><string>opnrelay-recorder</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
codesign --force --deep --sign - "$APP"
mkdir -p "$OUT"
ln -s /Applications "$WORK/Applications"
hdiutil create -volname "OpnRelay Recorder" -srcfolder "$WORK" -ov -format UDZO "$OUT/OpnRelay-Recorder-macOS-universal.dmg" >/dev/null
rm -rf "$WORK"
echo "built $OUT/OpnRelay-Recorder-macOS-universal.dmg"
