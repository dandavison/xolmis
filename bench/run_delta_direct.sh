#!/bin/bash
# Benchmark: run delta directly via tmux (no xolmis)
cd "$(dirname "$0")/.."

SESSION="bench_direct_$$"
trap "tmux kill-session -t $SESSION 2>/dev/null" EXIT

tmux new-session -d -s "$SESSION" -x 80 -y 24

sleep 0.3

tmux send-keys -t "$SESSION" "git log -100 | delta --no-gitconfig --paging=never >/dev/null 2>&1; exit" Enter

for i in $(seq 1 30); do
    tmux has-session -t "$SESSION" 2>/dev/null || exit 0
    sleep 0.1
done
