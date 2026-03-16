# Release Workflow Dry Run Checklist

## Required Artifacts

- [ ] Windows sidecar binary: `src-tauri/binaries/engine-x86_64-pc-windows-msvc.exe`
- [ ] macOS x64 sidecar binary: `src-tauri/binaries/engine-x86_64-apple-darwin`
- [ ] macOS arm64 sidecar binary: `src-tauri/binaries/engine-aarch64-apple-darwin`
- [ ] Windows installer artifact (`.msi` or `.exe`)
- [ ] macOS installer artifact (`.app` or `.dmg`)

## Required Validation

- [ ] Workflow lints with `actionlint`
- [ ] Rust checks pass on both matrix runners
- [ ] Frontend build (`bun run build`) passes on both matrix runners
- [ ] Sidecar build script exits 0 on both matrix runners
