#!/bin/sh
# Cargo runner for macOS dev builds. Signing follows the Tauri bundler's rule: use
# APPLE_SIGNING_IDENTITY when set, otherwise leave the binary as it is. A fixed identifier
# keeps the code identity stable between rebuilds, so Keychain stops asking for the login
# password on every launch.
set -eu
binary="$1"
shift
if [ -n "${APPLE_SIGNING_IDENTITY:-}" ] && [ "$(basename "$binary")" = "overlingo" ]; then
    codesign --force --sign "$APPLE_SIGNING_IDENTITY" \
        --identifier com.deanwfy.overlingo.dev \
        --entitlements "$(dirname "$0")/../src-tauri/Entitlements.plist" \
        "$binary"
fi
exec "$binary" "$@"
