#!/bin/bash
# Rebuild and restart the panel pair. Run this after every change to the backend or the UI.
#
# JJ asked for this because a running panel keeps serving the binary it started with, even after
# the file underneath it is replaced. Twice he was looking at a window that predated the fix I
# had just told him about, which is worse than no fix: it looks like the change did nothing.
set -uo pipefail
cd "$(dirname "$0")" || exit 1

echo "building..."
cargo build --release 2>&1 | tail -1 || exit 1
(cd panel && cargo build --release 2>&1 | tail -1) || exit 1

# Deploy first, so the services and the panel are all running the same build. A stale
# ~/.local/bin/carl is what made Carl keep saying he had never heard of Miles: the fix was
# built, the panel was restarted, and the binary the services run was days older.
cp target/release/carl "$HOME/.local/bin/carl.new" 2>/dev/null && \
  mv -f "$HOME/.local/bin/carl" "$HOME/.local/bin/carl.prev" 2>/dev/null && \
  mv -f "$HOME/.local/bin/carl.new" "$HOME/.local/bin/carl" && echo "deployed to ~/.local/bin/carl"

# The two cheap services. carl-army is deliberately not restarted here: it would end ten claude
# processes and start ten replacements, which is real money for a rebuild that may not touch
# them. Restart it explicitly when the change is one the agents need.
systemctl --user restart carl-slack carl-listen 2>/dev/null && echo "restarted slack and listen"

# Backend next. It unlinks its socket on SIGTERM, so the panel reconnects rather than sitting
# on a dead one.
for pat in "release/carl-panel" "release/carl panel"; do
  PID=$(pgrep -f "$pat" | head -1)
  [ -n "$PID" ] && kill "$PID" 2>/dev/null && echo "stopped $pat ($PID)"
done
sleep 2

nohup ./target/release/carl panel > /tmp/carl-backend.log 2>&1 &
sleep 2
nohup ./target/release/carl-panel > /tmp/carl-panel.log 2>&1 &
sleep 3

echo "backend: $(head -1 /tmp/carl-backend.log)"
echo "panel:   $(head -1 /tmp/carl-panel.log)"
pgrep -f "release/carl-panel" >/dev/null && echo "panel is up" || echo "PANEL DID NOT START"
