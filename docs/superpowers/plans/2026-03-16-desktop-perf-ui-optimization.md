# Desktop Perf And UI Optimization Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce per-row perceived and actual latency under single-threaded 1688 constraints, while redesigning the Tauri UI into a dark control console.

**Architecture:** Keep the browser-side 1688 flow strictly serial, but refactor row reporting into staged updates, cut VLM round trips with batch final review, cache candidate images during a task, and redesign the frontend around an operations-dashboard shell. Backend changes stay in the existing Rust/Tauri orchestration path and frontend changes stay within the existing Vue Composition API app.

**Tech Stack:** Rust, Tauri 2, Vue 3 with `<script setup>`, Bun tests, cargo tests

---

## Chunk 1: Row Stage Events And Monitor Upserts

### Task 1: Define row stage payloads end-to-end

**Files:**
- Modify: `src-tauri/src/events.rs`
- Modify: `src/types/events.ts`
- Test: `src/views/__tests__/MonitorView.test.ts`

- [ ] Step 1: Write failing tests for row upsert behavior and stage metadata.
- [ ] Step 2: Run `bun test src/views/__tests__/MonitorView.test.ts` and verify failure.
- [ ] Step 3: Add staged row event payload types in Rust and TypeScript.
- [ ] Step 4: Update monitor helpers to upsert by `row_index` instead of append-only.
- [ ] Step 5: Re-run `bun test src/views/__tests__/MonitorView.test.ts`.

### Task 2: Emit row lifecycle updates from the task runner

**Files:**
- Modify: `src-tauri/src/commands/run_task.rs`
- Test: `src-tauri/tests/run_task_command_test.rs`

- [ ] Step 1: Write failing Rust tests asserting a row is emitted before final completion and later updated.
- [ ] Step 2: Run targeted cargo test and verify failure.
- [ ] Step 3: Emit staged row updates through the task loop for planning/searching/screening/final-review/completed/failed.
- [ ] Step 4: Re-run targeted cargo test.

## Chunk 2: Low-Risk Throughput Improvements

### Task 3: Batch strict final review

**Files:**
- Modify: `src-tauri/src/core/orchestrator.rs`
- Modify: `src-tauri/src/core/vlm.rs`
- Test: `src-tauri/tests/orchestrator_parity_test.rs`
- Test: `src-tauri/src/core/vlm.rs`

- [ ] Step 1: Write failing tests proving final review uses batched strict prompts and fewer calls.
- [ ] Step 2: Run targeted cargo tests and verify failure.
- [ ] Step 3: Implement multi-candidate strict final review batches.
- [ ] Step 4: Re-run targeted cargo tests.

### Task 4: Add candidate image cache

**Files:**
- Modify: `src-tauri/src/core/vlm.rs`
- Test: `src-tauri/src/core/vlm.rs`

- [ ] Step 1: Write a failing test for repeated candidate image URLs reusing cached tiles.
- [ ] Step 2: Run targeted cargo test and verify failure.
- [ ] Step 3: Add task-scoped candidate image cache inside the runtime VLM client path.
- [ ] Step 4: Re-run targeted cargo test.

### Task 5: Move diagnostics behind user-visible result emission

**Files:**
- Modify: `src-tauri/src/commands/run_task.rs`
- Test: `src-tauri/tests/run_task_command_test.rs`

- [ ] Step 1: Write failing test asserting row result is emitted before diagnostics completion logs.
- [ ] Step 2: Run targeted cargo test and verify failure.
- [ ] Step 3: Reorder row result emission ahead of diagnostics persistence.
- [ ] Step 4: Re-run targeted cargo test.

## Chunk 3: Dark Console UI

### Task 6: Replace the app shell and introduce shared dashboard tokens

**Files:**
- Modify: `src/App.vue`
- Modify: `src/main.ts`
- Create or Modify: `src/app.css` or equivalent shared stylesheet

- [ ] Step 1: Introduce shared design tokens and dashboard shell structure.
- [ ] Step 2: Verify app builds with `bun test` and `bun run build` if needed.

### Task 7: Redesign settings and task runner surfaces

**Files:**
- Modify: `src/views/SettingsView.vue`
- Modify: `src/views/TaskRunnerView.vue`
- Test: `src/views/__tests__/TaskRunnerView.test.ts`

- [ ] Step 1: Preserve existing logic but move both panels into the new dark console style.
- [ ] Step 2: Keep Composition API and current upload behavior intact.
- [ ] Step 3: Re-run task-runner tests.

### Task 8: Redesign monitor as an operations table

**Files:**
- Modify: `src/views/MonitorView.vue`
- Modify: `src/composables/useTaskEvents.ts`
- Test: `src/views/__tests__/MonitorView.test.ts`

- [ ] Step 1: Add stage badges, current status emphasis, and richer row state rendering.
- [ ] Step 2: Show current progress and recent logs in the dashboard zone.
- [ ] Step 3: Re-run monitor tests.

## Chunk 4: Full Verification

### Task 9: Run regression suite

**Files:**
- No code changes expected

- [ ] Step 1: Run `cargo test` in `src-tauri`.
- [ ] Step 2: Run `bun test` in repo root.
- [ ] Step 3: If needed, run `bun run tauri dev` manually for visual validation.

### Task 10: Commit implementation in logical slices

**Files:**
- All changed files from prior tasks

- [ ] Step 1: Commit backend event/performance changes.
- [ ] Step 2: Commit UI redesign changes.
- [ ] Step 3: Summarize verification evidence.
