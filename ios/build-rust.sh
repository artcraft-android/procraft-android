#!/usr/bin/env bash
# Builds the core (crates/core) for the iPad app as a static library, with its Swift bindings and
# the app's icon, into ios/Generated:
#
#   lib/<platform>/libprocraft_core.a     the core for the iPad (iphoneos) or the simulator
#                                         (iphonesimulator), always optimized (a debug engine is too
#                                         slow to paint with)
#   ProCraftCore.swift                    the bindings, compiled into the app
#   include/ProCraftCoreFFI.h             their C half, a module on the Swift import path
#   include/module.modulemap
#   Assets.xcassets                       the app icon (assets/app-icon/procraft-1024.png)
#
# Usage: ios/build-rust.sh [iphoneos | iphonesimulator]
#
# Xcode runs it before each build (PLATFORM_NAME says which); build-ipa.sh runs it before making
# the project. Needs Rust with the aarch64-apple-ios and aarch64-apple-ios-sim targets. The static
# library comes from the same crate as Android's .so (`cargo rustc --crate-type staticlib`), so its
# Cargo.toml is left as it is.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/ios/Generated"
PLATFORM="${1:-${PLATFORM_NAME:-iphoneos}}"
case "$PLATFORM" in
  iphoneos) TARGET=aarch64-apple-ios ;;
  iphonesimulator) TARGET=aarch64-apple-ios-sim ;;
  *) echo "unknown platform: $PLATFORM" >&2; exit 2 ;;
esac
# cargo as the user's shell has it; Xcode's settings are the app's, not those of cargo's host tools
# (an SDKROOT of the iPad's would have the build scripts linked for it), which link with the Mac's
export PATH="$HOME/.cargo/bin:$PATH"
unset SDKROOT
if [ -n "${DEVELOPER_SDK_DIR:-}" ]; then
  export LIBRARY_PATH="$DEVELOPER_SDK_DIR/MacOSX.sdk/usr/lib${LIBRARY_PATH:+:$LIBRARY_PATH}"
fi
export IPHONEOS_DEPLOYMENT_TARGET=17.0
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"

cd "$ROOT"
echo "==> the core for $PLATFORM ($TARGET)"
cargo rustc --locked --release --target "$TARGET" -p procraft-core --lib --crate-type staticlib
mkdir -p "$OUT/lib/$PLATFORM" "$OUT/include"
cp "$TARGET_DIR/$TARGET/release/libprocraft_core.a" "$OUT/lib/$PLATFORM/libprocraft_core.a"

echo "==> its Swift bindings"
BINDINGS="$OUT/bindings"
rm -rf "$BINDINGS"
cargo run --quiet --locked -p procraft-bindgen -- generate "$OUT/lib/$PLATFORM/libprocraft_core.a" \
  --language swift --out-dir "$BINDINGS" --no-format
# written only when changed, so Xcode doesn't compile them again for nothing
update() { cmp -s "$1" "$2" || cp "$1" "$2"; }
update "$BINDINGS/ProCraftCore.swift" "$OUT/ProCraftCore.swift"
update "$BINDINGS/ProCraftCoreFFI.h" "$OUT/include/ProCraftCoreFFI.h"
update "$BINDINGS/ProCraftCoreFFI.modulemap" "$OUT/include/module.modulemap"
rm -rf "$BINDINGS"

echo "==> the app icon"
ICON="$OUT/Assets.xcassets/AppIcon.appiconset"
mkdir -p "$ICON"
printf '{"info":{"author":"xcode","version":1}}\n' >"$OUT/Assets.xcassets/Contents.json"
cat >"$ICON/Contents.json" <<'JSON'
{
  "images": [{ "filename": "AppIcon.png", "idiom": "universal", "platform": "ios", "size": "1024x1024" }],
  "info": { "author": "xcode", "version": 1 }
}
JSON
update "$ROOT/assets/app-icon/procraft-1024.png" "$ICON/AppIcon.png"
