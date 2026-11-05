#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
(
    cd .deps/pocketjs
    bun test --conditions=browser tests/compiler-portability.test.ts tests/renderer.test.ts
    bunx --no-install tsc --noEmit
)
cargo fmt --check
cargo fmt --manifest-path firmware/esp32s31/Cargo.toml --check
cargo test --release --locked
cargo clippy --release --locked --all-targets -- -D warnings
cargo run --release --locked --example embassy
cargo run --release --locked --example preview
python3 tools/preview.py
(
    cd firmware/esp32s31
    for protocol in rynk vial; do
        cargo build --release --locked --no-default-features --features "$protocol"
        cargo clippy --release --locked --no-default-features --features "$protocol" -- -D warnings
        artifact="../../target/artifacts/$protocol"
        mkdir -p "$artifact"
        cp target/riscv32imafc-unknown-none-elf/release/rmk-mosaico-quickjs "$artifact/rmk-mosaico-quickjs.elf"
        python3 ../../tools/memory.py "$artifact/rmk-mosaico-quickjs.elf" > "$artifact/memory.json"
    done
)
