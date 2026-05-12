# 1688 Cost Price Hardening Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans or superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** make `1688成本价` retrieval reliable by fixing observability gaps, tightening the auto-pricing boundary, modeling real 1688 spec selection correctly, and adding safer fallback paths only after the rule-based path is trustworthy.

**Architecture:** keep the existing Ozon preflight, 1688 search, and AI candidate ranking flow unchanged until a single best 1688 candidate is chosen. After that, rebuild the detail-pricing stage around four layers: detail diagnostics capture, structured spec-group parsing, cross-language spec normalization, and price-source verification. Visual LLM stays a bounded arbitration fallback rather than the primary price path.

**Tech Stack:** Rust (`src-tauri/src/commands/run_task.rs`, task orchestration, diagnostics, event emission), Bun/TypeScript sidecar (`src-sidecar/src/server.ts`, `src-sidecar/src/1688_engine.ts`, `src-sidecar/src/ozon_session.ts`), existing Bun unit tests and Rust integration-style tests.

---

## Problem Frame

The current 1688 detail-pricing path has the right high-level intent but weak guarantees:

- it treats `#skuSelection` as a flat list instead of a possible multi-group spec model
- it forwards Russian Ozon attributes into Chinese 1688 labels without translation or normalization
- it does not verify that spec selection truly applied before reading price
- it does not verify that clicking quantity `+` changed quantity or triggered price refresh
- it relies primarily on `#submitOrder` text instead of preferring structured page/network data when available
- it collapses multiple failure modes into either `manual_review_required_unknown_spec` or `1688详情页定价失败`, which blocks root-cause analysis

This means the current pipeline can fail at multiple distinct steps while exposing only a shallow terminal status.

## Scope

In scope:

- detail-pricing diagnostics and failure classification
- 1688 detail-page spec parsing and selection model
- Ozon-to-1688 cross-language and unit normalization
- detail price-source verification and fallback order
- stricter manual-review boundaries
- targeted test coverage for the rebuilt chain

Out of scope:

- changing the upstream AI candidate-ranking strategy
- replacing the existing Ozon preflight architecture
- adding a new external translation provider in the first pass
- letting visual LLM read or decide final payable price directly

## Existing Constraints To Preserve

- If `#skuSelection` does not exist, legacy detail-pricing fallback remains allowed.
- If spec resolution or price-source verification is not trustworthy, the row must not fabricate `1688成本价`.
- The system should preserve the chosen 1688 candidate context even when price write-back fails.
- Single-browser, low-risk automation posture remains the default.

## Required Boundary Decisions

These decisions should be treated as implementation requirements, not optional polish.

### Auto-pricing is allowed only when all conditions hold

1. The detail page spec structure is parsed successfully.
2. The intended spec option or option-combination is selected successfully.
3. The selected state is verified from page state rather than inferred from click success alone.
4. The quantity increment action is verified to have changed the quantity.
5. The final price source is verified to have refreshed after selection/quantity changes.
6. Base price and freight are both readable from a trusted source.

### Manual review is required when any condition holds

1. Spec groups cannot be parsed reliably.
2. Multiple candidate options remain tied after normalization and scoring.
3. Cross-language mapping is insufficient to justify a winner.
4. Selection click succeeds but active-state verification fails.
5. Quantity `+` click succeeds but quantity does not change.
6. Page text and structured/network price sources disagree materially.
7. The page exposes only partial pricing signals after a bounded retry window.

### Visual LLM boundary

- VLM is not the primary pricing path.
- VLM may arbitrate between near-tied spec options only after rule-based narrowing.
- VLM output must be structured and confidence-gated.
- VLM may never be the source of final price text or freight text.

## Failure Taxonomy To Introduce

The implementation should stop collapsing all pricing failures into one bucket. Add structured failure reasons along these lines:

- `spec_group_parse_failed`
- `spec_option_ambiguous`
- `spec_selection_not_applied`
- `quantity_increment_not_applied`
- `price_source_not_refreshed`
- `price_source_missing`
- `price_source_conflict`
- `cross_language_mapping_insufficient`
- `manual_review_required_unknown_spec`

Rust should map these into user-facing statuses conservatively while preserving the fine-grained diagnostic code in logs and artifacts.

## Implementation Phases

### Phase 1: Diagnostic Capture And Observability

**Files:**
- Modify: `src-sidecar/src/1688_engine.ts`
- Modify: `src-sidecar/src/server.ts`
- Modify: `src-tauri/src/commands/run_task.rs`
- Test: `src-sidecar/src/1688_engine.test.ts`
- Test: `src-tauri/tests/run_task_command_test.rs`

