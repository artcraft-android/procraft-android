#!/usr/bin/env bash
# Builds the HarmonyOS app's native library and icons into harmony/app:
#
#   entry/libs/<abi>/libprocraft_harmony.so   the core and its Node-API bridge (harmony/bridge),
#                                             always optimized (a debug engine is too slow)
#   entry/types/libprocraft_harmony/index.d.ts its ArkTS declarations, when ohrs writes them
#   AppScope/resources/base/media/app_icon.png the app icon (assets/app-icon/procraft-1024.png)
#   entry/src/main/resources/base/media/icon.png
#
# Usage: harmony/build-rust.sh [arm64] [x86_64]     (default arm64: the tablets; x86_64: the
#                                                    emulator on an Intel or AMD PC)
#
# Needs Rust with the aarch64-unknown-linux-ohos target (x86_64-unknown-linux-ohos for the
# emulator) and the native part of the HarmonyOS or OpenHarmony SDK: OHOS_NDK_HOME is the folder
# that holds native/llvm and native/sysroot (DevEco Studio's <sdk>/default/openharmony). With ohrs
# (`cargo install ohrs`) the build goes through it, which also writes the declarations; without, it
# is cargo with the SDK's clang as the linker. Then open harmony/app in DevEco Studio.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="$ROOT/harmony/app"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
: "${OHOS_NDK_HOME:?set OHOS_NDK_HOME to the SDK folder that holds native/ (DevEco Studio: <sdk>/default/openharmony)}"
[ -d "$OHOS_NDK_HOME/native/sysroot" ] || { echo "no native/sysroot in $OHOS_NDK_HOME" >&2; exit 2; }
[ "$#" -gt 0 ] || set -- arm64

cd "$ROOT"
for abi in "$@"; do
  case "$abi" in
    arm64) TARGET=aarch64-unknown-linux-ohos; CLANG_TARGET=aarch64-linux-ohos; DIR=arm64-v8a ;;
    x86_64) TARGET=x86_64-unknown-linux-ohos; CLANG_TARGET=x86_64-linux-ohos; DIR=x86_64 ;;
    *) echo "unknown ABI: $abi (arm64 or x86_64)" >&2; exit 2 ;;
  esac
  echo "==> the core and its bridge for $abi ($TARGET)"
  mkdir -p "$APP/entry/libs/$DIR"
  if command -v ohrs >/dev/null 2>&1; then
    # writes harmony/bridge/dist/<abi dir>/ and dist/index.d.ts
    ohrs build -p procraft-harmony --release --arch "$abi"
    cp "$ROOT/harmony/bridge/dist/$DIR/libprocraft_harmony.so" "$APP/entry/libs/$DIR/"
    cp "$ROOT/harmony/bridge/dist/index.d.ts" "$APP/entry/types/libprocraft_harmony/index.d.ts"
  else
    LLVM="$OHOS_NDK_HOME/native/llvm/bin"
    CLANG="$LLVM/clang"
    [ -x "$CLANG.exe" ] && CLANG="$CLANG.exe"
    UPPER="$(echo "$TARGET" | tr 'a-z-' 'A-Z_')"
    export "CARGO_TARGET_${UPPER}_LINKER=$CLANG"
    export "CARGO_TARGET_${UPPER}_RUSTFLAGS=-C link-arg=--target=$CLANG_TARGET -C link-arg=--sysroot=$OHOS_NDK_HOME/native/sysroot"
    cargo build --locked --release --target "$TARGET" -p procraft-harmony
    cp "$TARGET_DIR/$TARGET/release/libprocraft_harmony.so" "$APP/entry/libs/$DIR/"
  fi
done

echo "==> the app icon"
# written only when changed, so DevEco doesn't package them again for nothing
update() { mkdir -p "$(dirname "$2")"; cmp -s "$1" "$2" || cp "$1" "$2"; }
update "$ROOT/assets/app-icon/procraft-1024.png" "$APP/AppScope/resources/base/media/app_icon.png"
update "$ROOT/assets/app-icon/procraft-1024.png" "$APP/entry/src/main/resources/base/media/icon.png"
