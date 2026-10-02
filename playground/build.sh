#!/bin/sh
# Build the playground's module (wasm32-unknown-unknown, release): playground/pbit.wasm for a page served over http, and
# playground/pbit-wasm.js, the same bytes in base64, for index.html opened from disk (browsers do not let a file:// page fetch).
# Needs the target: rustup target add wasm32-unknown-unknown
set -e
cd "$(dirname "$0")/.."
cargo build --release --locked -p pbit-wasm --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/pbit_wasm.wasm playground/pbit.wasm
printf 'var PBIT_WASM_B64 = "%s";\n' "$(base64 < playground/pbit.wasm | tr -d '\n\r')" > playground/pbit-wasm.js
ls -l playground/pbit.wasm playground/pbit-wasm.js
