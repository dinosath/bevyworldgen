#!/usr/bin/env bash
# Spike S-6: native vs wasm32 golden hashes must be identical.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build -q -p wasm-golden --release --target wasm32-unknown-unknown
cargo run -q -p wasm-golden --release --bin golden-native > /tmp/golden-native.txt
node scripts/wasm-golden.mjs target/wasm32-unknown-unknown/release/wasm_golden.wasm > /tmp/golden-wasm.txt
diff /tmp/golden-native.txt /tmp/golden-wasm.txt && echo "native == wasm32 for $(wc -l < /tmp/golden-native.txt) chunks"
