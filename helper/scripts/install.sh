#!/bin/bash
# Install clash-tiny-helper as a LaunchDaemon (requires root)
set -e

HELPER_BIN="$1"
PLIST_SRC="$2"
DEST_BIN="/Library/PrivilegedHelperTools/com.clash-tiny.helper"
DEST_PLIST="/Library/LaunchDaemons/com.clash-tiny.helper.plist"

if [ -z "$HELPER_BIN" ] || [ -z "$PLIST_SRC" ]; then
    echo "Usage: install.sh <helper_binary> <plist_file>"
    exit 1
fi

if [ ! -f "$HELPER_BIN" ]; then
    echo "Error: helper binary not found at $HELPER_BIN"
    exit 1
fi

# Stop existing service if running
if launchctl list | grep -q "com.clash-tiny.helper"; then
    launchctl unload "$DEST_PLIST" 2>/dev/null || true
fi

# Create target directory
mkdir -p /Library/PrivilegedHelperTools

# Copy files
cp "$HELPER_BIN" "$DEST_BIN"
chmod 755 "$DEST_BIN"
chown root:wheel "$DEST_BIN"

cp "$PLIST_SRC" "$DEST_PLIST"
chmod 644 "$DEST_PLIST"
chown root:wheel "$DEST_PLIST"

# Load the daemon
launchctl load -w "$DEST_PLIST"

echo "[install] clash-tiny-helper installed and started."
