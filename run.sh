#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

LOG_LEVEL="${RUST_LOG:-info}"
TMPLOG=$(mktemp /tmp/tendril-XXXX.log)
RUST_LOG="$LOG_LEVEL" cargo run --bin tendril > "$TMPLOG" 2>&1 &
COMPOSITOR_PID=$!

for i in $(seq 1 10); do
  SOCKET=$(grep -oP 'WAYLAND_DISPLAY="?\K[^"]+' "$TMPLOG" 2>/dev/null || true)
  if [ -n "$SOCKET" ]; then
    break
  fi
  sleep 0.5
done

echo "tendril compositor ready on $SOCKET (PID $COMPOSITOR_PID)"
echo "Log: $TMPLOG"
echo "Launching 3 kitty windows..."

for i in 1 2 3; do
  WAYLAND_DISPLAY=$SOCKET kitty --override hide_window_decorations=yes &
  sleep 0.3
done

wait $COMPOSITOR_PID
