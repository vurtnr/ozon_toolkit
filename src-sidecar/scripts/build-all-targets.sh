#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT_DIR/src-sidecar"

if [[ "${RUNNER_OS:-}" == "Windows" ]]; then
  bun run scripts/build-sidecar.ts --target bun-windows-x64
  exit 0
fi

if [[ "${RUNNER_OS:-}" == "macOS" ]]; then
  bun run scripts/build-sidecar.ts --target bun-darwin-x64
  bun run scripts/build-sidecar.ts --target bun-darwin-arm64
  exit 0
fi

echo "Unsupported RUNNER_OS=${RUNNER_OS:-unknown}"
exit 1
