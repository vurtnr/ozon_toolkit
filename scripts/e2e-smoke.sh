#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
INPUT_XLSX="${1:-}"

missing=()

if [[ -z "${DASHSCOPE_API_KEY:-}" ]]; then
  missing+=("env:DASHSCOPE_API_KEY")
fi

if [[ -z "$INPUT_XLSX" ]]; then
  missing+=("arg:input_xlsx_absolute_path")
elif [[ ! "$INPUT_XLSX" = /* && ! "$INPUT_XLSX" =~ ^[A-Za-z]:\\ ]]; then
  missing+=("input_xlsx_must_be_absolute:$INPUT_XLSX")
elif [[ ! -f "$INPUT_XLSX" ]]; then
  missing+=("input_xlsx_not_found:$INPUT_XLSX")
fi

if [[ ! -f "$ROOT_DIR/src-tauri/binaries/engine-x86_64-pc-windows-msvc.exe" ]]; then
  missing+=("binary:src-tauri/binaries/engine-x86_64-pc-windows-msvc.exe")
fi
if [[ ! -f "$ROOT_DIR/src-tauri/binaries/engine-x86_64-apple-darwin" ]]; then
  missing+=("binary:src-tauri/binaries/engine-x86_64-apple-darwin")
fi
if [[ ! -f "$ROOT_DIR/src-tauri/binaries/engine-aarch64-apple-darwin" ]]; then
  missing+=("binary:src-tauri/binaries/engine-aarch64-apple-darwin")
fi

if (( ${#missing[@]} > 0 )); then
  echo "E2E smoke precheck failed. Missing prerequisites:" >&2
  for item in "${missing[@]}"; do
    echo "- $item" >&2
  done
  exit 1
fi

cd "$ROOT_DIR"

bun test src/views/__tests__/SettingsView.test.ts \
  src/views/__tests__/TaskRunnerView.test.ts \
  src/views/__tests__/MonitorView.test.ts \
  src/components/__tests__/BlockingAlert.test.ts

(
  cd src-tauri
  cargo test --test run_task_command_test
  cargo test --test recovery_flow_test
)

echo "E2E smoke precheck passed. Ready for manual desktop validation."
