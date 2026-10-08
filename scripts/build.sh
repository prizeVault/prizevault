#!/usr/bin/env bash
# Builds and verifies everything locally, no network beyond crates.io/npm:
#   1. contract tests in the Soroban host        (cargo test)
#   2. release wasm for wasm32v1-none            (stellar contract build)
#   3. contract spec extracted into the app      (app/src/spec.json)
#   4. app offline tests and TypeScript build    (npm test, npm run build)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
WASM="target/wasm32v1-none/release/prizevault.wasm"

echo "== cargo test"
cargo test

echo "== stellar contract build"
stellar contract build
ls -l "$WASM"

echo "== app"
cd app
if [ ! -d node_modules ]; then
  npm install --no-audit --no-fund
fi
node --disable-warning=ExperimentalWarning scripts/extract-spec.ts "../$WASM"
npm test
npm run build

echo "== done"
echo "wasm:  $ROOT/$WASM ($(stat -c %s "$ROOT/$WASM") bytes)"
echo "cli:   $ROOT/app/dist/cli.js  (or: node app/src/cli.ts --help)"
echo "page:  $ROOT/web/index.html"
