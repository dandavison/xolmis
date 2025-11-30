#!/bin/bash
# Benchmark: run delta through xolmis via tmux
cd "$(dirname "$0")/.."

SESSION="xolmis_bench_$$"
trap "tmux kill-session -t $SESSION 2>/dev/null" EXIT

tmux new-session -d -s "$SESSION" -x 80 -y 24 ./target/release/xolmis
sleep 0.3

tmux send-keys -t "$SESSION" "git log -100 | delta --no-gitconfig --paging=never >/dev/null 2>&1; exit" Enter

# Wait for xolmis to exit (session will terminate)
for i in $(seq 1 30); do
    tmux has-session -t "$SESSION" 2>/dev/null || exit 0
    sleep 0.1
done
