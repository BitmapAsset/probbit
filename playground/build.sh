#!/bin/sh
# Build the playground's module (wasm32-unknown-unknown, release): playground/probbit.wasm for a page served over http, and
# playground/probbit-wasm.js, the same bytes in base64, for index.html opened from disk (browsers do not let a file:// page fetch).
# Also playground/puzzle-personas.js, the three example personas and the engine version for puzzle.html.
# Needs the target: rustup target add wasm32-unknown-unknown
set -e
cd "$(dirname "$0")/.."
cargo build --release --locked -p probbit-wasm --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/probbit_wasm.wasm playground/probbit.wasm
printf 'var PROBBIT_WASM_B64 = "%s";\n' "$(base64 < playground/probbit.wasm | tr -d '\n\r')" > playground/probbit-wasm.js
{ printf 'var PUZZLE_PERSONAS = {'; for n in tutor ops-engineer trader-assistant; do printf '"%s": ' "$n"; tr -d '\n\r' < "examples/persona/$n.json"; printf ', '; done
  printf '};\nvar PROBBIT_VERSION = "%s";\n' "$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)"; } > playground/puzzle-personas.js
ls -l playground/probbit.wasm playground/probbit-wasm.js playground/puzzle-personas.js
