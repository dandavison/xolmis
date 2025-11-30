#!/bin/bash
# Benchmark: run delta via tmux, optionally through xolmis
# Usage: run_delta.sh [xolmis]

cd "$(dirname "$0")/.."

USE_XOLMIS="${1:-}"
SESSION="bench_$$"
trap "tmux kill-session -t $SESSION 2>/dev/null" EXIT

if [ "$USE_XOLMIS" = "xolmis" ]; then
    tmux new-session -d -s "$SESSION" -x 80 -y 24 ./target/release/xolmis
else
    tmux new-session -d -s "$SESSION" -x 80 -y 24
fi

sleep 0.3

tmux send-keys -t "$SESSION" "git log -100 | delta --no-gitconfig --paging=never >/dev/null 2>&1; exit" Enter

for i in $(seq 1 30); do
    tmux has-session -t "$SESSION" 2>/dev/null || exit 0
    sleep 0.1
done

