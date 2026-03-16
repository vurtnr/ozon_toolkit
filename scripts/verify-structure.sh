#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_root="$(cd "$script_dir/.." && pwd)"

required_dirs=("src" "src-tauri" "src-sidecar")
missing=0

for dir in "${required_dirs[@]}"; do
  if [[ ! -d "$project_root/$dir" ]]; then
    echo "missing required directory: $dir"
    missing=1
  fi
done

if [[ $missing -ne 0 ]]; then
  exit 1
fi

echo "structure check passed"
