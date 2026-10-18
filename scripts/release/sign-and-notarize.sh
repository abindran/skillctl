#!/bin/sh
# Sign a binary with a Developer ID certificate and notarize it.
#
# Usage: sign-and-notarize.sh <binary>
# Environment: APPLE_CERTIFICATE_P12 (base64), APPLE_CERTIFICATE_PASSWORD,
#              APPLE_SIGNING_IDENTITY ("Developer ID Application: …"),
#              APPLE_ID, APPLE_TEAM_ID, APPLE_APP_PASSWORD.
# Binaries installed with cargo install or from a Homebrew tap do not need this;
# it matters for binaries downloaded through a browser (Gatekeeper quarantine).
set -eu
binary="${1:?binary path}"
: "${APPLE_CERTIFICATE_P12:?}" "${APPLE_CERTIFICATE_PASSWORD:?}" "${APPLE_SIGNING_IDENTITY:?}"
: "${APPLE_ID:?}" "${APPLE_TEAM_ID:?}" "${APPLE_APP_PASSWORD:?}"

work="$(mktemp -d)"
keychain="$work/signing.keychain-db"
keychain_password="$(openssl rand -hex 16)"
cleanup() { security delete-keychain "$keychain" 2>/dev/null || true; rm -rf "$work"; }
trap cleanup EXIT

printf '%s' "$APPLE_CERTIFICATE_P12" | base64 --decode > "$work/certificate.p12"
security create-keychain -p "$keychain_password" "$keychain"
security set-keychain-settings -lut 1800 "$keychain"
security unlock-keychain -p "$keychain_password" "$keychain"
security import "$work/certificate.p12" -k "$keychain" -P "$APPLE_CERTIFICATE_PASSWORD" \
  -T /usr/bin/codesign -T /usr/bin/security
security set-key-partition-list -S apple-tool:,apple: -s -k "$keychain_password" "$keychain" >/dev/null
security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')

codesign --force --options runtime --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$binary"
codesign --verify --strict --verbose=2 "$binary"

# notarytool accepts zip archives; a bare Mach-O cannot be stapled, so the
# notarization ticket is looked up online by Gatekeeper on first launch.
ditto -c -k --keepParent "$binary" "$work/notarize.zip"
xcrun notarytool submit "$work/notarize.zip" \
  --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_PASSWORD" \
  --wait
echo "Signed and notarized $binary"