**Objective:** make each detail-pricing attempt reconstructable from logs/artifacts.

Implementation notes:

- Extend the detail-pricing response contract to include a structured diagnostic payload in both success and failure cases.
- Capture and persist:
  - full-page screenshot after entering the detail page
  - spec-area screenshot
  - `#skuSelection` DOM snapshot
  - selection-before / selection-after state
  - quantity-before / quantity-after state
  - `#submitOrder` before / after text
  - bounded network response summaries when selection or quantity changes trigger data fetches
- Add a sidecar-level helper that serializes parsed spec groups/options instead of returning only flattened rows.
- Ensure Rust logs the structured failure code and writes artifact paths into diagnostics when available.

Exit criteria:

- A single failed row can be diagnosed without re-running the browser session.
- Logs distinguish parsing failure from selection failure from price refresh failure.

### Phase 2: Replace Flat Variant Rows With Spec-Group Modeling

**Files:**
- Modify: `src-sidecar/src/1688_engine.ts`
- Test: `src-sidecar/src/1688_engine.test.ts`

**Objective:** model the real 1688 detail-page selection problem instead of assuming one flat row equals one final SKU.

Implementation notes:

- Replace the current `DetailVariantRow`-centric model with something like:
  - `DetailSpecGroup`
  - `DetailSpecOption`
  - `DetailSelectionState`
- Parse group names, option labels, option images, disabled state, and current selected state.
- Support pages with one group and pages with multiple groups.
- Add a deterministic selection routine that can:
  - choose one option inside a group
  - verify the option becomes active
  - compose multi-group selections incrementally
- Preserve compatibility for pages that are genuinely single-group/simple.

Exit criteria:

- The sidecar can express and log multi-group pages clearly.
- Selection success is based on post-click state, not only on click execution.

### Phase 3: Add Cross-Language And Unit Normalization

**Files:**
- Modify: `src-sidecar/src/ozon_session.ts`
- Modify: `src-sidecar/src/1688_engine.ts`
- Test: `src-sidecar/src/ozon_session.test.ts`
- Test: `src-sidecar/src/1688_engine.test.ts`

**Objective:** make Ozon structured attributes usable against Chinese 1688 labels.

Implementation notes:

- Introduce a small, explicit normalization layer for:
  - common color mappings such as `Белый -> 白色`, `Черный -> 黑色`
  - unit normalization such as `см -> cm`, `мм -> mm`, `шт -> 件`
  - quantity / pack-size normalization
  - normalized numeric token extraction
- Normalize both Ozon attribute values and 1688 option labels into comparable forms before scoring.
- Keep the first pass dictionary-based and local; do not add remote translation dependencies.
- Log whether a match relied on direct image alignment, normalized structured attributes, or title-only fallback.

Exit criteria:

- Russian Ozon spec data can participate meaningfully in 1688 option scoring.
- Ambiguous or unmapped values are surfaced explicitly instead of silently failing weakly.

### Phase 4: Tighten Selection, Quantity, And Refresh Verification

**Files:**
- Modify: `src-sidecar/src/1688_engine.ts`
- Test: `src-sidecar/src/1688_engine.test.ts`

**Objective:** ensure the automation only trusts state changes that actually happened.

Implementation notes:

- After selecting target options, verify active-state markers or equivalent page-state signals.
- Replace the current `+` picker heuristic with a more localized quantity-control search near the purchasable region.
- Record quantity before clicking `+` and wait for quantity-after to differ.
- Wait for a real price refresh signal:
  - changed `#submitOrder` text
  - changed structured page state
  - relevant network response with refreshed price fields
- Bound retries and return a specific failure code if state never changes.

Exit criteria:

- `quantityPlusClicked: true` is no longer treated as enough evidence.
- Price reads happen only after a verified state transition.

### Phase 5: Prefer Structured Price Sources, Keep DOM As Fallback

**Files:**
- Modify: `src-sidecar/src/1688_engine.ts`
- Modify: `src-sidecar/src/server.ts`
- Test: `src-sidecar/src/1688_engine.test.ts`

**Objective:** reduce dependence on brittle text selectors.

Implementation notes:

- Probe for structured price sources in this order:
  1. network response payload triggered by spec/quantity changes
  2. stable page state objects or embedded data
  3. `#submitOrder` text parsing fallback
- Normalize all price reads into a shared payload:
  - `basePrice`
  - `freightPrice`
  - `totalPrice`
  - `priceSource`
  - `priceSourceRefreshed`
