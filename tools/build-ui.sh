#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
POCKETJS_MINIFY=1 bun .deps/pocketjs/tools/build.ts ui/app.tsx --framework=solid --strict-classes --outdir=ui/generated
