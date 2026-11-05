#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
bun tools/build-ui.ts
