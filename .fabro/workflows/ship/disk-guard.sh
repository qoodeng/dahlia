#!/bin/sh
# Refuses to start a run's build when the disk is nearly full, instead of filling it.
# A cold Dahlia build needs several GB; ~/.local/bin/rust-target-sweep frees stale ones daily.
set -eu

need_gb=20
free_gb=$(df -g . | awk 'NR == 2 { print $4 }')
if [ "$free_gb" -lt "$need_gb" ]; then
    echo "Only ${free_gb} GB free; a run needs ${need_gb} GB. Run rust-target-sweep or free space, then retry." >&2
    exit 1
fi
echo "disk: ${free_gb} GB free"
