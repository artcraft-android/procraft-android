#!/usr/bin/env bash
# Makes the iPad app's unsigned .ipa into $DIST (default dist/), on a Mac (CI: a macOS runner):
#
#   procraft-<version>-ipados-unsigned.ipa   signed by whoever installs it (with their own Apple
#                                            ID: AltStore, SideStore, Sideloadly, iloader, ...)
#
# Usage: ios/build-ipa.sh
#
# VERSION and BUILD as packaging/macos/package.sh has them (default: android/gradle.properties).
# Needs Xcode, XcodeGen (brew install xcodegen) and Rust with the aarch64-apple-ios target. No
# Apple account: nothing is signed (CODE_SIGNING_ALLOWED=NO).
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROPERTIES="$ROOT/android/gradle.properties"
VERSION="${VERSION:-$(sed -n 's/^procraft\.version=//p' "$PROPERTIES" | tr -d '\r')}"
BUILD="${BUILD:-$(sed -n 's/^procraft\.versionCode=//p' "$PROPERTIES" | tr -d '\r')}"
[ -n "$VERSION" ] && [ -n "$BUILD" ] || { echo "no version: set VERSION and BUILD or see $PROPERTIES" >&2; exit 2; }
# the bundle's own version is numbers only
SHORT="${VERSION%%-*}"
DIST="$(mkdir -p "${DIST:-$ROOT/dist}" && cd "${DIST:-$ROOT/dist}" && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
WORK="$TARGET_DIR/ios-package"
IPA="$DIST/procraft-$VERSION-ipados-unsigned.ipa"
echo "==> ProCraft $VERSION ($SHORT, build $BUILD) for iPadOS, unsigned"

# the core and its bindings first: the project lists the files they make
bash "$ROOT/ios/build-rust.sh" iphoneos
(cd "$ROOT/ios" && xcodegen generate --spec project.yml)

rm -rf "$WORK"
xcodebuild -project "$ROOT/ios/ProCraft.xcodeproj" -scheme ProCraft -configuration Release \
  -sdk iphoneos -destination 'generic/platform=iOS' -archivePath "$WORK/ProCraft.xcarchive" archive \
  CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO CODE_SIGN_IDENTITY="" \
  MARKETING_VERSION="$SHORT" CURRENT_PROJECT_VERSION="$BUILD"

APP="$WORK/ProCraft.xcarchive/Products/Applications/ProCraft.app"
[ -d "$APP" ] || { echo "error: the archive has no ProCraft.app" >&2; exit 1; }
# an .ipa is the app in a Payload folder, zipped
mkdir -p "$WORK/ipa/Payload"
ditto "$APP" "$WORK/ipa/Payload/ProCraft.app"
rm -f "$IPA"
(cd "$WORK/ipa" && zip -qry "$IPA" Payload)
echo "wrote $IPA"
ls -l "$DIST"
