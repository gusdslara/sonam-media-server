$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath (Split-Path -Parent $PSScriptRoot)
if (-not (Test-Path -LiteralPath 'node_modules')) { npm ci; if ($LASTEXITCODE) { exit $LASTEXITCODE } }
npm run build
if ($LASTEXITCODE) { exit $LASTEXITCODE }
cargo run --release --locked --manifest-path server/Cargo.toml
