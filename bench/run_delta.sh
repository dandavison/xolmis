#!/bin/bash
# Benchmark: run delta via tmux, optionally through xolmis
# Usage: run_delta.sh [xolmis]

cd "$(dirname "$0")/.."

USE_XOLMIS="${1:-}"
SESSION="bench_$$"
DONE_SIGNAL="bench_done_$$"

trap "tmux kill-session -t $SESSION 2>/dev/null" EXIT

if [ "$USE_XOLMIS" = "xolmis" ]; then
    tmux new-session -d -s "$SESSION" -x 80 -y 24 ./target/release/xolmis
else
    tmux new-session -d -s "$SESSION" -x 80 -y 24
fi

sleep 0.3

# Run the workload, then signal completion
tmux send-keys -t "$SESSION" "for i in \$(seq 1 10); do git log -100 | delta --no-gitconfig --paging=never >/dev/null 2>&1; done; tmux wait-for -S $DONE_SIGNAL; exit" Enter

# Wait for the signal
tmux wait-for "$DONE_SIGNAL"
