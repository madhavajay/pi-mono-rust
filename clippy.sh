#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Enforce formatting
cargo fmt --manifest-path "$ROOT/rust/Cargo.toml"

# Lint everything (lib, bins, tests, benches, examples), treat warnings as errors
cargo clippy --fix --allow-dirty --all-targets --all-features --no-deps --manifest-path "$ROOT/rust/Cargo.toml" -- -D warnings
