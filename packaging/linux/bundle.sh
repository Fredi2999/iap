#!/usr/bin/env bash
# IAP Linux-AppImage-Bundler (Konzept 16).
#
# Baut eine AppImage-Datei mit gebündeltem WebKitGTK aus dem lokalen
# Release-Build. Setzt lokal installiertes appimagetool (>= 1.9) voraus;
# ohne appimagetool wird ehrlich abgebrochen (kein Fake-Bundle).

set -euo pipefail

if [[ "${TRACE:-0}" == "1" ]]; then
  set -x
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DIST_ROOT="$REPO_ROOT/dist/iap-linux"
APPDIR="$DIST_ROOT/IAP.AppDir"
BINARY_SRC="${1:-$REPO_ROOT/target/release/iap}"

if ! command -v appimagetool >/dev/null 2>&1; then
  echo "Fehler: appimagetool nicht im PATH. Installation: https://appimage.github.io/appimagetool/" >&2
  exit 2
fi

if [[ ! -x "$BINARY_SRC" ]]; then
  echo "Fehler: Release-Binary nicht gefunden unter $BINARY_SRC" >&2
  echo "Baue mit: cargo build --release -p portable-ai-app" >&2
  exit 2
fi

# WebKitGTK-Bibliotheken finden (Ubuntu/Debian-Namen; andere Distros können
# weitere Namen brauchen — im Zweifelsfall über LIB_HINTS ergänzen).
LIB_HINTS=(
  libwebkit2gtk-4.1.so.0
  libjavascriptcoregtk-4.1.so.0
  libgtk-3.so.0
  libgdk-3.so.0
  libsoup-3.0.so.0
)

echo "==> Räume $APPDIR auf"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/lib"

echo "==> Kopiere Binary"
cp -v "$BINARY_SRC" "$APPDIR/usr/bin/iap"

echo "==> Bündele WebKitGTK-Bibliotheken"
for lib in "${LIB_HINTS[@]}"; do
  path=$(ldconfig -p | awk -v lib="$lib" '$1 == lib { print $NF; exit }' || true)
  if [[ -z "$path" ]]; then
    echo "  Warnung: $lib nicht gefunden — Distros mit älterem WebKitGTK (4.0) müssen den Namen selbst ergänzen." >&2
    continue
  fi
  cp -v "$path" "$APPDIR/usr/lib/"
done

echo "==> Schreibe AppRun mit noexec-Erkennung"
cat > "$APPDIR/AppRun" <<'RUNSCRIPT'
#!/usr/bin/env bash
# IAP Launcher mit noexec-Erkennung (Konzept 16).
set -euo pipefail
HERE="$(dirname "$(readlink -f "$0")")"
export PATH="$HERE/usr/bin:${PATH:-}"
export LD_LIBRARY_PATH="$HERE/usr/lib:${LD_LIBRARY_PATH:-}"

check_noexec() {
  local path="$1"
  local mount
  mount=$(df --output=target "$path" 2>/dev/null | tail -n1 || true)
  if [[ -z "$mount" ]]; then
    return 0
  fi
  if grep -Fq "noexec" <(findmnt -no OPTIONS "$mount" 2>/dev/null); then
    cat <<MSG >&2
IAP: Der USB-Stick ist mit \`noexec\` eingehängt, daher darf
diese AppImage nicht direkt vom Stick starten. Zwei einfache Auswege:

  1. Stick neu einhängen ohne \`noexec\`, z. B.
     sudo mount -o remount,exec "$mount"
  2. AppImage lokal kopieren und dort starten, z. B.
     cp -v "$0" ~/IAP.AppImage && ~/IAP.AppImage

Nach dem Wechsel wird IAP wieder direkt startfähig.
MSG
    exit 3
  fi
}

check_noexec "$HERE"
exec "$HERE/usr/bin/iap" "$@"
RUNSCRIPT
chmod +x "$APPDIR/AppRun"

echo "==> Schreibe Desktop-Datei"
cat > "$APPDIR/iap.desktop" <<DESK
[Desktop Entry]
Type=Application
Name=IAP
Exec=iap
Icon=iap
Categories=Utility;
DESK

if [[ -f "$REPO_ROOT/packaging/linux/icon.png" ]]; then
  cp "$REPO_ROOT/packaging/linux/icon.png" "$APPDIR/iap.png"
else
  # Platzhalter (1x1-Transparent-PNG), damit appimagetool nicht meckert.
  printf '\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1f\x15\xc4\x89\x00\x00\x00\rIDATx\x9cc\xf8\xff\xff?\x00\x05\xfe\x02\xfe\xdc\xccY\xe7\x00\x00\x00\x00IEND\xaeB`\x82' > "$APPDIR/iap.png"
fi

echo "==> Baue AppImage"
mkdir -p "$DIST_ROOT"
ARCH=$(uname -m)
export ARCH
appimagetool "$APPDIR" "$DIST_ROOT/IAP-${ARCH}.AppImage"

echo "==> Fertig: $DIST_ROOT/IAP-${ARCH}.AppImage"
