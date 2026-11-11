#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"
pocketjs_url="https://github.com/HaoboGu/pocketjs"
pocketjs_rev="acddb19a9544e57c3d6995602b3433d105a9929d"
mkdir -p .deps
if [[ ! -d .deps/pocketjs/.git ]]; then
    git clone --no-checkout "$pocketjs_url" .deps/pocketjs
fi
git -C .deps/pocketjs fetch "$pocketjs_url" "$pocketjs_rev"
git -C .deps/pocketjs checkout --detach "$pocketjs_rev"
for patch in pocketjs-style-lookup.patch pocketjs-display-runtime.patch pocketjs-native-execution.patch; do
    if ! git -C .deps/pocketjs apply --reverse --check "$repo_root/patches/$patch" 2>/dev/null; then
        git -C .deps/pocketjs apply "$repo_root/patches/$patch"
    fi
done
if [[ ! -d .deps/rmk/.git ]]; then
    git clone --filter=blob:none --no-checkout https://github.com/rmk-rs/rmk .deps/rmk
fi
# RMK's storage identifier uses path history, which a shallow clone changes.
if [[ "$(git -C .deps/rmk rev-parse --is-shallow-repository)" == "true" ]]; then
    git -C .deps/rmk fetch --unshallow https://github.com/rmk-rs/rmk c7195de6fe994da3651fdc20ee358ca4e69de11d
    touch .deps/rmk/rmk/build.rs
fi
git -C .deps/rmk fetch https://github.com/rmk-rs/rmk c7195de6fe994da3651fdc20ee358ca4e69de11d
git -C .deps/rmk checkout --detach c7195de6fe994da3651fdc20ee358ca4e69de11d
if ! git -C .deps/rmk apply --reverse --check "$repo_root/patches/rmk-dongle-state.patch" 2>/dev/null; then
    git -C .deps/rmk apply "$repo_root/patches/rmk-dongle-state.patch"
fi
bun install --cwd .deps/pocketjs --frozen-lockfile --ignore-scripts
if [[ "${1:-}" == "--s31" ]]; then
    rustup target add riscv32imafc-unknown-none-elf
    if [[ ! -x .deps/riscv32-esp-elf/bin/riscv32-esp-elf-gcc ]]; then
        case "$(uname -s)-$(uname -m)" in
            Darwin-arm64) platform=aarch64-apple-darwin ;;
            Linux-x86_64) platform=x86_64-linux-gnu ;;
            *) printf 'Install the Espressif RISC-V toolchain at .deps/riscv32-esp-elf for this host.\n' >&2; exit 1 ;;
        esac
        archive="riscv32-esp-elf-16.1.0_20260609-${platform}.tar.xz"
        curl --fail --location "https://github.com/espressif/crosstool-NG/releases/download/esp-16.1.0_20260609/${archive}" --output ".deps/${archive}"
        tar -xf ".deps/${archive}" -C .deps
    fi
fi