- If two sources disagree materially, classify as `price_source_conflict` and require manual review.

Exit criteria:

- A selector drift in `#submitOrder` no longer breaks the whole chain when another trustworthy source exists.

### Phase 6: Rebuild Rust Mapping And User-Facing Status Rules

**Files:**
- Modify: `src-tauri/src/commands/run_task.rs`
- Test: `src-tauri/tests/run_task_command_test.rs`

**Objective:** keep the UI/output conservative while preserving precise internal causes.

Implementation notes:

- Extend the Rust-side detail-pricing payload to consume:
  - structured failure codes
  - `priceSource`
  - refresh-verification booleans
  - artifact references when present
- Map failure codes into tighter user-facing statuses, for example:
  - `无法判断商品规格，需人工介入`
  - `1688详情页规格选择失败`
  - `1688详情页数量未生效`
  - `1688详情页价格未刷新`
  - `1688详情页价格来源冲突`
- Preserve candidate URL and matched variant context even when price is blank.
- Keep the final Excel write conservative: no trustworthy total, no `1688成本价`.

Exit criteria:

- Terminal row statuses explain the actual failure class instead of flattening everything into one generic error.

### Phase 7: Add Bounded VLM Arbitration Fallback

**Files:**
- Modify: `src-sidecar/src/server.ts`
- Modify: `src-sidecar/src/1688_engine.ts`
- Modify: `src-tauri/src/commands/run_task.rs`
- Test: `src-tauri/tests/run_task_command_test.rs`

**Objective:** use VLM only where it adds value without weakening trust boundaries.

Implementation notes:

- Trigger VLM only when:
  - the page has been parsed successfully
  - the rule-based scorer narrowed to a small ambiguous set
  - structured price sources remain available after a final selection
- Feed the VLM bounded inputs:
  - Ozon detail screenshot
  - parsed candidate options
  - spec-area screenshot
- Require structured output:
  - selected group/option indexes
  - confidence
  - `manualReviewRequired`
- If confidence is below threshold, keep manual review required.

Exit criteria:

- VLM arbitration improves edge-case spec resolution without becoming a hidden primary path.

## Test Strategy

### Sidecar unit coverage

Add or expand tests for:

- spec-group parsing on single-group and multi-group layouts
- disabled options and already-selected options
- Russian-to-Chinese normalization for color, size, and quantity
- close-score ambiguity resulting in manual review
- selection verification failure after click
- quantity increment click with unchanged quantity
- price refresh timeout after selection/quantity changes
- conflicting price sources

### Rust integration-style coverage

Add or expand tests for:

- manual review on cross-language mismatch with no reliable mapping
- manual review on parsed-but-ambiguous spec combinations
- failure-specific status mapping for selection-not-applied, quantity-not-applied, and price-not-refreshed
- success path using structured detail-pricing payload with explicit source and refreshed state
- preservation of candidate context when price write-back fails

### Manual verification scenarios

Use a small real-world sample set that includes:

- Ozon attributes in Russian with decisive color and size values
- a 1688 page with multiple spec groups
- a page where row images are missing but labels are sufficient
- a page where `#submitOrder` exists but refresh is delayed
- a page where network payload reveals price earlier than DOM text

## Sequencing

Recommended execution order:

1. Phase 1 diagnostics
2. Phase 2 spec-group modeling
3. Phase 3 normalization
4. Phase 4 state-change verification
5. Phase 5 structured price-source preference
6. Phase 6 Rust mapping
7. Phase 7 bounded VLM fallback

Do not start VLM fallback work before diagnostics, parsing, and verification are in place. Otherwise the system will hide state-model problems behind probabilistic behavior.

## Risks

- 1688 detail templates may vary more than expected, requiring template-specific parsing branches.
- Some pages may expose no trustworthy structured price source, forcing a conservative DOM fallback.
- Local dictionary normalization may not cover all Russian attribute vocabulary on the first pass.
- Additional diagnostics capture can increase artifact volume; retention policy may need tightening later.

## Success Criteria

The plan is successful when:

- failures in the detail-pricing stage are diagnosable from artifacts and structured logs
- Russian Ozon attributes can reliably influence Chinese 1688 spec selection
- auto-pricing only happens after verified selection, verified quantity change, and verified price refresh
- rows that remain ambiguous preserve candidate context but leave `1688成本价` empty
- VLM fallback is optional, confidence-gated, and no longer a hidden dependency for routine pricing success
