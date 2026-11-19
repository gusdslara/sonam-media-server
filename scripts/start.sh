#!/usr/bin/env sh
set -eu
cd "$(dirname "$0")/.."
if [ ! -d node_modules ]; then npm ci; fi
npm run build
exec cargo run --release --locked --manifest-path server/Cargo.toml
