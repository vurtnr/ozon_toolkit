# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

A Tauri 2 desktop app that automates Ozon-to-1688 product matching. Users upload an Excel file containing Ozon SKUs; the app resolves each SKU's product image via an Ozon browser session, searches 1688 by image via a Puppeteer sidecar, then uses a staged VLM (DashScope) review pipeline to find the best matching supplier listing. Results are written back to a local Excel file.

## Development

```bash
bun install
bun run tauri dev        # starts Vite dev server + Rust backend + hot reload
```

## Build

```bash
bun run build            # vue-tsc --noEmit && vite build (frontend only)
bun run tauri build      # full production bundle (frontend + Rust + sidecar binary)
```

## Tests

```bash
# Frontend (bun test runner)
bun test src/views/__tests__/SettingsView.test.ts
bun test src/views/__tests__/TaskRunnerView.test.ts
bun test src/views/__tests__/MonitorView.test.ts
bun test src/components/__tests__/BlockingAlert.test.ts

# Sidecar (bun test runner)
cd src-sidecar && bun test

# Rust (cargo test)
cd src-tauri && cargo test --test run_task_command_test
cd src-tauri && cargo test --test temp_cleanup_test
cd src-tauri && cargo test --test recovery_flow_test
cd src-tauri && cargo test                            # all Rust tests
```

## Sidecar Build

```bash
cd src-sidecar
bun run build:sidecar --dry-run
bash scripts/build-all-targets.sh
```

`src-tauri/binaries/engine-*` sidecar binaries are built in CI and do not need to be committed.

## E2E Smoke

```bash
# Requires DASHSCOPE_API_KEY env var and sidecar binaries
bash scripts/e2e-smoke.sh /absolute/path/to/input.xlsx
```

## Architecture

Three process layers communicate at runtime:

```
Vue 3 UI (src/)  ──tauri invoke──▶  Rust core (src-tauri/)  ──HTTP──▶  Bun sidecar (src-sidecar/)
                 ◀──tauri events──                          ◀──JSON──
```

### Vue 3 Frontend (`src/`)

Single-page app, no router. `App.vue` composes three view components:

- **SettingsView** — DashScope API key + Chrome executable path (persisted via `tauri-plugin-store`)
- **TaskRunnerView** — Excel file upload, drag-and-drop, task start/resume
- **MonitorView** — real-time per-row progress table, logs, blocking alerts

State is managed via Vue 3 Composition API composables (no Pinia/Vuex):

- `composables/useTaskRunner.ts` — file upload, task lifecycle, `invoke("run_task")` / `invoke("upload_excel_file")`
- `composables/useTaskEvents.ts` — listens to Tauri events (`progress`, `row_result`, `log`, `task_done`, `blocking_alert`, `task_phase`) and updates reactive `MonitorState`
- `stores/settings.ts` — settings load/save via `invoke("load_settings")` / `invoke("save_settings")`, Chrome path normalization
- `types/events.ts` — shared TypeScript event payload interfaces (must stay in sync with Rust `events.rs` structs)

### Rust Backend (`src-tauri/src/`)

Tauri 2 app with these modules:

- **`commands/`** — Tauri command handlers exposed to the frontend:
  - `run_task` — main orchestration entry point; spawns/manages the sidecar process, iterates Excel rows, emits progress events
  - `upload` — copies Excel to a temp working directory
  - `settings` — load/save settings via `tauri-plugin-store`
- **`core/`** — business logic:
  - `orchestrator` — two-pass VLM match pipeline (screening → final review) with fallback images
  - `vlm` — DashScope VLM API client (batch requests with base64 images)
  - `matcher` — candidate selection and scoring
  - `search_image` — VLM-driven image crop planning for search
  - `excel` — Excel read/write (calamine for read, rust_xlsxwriter for write)
  - `ozon_cache` — SKU→image URL cache keyed by SKU
  - `ozon_product` — Ozon product URL/SKU resolution
- **`events.rs`** — event names, payload structs, and `EventSink` trait (enables testable event emission without Tauri window dependency)
- **`recovery.rs`** — blocking alert codes and a global recovery gate for sidecar login/captcha challenges
- **`lifecycle/cleanup.rs`** — temp directory cleanup and task guard

### Bun Sidecar (`src-sidecar/src/`)

Express HTTP server (port 8266) bundled as a single-file Bun binary. Manages two separate Chrome instances:

- **1688 browser** — Puppeteer-controlled Chrome with anti-detection evasions for image search on 1688.com
- **Ozon browser** — separate Chrome profile for Ozon product page scraping

Endpoints: `/search`, `/resolve-ozon-product`, `/resolve-ozon-sku`, `/close-ozon-session`, `/session-state`, `/health`, `/shutdown`

Key files:
- `server.ts` — HTTP routes, browser lifecycle, session state classification (ready/login_required/anti_bot_challenge)
- `1688_engine.ts` — image upload and search result scraping on 1688.com
- `ozon_session.ts` — Ozon product page navigation and data extraction
- `chrome-path.ts` — cross-platform Chrome executable discovery

## Key Conventions

- **Event-driven progress**: The Rust backend emits Tauri events (`row_result`, `progress`, `task_phase`, `blocking_alert`) per row rather than batching, so the UI updates in real-time. Payload shapes in `src/types/events.ts` must match `src-tauri/src/events.rs`.
- **Wire format naming**: Rust↔JS boundary uses snake_case (Rust native). The frontend converts to camelCase only in its own types (`MonitorRow` vs `RowResultEventPayload`).
- **Sidecar communication**: Rust spawns and manages the sidecar process lifecycle. All sidecar URLs are configurable via env vars (`SIDECAR_SEARCH_URL`, `SIDECAR_HEALTH_URL`, etc.) to support testing with mock servers.
- **Rust test mocking**: Integration tests use env vars (`RUN_TASK_MOCK_CANDIDATES_JSON`, `RUN_TASK_MOCK_VLM_REPLIES_JSON`, etc.) to inject mock data instead of hitting real services.
- **EventSink trait**: Rust tests use a mock `EventSink` implementation instead of a real Tauri window, keeping tests independent of the Tauri runtime.
- **UI is Chinese**: user-facing strings are in Chinese (Simplified).

## CI/Release

- `.github/workflows/release.yml` — production release workflow
- `.github/workflows/windows-unsigned.yml` — internal Windows test builds (triggers on `main` push)
- Release secrets documented in `docs/release-secrets.md`
