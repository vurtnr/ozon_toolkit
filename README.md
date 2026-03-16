# desktop_app

Production-oriented Tauri desktop application that consolidates:

- Rust processing core (`src-tauri/`)
- Bun sidecar crawler (`src-sidecar/`)
- Vue 3 UI (`src/`)

## Development

```bash
bun install
bun run tauri dev
```

## Build

```bash
bun run build
```

## Tests

```bash
# frontend helper tests
bun test src/views/__tests__/SettingsView.test.ts \
  src/views/__tests__/TaskRunnerView.test.ts \
  src/views/__tests__/MonitorView.test.ts \
  src/components/__tests__/BlockingAlert.test.ts

# rust tests
cd src-tauri
cargo test --test run_task_command_test
cargo test --test temp_cleanup_test
cargo test --test recovery_flow_test
```

## Sidecar Packaging

```bash
cd src-sidecar
bun run build:sidecar --dry-run
bash scripts/build-all-targets.sh
```

## Release Workflow Lint

```bash
actionlint .github/workflows/release.yml
```

Release secrets reference:
- `docs/release-secrets.md`

## Windows Unsigned Test Build

Use the `Windows Unsigned Build` GitHub Actions workflow when you need an internal Windows installer without code signing.

- The workflow runs automatically on `main` pushes and can also be triggered manually from GitHub Actions.
- Download the `desktop-app-windows-unsigned-<sha>` artifact after the run finishes.
- The artifact contains the Windows bundle output under `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/`.
- SmartScreen and `Unknown publisher` warnings are expected because the installer is unsigned.
- `src-tauri/binaries/engine-*` is built in CI and does not need to be committed.

## E2E Smoke

```bash
# Requires DASHSCOPE_API_KEY and sidecar binaries to exist
bash scripts/e2e-smoke.sh /absolute/path/to/input.xlsx
```

Detailed acceptance checklist:
- `docs/e2e-acceptance-checklist.md`
