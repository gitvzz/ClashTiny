#!/bin/bash
# Uninstall clash-tiny-helper LaunchDaemon (requires root)
set -e

DEST_BIN="/Library/PrivilegedHelperTools/com.clash-tiny.helper"
DEST_PLIST="/Library/LaunchDaemons/com.clash-tiny.helper.plist"
SOCKET="/var/run/clash-tiny-helper.sock"

# Stop the daemon
if launchctl list | grep -q "com.clash-tiny.helper"; then
    launchctl unload "$DEST_PLIST" 2>/dev/null || true
fi

# Remove files
rm -f "$DEST_BIN"
rm -f "$DEST_PLIST"
rm -f "$SOCKET"

echo "[uninstall] clash-tiny-helper removed."
