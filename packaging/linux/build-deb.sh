#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# Build a .deb for the desktop recorder: packaging/linux/build-deb.sh <binary> <version> <outdir>
set -euo pipefail
BIN="$1"; VERSION="$2"; OUT="$3"
ARCH="$(dpkg --print-architecture 2>/dev/null || echo amd64)"
ROOT="$(mktemp -d)"
chmod 755 "$ROOT"
install -Dm755 "$BIN" "$ROOT/usr/bin/opnrelay-recorder"
install -Dm644 "$(dirname "$0")/opnrelay-recorder.desktop" "$ROOT/usr/share/applications/opnrelay-recorder.desktop"
install -Dm644 "$(dirname "$0")/../../apps/extension/static/icons/icon-128.png" "$ROOT/usr/share/icons/hicolor/128x128/apps/opnrelay-recorder.png"
mkdir -p "$ROOT/DEBIAN"
cat > "$ROOT/DEBIAN/control" <<CTRL
Package: opnrelay-recorder
Version: ${VERSION#v}
Section: utils
Priority: optional
Architecture: $ARCH
Depends: libc6, libxcb1, libxkbcommon-x11-0, libgl1, libdbus-1-3
Maintainer: OpnRelay <noreply@opnrelay.dev>
Homepage: https://github.com/stevy2191/opnrelay
Description: OpnRelay desktop recorder
 Records step-by-step guides on your desktop and uploads them to your
 organisation's self-hosted OpnRelay server.
CTRL
mkdir -p "$OUT"
dpkg-deb --root-owner-group --build "$ROOT" "$OUT/OpnRelay-Recorder-Linux-${ARCH}.deb" >/dev/null
tar -C "$(dirname "$BIN")" -czf "$OUT/OpnRelay-Recorder-Linux-${ARCH}.tar.gz" "$(basename "$BIN")"
rm -rf "$ROOT"
echo "built $OUT/OpnRelay-Recorder-Linux-${ARCH}.deb"
