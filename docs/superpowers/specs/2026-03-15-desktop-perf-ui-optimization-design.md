# Desktop Performance And UI Optimization Design

## Context

The current desktop app main flow is functionally correct:

- Launch local Chrome with the managed sidecar.
- Use generated search images to perform 1688 image search.
- Run VLM-based screening and strict final review.
- Export `result.xlsx` and optional diagnostics beside the original Excel.

The remaining issues are:

1. Per-row feedback arrives too late in the realtime monitor.
2. Per-row total handling time is higher than necessary even when 1688 search is already finished.
3. The desktop UI is functional but visually weak and does not communicate runtime state well.

The user explicitly requires:

- Keep 1688 access single-threaded.
- Do not add multi-tab or multi-browser concurrency.
- Prefer lower risk against 1688 anti-bot controls over raw speed.
- Redesign the desktop app toward a dark, technology-forward control console.

## Goals

### Performance goals

- Reduce perceived latency by showing row progress before the full row pipeline completes.
- Reduce actual per-row wall time without increasing 1688-side concurrency.
- Preserve the current matching quality and recovery behavior.

### UI goals

- Replace the current stacked white-card layout with a dark console-style experience.
- Make environment readiness, task status, and current row stage visible at a glance.
- Improve readability of monitor rows, failures, and progress.

## Non-goals

- No multi-tab 1688 search.
- No multi-browser concurrency.
- No change to the product shape or output format.
- No removal of diagnostics support.
- No migration away from the current Tauri architecture.

## Current Bottlenecks

The current row pipeline is strictly serialized:

1. Build search-image plan via VLM.
2. Render primary and fallback search images.
3. Perform primary 1688 search.
4. Perform fallback 1688 search if needed.
5. Run up to three screening VLM calls.
6. Run up to eight strict final-review VLM calls.
7. Persist diagnostics when enabled.
8. Emit a single `row_result` event after everything above is complete.

This creates two user-visible issues:

- Chrome may already be back at the 1688 landing state while the app still has not updated the monitor table.
- The row can spend substantial time in local post-search processing even though the network search phase is done.

The highest-value optimization opportunities are therefore inside local orchestration, event timing, and VLM request count, not inside 1688 browser concurrency.

## Chosen Approach

Use a steady-state optimization strategy:

- Keep 1688 browser work strictly serial.
- Refactor row reporting from final-only to staged updates.
- Reduce unnecessary VLM round trips.
- Cache candidate-image downloads during a task.
- Move diagnostics persistence behind result reporting.
- Redesign the UI around a dark operations-console layout.

This approach is preferred because it improves both actual latency and perceived latency while preserving the anti-bot risk profile.

## Performance Design

### 1. Row lifecycle events

Introduce row lifecycle updates instead of a single final event.

Proposed stages:

- `queued`
- `planning_search_image`
- `searching_1688_primary`
- `searching_1688_fallback`
- `candidates_recalled`
- `screening_candidates`
- `final_review`
- `writing_diagnostics`
- `completed`
- `failed`

The monitor table should create or update a row as stages change. Final result fields such as price and item URL are added only once available.

This change solves the main UX complaint directly: rows become visible early and continue updating instead of appearing only after the full pipeline completes.

### 2. Candidate selection tightening

Keep the current frontier idea but reduce screening payload size further before VLM calls.

Adjustments:

- Preserve dedupe by URL.
- Keep relevance-first ordering.
- Reduce the candidate budget used for screening from the current upper bound of 27 when the relevance frontier is already strong.
- Introduce an adaptive cap based on:
  - top relevance concentration
  - price availability
  - duplicate title patterns

Expected effect:

- Fewer candidate-image downloads.
- Fewer grid builds.
- Fewer screening VLM calls on common easy rows.

### 3. Batch final review

The current strict final review is the main local-time amplifier because it can issue up to eight separate one-candidate calls.

Replace this with small-batch strict review:

- Review in batches of 2 to 4 candidates.
- Keep strict prompt semantics for final review.
- Preserve a deterministic cheapest-match selection after confirmation.

Benefits:

- Cuts VLM round trips significantly.
- Keeps the strict same-product decision boundary.
- Avoids touching 1688-side behavior.

### 4. Candidate-image cache

Add a task-scoped candidate image cache keyed by candidate image URL.

Use the cache to avoid repeated:

