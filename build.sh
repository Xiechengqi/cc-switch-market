#!/usr/bin/env bash
set -euo pipefail

(cd web && pnpm install --frozen-lockfile && pnpm build)
cargo build --release
cp -f -v target/release/cc-switch-market .
