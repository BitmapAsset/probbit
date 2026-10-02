#!/bin/sh
# Build the playground's module (wasm32-unknown-unknown, release): playground/probbit.wasm for a page served over http, and
# playground/probbit-wasm.js, the same bytes in base64, for index.html opened from disk (browsers do not let a file:// page fetch).
# Needs the target: rustup target add wasm32-unknown-unknown
set -e
cd "$(dirname "$0")/.."
cargo build --release --locked -p probbit-wasm --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/probbit_wasm.wasm playground/probbit.wasm
printf 'var PROBBIT_WASM_B64 = "%s";\n' "$(base64 < playground/probbit.wasm | tr -d '\n\r')" > playground/probbit-wasm.js
ls -l playground/probbit.wasm playground/probbit-wasm.js
