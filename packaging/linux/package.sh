#!/usr/bin/env bash
# Packages the desktop app for Linux into $DIST (default dist/), <arch> being this machine's:
#
#   procraft-<version>-linux-<arch>.AppImage   any distro with glibc as new as the build host's
#   procraft-<version>-linux-<arch>.deb        Debian, Ubuntu, Mint, Pop!_OS, ...
#   procraft-<version>-linux-<arch>.tar.gz     the plain tree (bin/, share/)
#
# Usage: packaging/linux/package.sh [--skip-build] [--formats "appimage deb tar"]
#
# VERSION is the release's (default: procraft.version in android/gradle.properties). Needs cargo,
# nfpm for the .deb (https://nfpm.goreleaser.com) and appimagetool for the AppImage (downloaded
# into the target folder if missing). Build on an old distro (CI: Ubuntu 22.04) so the binary
# runs on newer ones. Modelled on SolidCraft's packaging (tangency-labs/solidcraft).
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
HERE="$ROOT/packaging/linux"
SKIP_BUILD=0
FORMATS="appimage deb tar"
while [ $# -gt 0 ]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    --formats) FORMATS="$2"; shift 2 ;;
    -h | --help) sed -n '2,13p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
VERSION="${VERSION:-$(sed -n 's/^procraft\.version=//p' "$ROOT/android/gradle.properties" | tr -d '\r')}"
DIST="$(mkdir -p "${DIST:-$ROOT/dist}" && cd "${DIST:-$ROOT/dist}" && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64) DEB_ARCH=amd64 ;;
  aarch64 | arm64) ARCH=aarch64; DEB_ARCH=arm64 ;;
  *) echo "unsupported architecture $ARCH" >&2; exit 2 ;;
esac
BASENAME="procraft-$VERSION-linux-$ARCH"
has() { case " $FORMATS " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }
echo "==> ProCraft $VERSION for Linux $ARCH ($FORMATS)"

if [ "$SKIP_BUILD" = 0 ]; then
  (cd "$ROOT" && cargo build --release --locked -p procraft-desktop)
fi

# ---- the tree every format holds -----------------------------------------------------------------
WORK="$TARGET_DIR/linux-package"
STAGE="$WORK/root"
rm -rf "$WORK"
install -Dm755 "$TARGET_DIR/release/procraft" "$STAGE/usr/bin/procraft"
strip "$STAGE/usr/bin/procraft" 2>/dev/null || true
install -Dm644 "$HERE/procraft.desktop" "$STAGE/usr/share/applications/procraft.desktop"
mkdir -p "$STAGE/usr/share/icons"
cp -R "$ROOT/assets/app-icon/hicolor" "$STAGE/usr/share/icons/"
for f in LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md; do
  install -Dm644 "$ROOT/$f" "$STAGE/usr/share/doc/procraft/$f"
done
if command -v desktop-file-validate >/dev/null; then
  desktop-file-validate "$STAGE/usr/share/applications/procraft.desktop"
fi

# ---- .tar.gz -------------------------------------------------------------------------------------
if has tar; then
  mkdir -p "$WORK/tar"
  cp -R "$STAGE/usr" "$WORK/tar/$BASENAME"
  tar -C "$WORK/tar" -czf "$DIST/$BASENAME.tar.gz" "$BASENAME"
  echo "wrote $DIST/$BASENAME.tar.gz"
fi

# ---- .deb ----------------------------------------------------------------------------------------
if has deb; then
  command -v nfpm >/dev/null || { echo "error: nfpm not found (https://nfpm.goreleaser.com/install/)" >&2; exit 1; }
  export VERSION NFPM_ARCH="$DEB_ARCH"
  # nfpm expands variables in fields like `version`, but not in `contents[].src`
  sed "s|\${STAGE}|$STAGE|g" "$HERE/nfpm.yaml" >"$WORK/nfpm.yaml"
  (cd "$ROOT" && nfpm package -f "$WORK/nfpm.yaml" -p deb -t "$DIST/$BASENAME.deb")
fi

# ---- AppImage ------------------------------------------------------------------------------------
if has appimage; then
  APPDIR="$WORK/ProCraft.AppDir"
  cp -R "$STAGE" "$APPDIR"
  rm -rf "$APPDIR/usr/share/doc"
  install -Dm755 "$HERE/AppRun" "$APPDIR/AppRun"
  cp "$HERE/procraft.desktop" "$APPDIR/procraft.desktop"
  cp "$ROOT/assets/app-icon/hicolor/256x256/apps/procraft.png" "$APPDIR/procraft.png"
  ln -s procraft.png "$APPDIR/.DirIcon"
  TOOL="${APPIMAGETOOL:-$(command -v appimagetool || true)}"
  if [ -z "$TOOL" ]; then
    TOOL="$TARGET_DIR/appimagetool-$ARCH.AppImage"
    if [ ! -x "$TOOL" ]; then
      curl -fsSL -o "$TOOL" "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$ARCH.AppImage"
      chmod +x "$TOOL"
    fi
  fi
  # extracted and run, so it needs no FUSE (containers, CI)
  ARCH="$ARCH" APPIMAGE_EXTRACT_AND_RUN=1 "$TOOL" --no-appstream "$APPDIR" "$DIST/$BASENAME.AppImage"
  echo "wrote $DIST/$BASENAME.AppImage"
fi

echo "==> done"
ls -l "$DIST"
