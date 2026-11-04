#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"
mkdir -p .deps
if [[ ! -d .deps/pocketjs/.git ]]; then
    git clone --filter=blob:none --no-checkout https://github.com/pocket-nexus/pocketjs .deps/pocketjs
fi
git -C .deps/pocketjs checkout becc0e21b678384ec31efa2b4f8d0be3aeefad5f
if [[ ! -d .deps/quickjs/.git ]]; then
    git clone --depth 1 --branch v0.14.0 https://github.com/quickjs-ng/quickjs .deps/quickjs
fi
test "$(git -C .deps/quickjs rev-parse HEAD)" = 3c051980ab7e783dfbfb1c70c014ce5e05ecf24c
if git -C .deps/quickjs apply --reverse --check "$repo_root/patches/quickjs-port.patch" 2>/dev/null; then
    :
else
    git -C .deps/quickjs apply "$repo_root/patches/quickjs-port.patch"
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
