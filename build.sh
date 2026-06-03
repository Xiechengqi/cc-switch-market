#!/usr/bin/env bash
set -euo pipefail

rm -f -v cc-switch-market

(cd web && pnpm install --frozen-lockfile && pnpm build)
cargo build --release

cp -f -v target/release/cc-switch-market .
