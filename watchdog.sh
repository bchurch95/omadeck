#!/usr/bin/env bash
# /root/omashow/watchdog.sh
# Check if pi has been running for longer than 75 minutes without updating
set -euo pipefail

# Find pi process
PID=$(pgrep -f "^pi " || true)
if [ -n "$PID" ]; then
    # Check elapsed time in seconds
    ELAPSED=$(ps -o etimes= -p "$PID" | tr -d ' ' || echo 0)
    # 75 minutes = 4500 seconds
    if [ "$ELAPSED" -gt 4500 ]; then
        echo "[watchdog] Pi process $PID running for $ELAPSED seconds (>4500s). Terminating hung runner..."
        kill -9 "$PID" 2>/dev/null || true
    fi
fi
