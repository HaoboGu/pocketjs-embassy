#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo fmt --check
cargo fmt --manifest-path firmware/esp32s31/Cargo.toml --check
cargo test --release --locked
cargo clippy --release --locked --all-targets -- -D warnings
cargo run --release --locked --example embassy
cargo run --release --locked --example preview
python3 tools/preview.py
(
    cd firmware/esp32s31
    cargo build --release --locked
    cargo clippy --release --locked -- -D warnings
)
