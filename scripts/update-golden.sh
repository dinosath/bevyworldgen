#!/usr/bin/env bash
# Regenerate the pinned golden hashes after an *intentional* change to generator output.
set -euo pipefail
cd "$(dirname "$0")/.."
out=$(GOLDEN_PRINT=1 cargo test -q -p worldgen-terrain --release --test determinism golden -- --nocapture 2>&1 | grep -o 'GOLDEN \[.*\]' | head -1 || true)
vals=$(echo "$out" | sed -E 's/GOLDEN \[//; s/\]//; s/, / /g')
arr=""; for v in $vals; do arr="${arr}0x${v}, "; done
arr=$(echo "$arr" | sed -E 's/, $//')
count=$(echo $vals | wc -w)
sed -i -E "s/^const GOLDEN: \[u64; [0-9]+\] = .*/const GOLDEN: [u64; $count] = [$arr];/" crates/worldgen-terrain/tests/determinism.rs
grep -n '^const GOLDEN' crates/worldgen-terrain/tests/determinism.rs