- HTTP download of the same 1688 candidate image.
- image decode
- tile rendering work

The cache only needs to live for the current task. It should be bounded and discarded at task end.

### 5. Diagnostics after result emission

Today diagnostics can still occupy the tail of a row after the result is already logically known.

Change the order:

1. Emit updated row state with the final outcome.
2. Update progress counters.
3. Persist diagnostics in the background path for that completed row.
4. Emit a log line if diagnostics persistence succeeds or fails.

Diagnostics remain available, but they stop blocking the main row-result path.

### 6. Timing instrumentation

Add per-stage timing metrics to support future tuning.

Track at least:

- search-image planning time
- image generation time
- primary 1688 search time
- fallback 1688 search time
- screening time
- final review time
- diagnostics persistence time

These timings should be visible in logs and optionally retained in diagnostics manifests.

## UI Design

### Visual direction

Adopt a dark technology-console visual system:

- Background: deep graphite with layered gradients and subtle grid texture.
- Accent colors: cyan-blue and electric green.
- Surfaces: translucent dark panels with restrained borders and glow.
- Typography: stronger contrast and more deliberate hierarchy than the current default system-card style.

Avoid:

- flat white cards
- generic SaaS admin styling
- purple-heavy accents

### Layout

Use a three-zone dashboard:

1. **Top status bar**
   - app title
   - environment readiness
   - Chrome readiness
   - key availability
   - overall task progress

2. **Middle control zone**
   - left: file selection and task launch
   - right: active row stage, current SKU, recent logs, blocking alerts

3. **Bottom operations table**
   - realtime row monitor
   - status badges
   - price emphasis
   - link action affordances

### Component behavior

- `SettingsView` becomes an environment/config panel, not an isolated plain form.
- `TaskRunnerView` becomes a command deck for file selection, upload status, and task initiation.
- `MonitorView` becomes an operations board with row-state updates and a stronger stage model.
- Blocking alerts remain prominent but integrate into the overall dashboard rather than feeling bolted on.

### States

The UI must explicitly handle:

- empty
- loading
- uploading
- running
- blocking verification/login
- completed
- failed

The current UI has partial support for these states but does not present them as a coherent system.

## Data And Event Model Changes

### Frontend event state

Replace append-only row behavior with row upsert behavior keyed by `row_index`.

Each row record should carry:

- static identifiers: row index, SKU
- current stage
- final status text
- optional price
- optional item URL
- optional image URL
- stage timestamps or elapsed summary

### Backend event emission

Add row-progress events or extend row-result events so the frontend can update rows incrementally.

Recommended shape:

- row created event
- row stage update event
- row final result event

If one unified event type is preferred, it must support partial updates cleanly.

## Risks And Guardrails

### Risk: candidate tightening hurts recall

Mitigation:

- keep conservative fallback thresholds
- compare before/after results on a fixed sample set
- keep diagnostics for failed rows

### Risk: batched final review changes strictness

Mitigation:

- keep the final-review prompt strict
- add regression tests for same-product and near-product edge cases

### Risk: staged updates create inconsistent UI state

Mitigation:

- define explicit stage transitions
- use row upsert rather than append-only logic
- cover transitions in view-level tests

## Verification Plan

### Automated verification

- Rust unit and integration tests for:
  - candidate tightening
  - batched final review
  - staged row event emission
  - diagnostics-after-result ordering
  - cache hit behavior

- Frontend tests for:
  - row upsert behavior
  - stage badge rendering
  - progress dashboard state changes
  - dark-console layout helpers where logic exists

### Manual verification

Use real sample workbooks and compare:

- time to first visible row stage
- time to final row result
- 1688 anti-bot behavior stability
- result quality against the current stable baseline

## Implementation Order

1. Refactor row event model and frontend row-upsert behavior.
2. Add per-stage instrumentation.
3. Implement candidate-image cache.
4. Tighten screening candidate budget adaptively.
5. Replace single-candidate final review with small-batch strict review.
6. Move diagnostics persistence after result emission.
7. Redesign the shell and three core views into the dark console layout.
8. Run regression and manual timing verification.

## Expected Outcome

After implementation:

- Rows appear in the monitor much earlier.
- Single-row latency decreases without increasing 1688 concurrency.
- The UI feels like a dedicated automation console rather than a basic internal tool.
- The app remains aligned with the current anti-bot and single-task operational constraints.
