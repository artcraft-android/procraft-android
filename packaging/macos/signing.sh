#!/usr/bin/env bash
# The release's Developer ID signing on a CI Mac: `--setup` imports the certificate into a
# keychain of the job's own and tells package.sh (through $GITHUB_ENV) to sign and notarize with
# it; `--cleanup` removes the keychain and the key. Without the secrets (forks, dry runs) setup
# does nothing and package.sh signs ad hoc.
#
# Secrets (as environment variables):
#   APPLE_CERTIFICATE_P12        base64 of the "Developer ID Application" certificate and key (.p12)
#   APPLE_CERTIFICATE_PASSWORD   that .p12's password
#   APPLE_API_KEY_P8             the App Store Connect API key's text (AuthKey_<id>.p8), to notarize
#   APPLE_API_KEY_ID             its key ID
#   APPLE_API_ISSUER             its issuer ID
set -euo pipefail

KEYCHAIN="${RUNNER_TEMP:-/tmp}/procraft-signing.keychain-db"
KEY_FILE="${RUNNER_TEMP:-/tmp}/procraft-notary-key.p8"
summary() { [ -n "${GITHUB_STEP_SUMMARY:-}" ] && echo "$1" >>"$GITHUB_STEP_SUMMARY"; echo "$1"; }

case "${1:-}" in
--setup)
  if [ -z "${APPLE_CERTIFICATE_P12:-}" ]; then
    summary "macOS: no Developer ID secrets: signed ad hoc, not notarized"
    exit 0
  fi
  : "${APPLE_CERTIFICATE_PASSWORD:?set APPLE_CERTIFICATE_PASSWORD with APPLE_CERTIFICATE_P12}"
  password="$(openssl rand -hex 24)"
  certificate="${RUNNER_TEMP:-/tmp}/procraft-signing.p12"
  printf '%s' "$APPLE_CERTIFICATE_P12" | base64 --decode >"$certificate"
  security create-keychain -p "$password" "$KEYCHAIN"
  security set-keychain-settings -lut 21600 "$KEYCHAIN"
  security unlock-keychain -p "$password" "$KEYCHAIN"
  security import "$certificate" -k "$KEYCHAIN" -P "$APPLE_CERTIFICATE_PASSWORD" -T /usr/bin/codesign
  rm -f "$certificate"
  security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$password" "$KEYCHAIN" >/dev/null
  # the job's keychain searched first, the others kept
  security list-keychains -d user -s "$KEYCHAIN" $(security list-keychains -d user | tr -d '"')
  identity="$(security find-identity -v -p codesigning "$KEYCHAIN" | sed -n 's/.*) \([0-9A-F]\{40\}\) "Developer ID Application.*/\1/p' | head -1)"
  if [ -z "$identity" ]; then
    echo "::error::the certificate holds no Developer ID Application identity"
    exit 1
  fi
  echo "APPLE_SIGN_IDENTITY=$identity" >>"$GITHUB_ENV"
  if [ -n "${APPLE_API_KEY_P8:-}" ]; then
    : "${APPLE_API_KEY_ID:?set APPLE_API_KEY_ID with APPLE_API_KEY_P8}"
    : "${APPLE_API_ISSUER:?set APPLE_API_ISSUER with APPLE_API_KEY_P8}"
    printf '%s\n' "$APPLE_API_KEY_P8" >"$KEY_FILE"
    {
      echo "APPLE_API_KEY_PATH=$KEY_FILE"
      echo "APPLE_API_KEY_ID=$APPLE_API_KEY_ID"
      echo "APPLE_API_ISSUER_ID=$APPLE_API_ISSUER"
    } >>"$GITHUB_ENV"
    summary "macOS: signed with a Developer ID and notarized"
  else
    summary "macOS: signed with a Developer ID, not notarized (no API key)"
  fi
  ;;
--cleanup)
  security delete-keychain "$KEYCHAIN" 2>/dev/null || true
  rm -f "$KEY_FILE"
  ;;
*)
  echo "usage: signing.sh --setup | --cleanup" >&2
  exit 2
  ;;
esac
