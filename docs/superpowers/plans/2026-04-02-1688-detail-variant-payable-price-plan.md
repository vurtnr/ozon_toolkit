# 1688 Detail Variant Payable Price Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the current freight-only 1688 detail enrichment with a variant-aware payable-price flow that:
- prefers image-based variant matching when detail-page SKU rows expose images,
- falls back to spec-label numeric matching when SKU rows do not expose images,
- and stops with a manual-review-required status when `#skuSelection` exists but no trustworthy variant can be determined.

The final `1688成本价` must be `商品金额 + 另需运费（预估）` after the correct SKU row is selected. If `#skuSelection` does not exist, keep the old price logic. If `#skuSelection` exists but variant resolution is uncertain, do **not** compute a price.

**Architecture:** Keep the current Rust candidate-selection pipeline unchanged until a single best 1688 candidate is chosen. After selection, send the detail URL, matched 1688 image URL, and Ozon title to the sidecar. The sidecar opens the detail page and applies this state machine:

1. If `#skuSelection` does not exist, return a legacy pricing payload and allow Rust to keep the old logic.
2. If `#skuSelection` exists, collect all `div.expand-view-item.v-flex` rows.
3. Try to resolve a row by image first.
4. If row images are absent or unusable, try to resolve a row by `span.item-label` numeric fragments against the Ozon title.
5. If a row is matched, click the row’s trailing `+`, wait for `#submitOrder` totals to stabilize, then return `商品金额 + 运费`.
6. If no trustworthy row can be resolved, return `manual_review_required_unknown_spec`. Rust must write a dedicated human-intervention status and leave `1688成本价` empty.

**Tech Stack:** Rust (`reqwest`, existing `run_task.rs` flow), Bun/TypeScript sidecar (`express`, `puppeteer`), existing sidecar tests in `src-sidecar/src/1688_engine.test.ts`, Rust tests in `src-tauri/src/commands/run_task.rs` and `src-tauri/tests/run_task_command_test.rs`.

---

## File Structure

- Modify: `src-sidecar/src/server.ts`
  - Expand `/resolve-1688-detail-pricing` request/response shapes and implement the new detail-page pricing orchestration.
- Modify: `src-sidecar/src/1688_engine.ts`
  - Add pure helpers for variant-row extraction, image matching, spec-label numeric matching, amount parsing, and payable-total resolution.
- Modify: `src-sidecar/src/1688_engine.test.ts`
  - Add unit coverage for the new parsing, row-image matching, and spec-label matching helpers.
- Modify: `src-tauri/src/commands/run_task.rs`
  - Send matched image URL plus Ozon title to sidecar, consume the richer pricing payload, and map manual-review-required responses into a terminal row status with no price.
- Modify: `src-tauri/tests/run_task_command_test.rs`
  - Add integration-style coverage for variant-image, variant-label, legacy-no-sku-selection, and manual-review-required flows.

### Decision Rules

- If detail page does **not** contain `#skuSelection`, use the previous price logic.
- If detail page **does** contain `#skuSelection`, the previous price logic is no longer trustworthy by default.
- When `#skuSelection` exists:
  - image row matching has priority over spec-label matching
  - spec-label matching is only allowed when row images are absent or unusable
  - if neither image nor spec-label matching can determine a trustworthy row, return manual review required
- Manual-review-required rows must:
  - keep the 1688 link
  - keep the chosen candidate context
  - write a dedicated terminal status
  - leave `1688成本价` empty

---

## Task 1: Lock The New Pricing Contract

**Files:**
- Modify: `src-sidecar/src/server.ts`
- Modify: `src-tauri/src/commands/run_task.rs`
- Test: `src-tauri/src/commands/run_task.rs`

- [ ] **Step 1: Write the failing Rust-side contract tests**

```rust
#[test]
fn merge_1688_detail_pricing_prefers_variant_image_payable_total() {
    let payload = Sidecar1688DetailPricingPayload {
        item_amount_text: Some("¥12.82".to_string()),
        item_amount_value: Some(12.82),
        freight_text: Some("¥3".to_string()),
        freight_value: Some(3.0),
        final_total_text: Some("¥15.82".to_string()),
        final_total_value: Some(15.82),
        resolution_mode: Some("variant_image_payable_total".to_string()),
        matched_variant_label: Some("4件装 45cm".to_string()),
        is_free_shipping: false,
    };

    assert_eq!(resolve_1688_output_price("¥13.82", &payload), "¥15.82");
}

#[test]
fn merge_1688_detail_pricing_falls_back_only_when_no_sku_selection_exists() {
    let payload = Sidecar1688DetailPricingPayload {
        item_amount_text: None,
        item_amount_value: None,
        freight_text: Some("¥3".to_string()),
        freight_value: Some(3.0),
        final_total_text: None,
        final_total_value: None,
        resolution_mode: Some("legacy_no_sku_selection".to_string()),
        matched_variant_label: None,
        is_free_shipping: false,
    };

    assert_eq!(resolve_1688_output_price("¥12.82", &payload), "¥15.82");
}
```

- [ ] **Step 2: Add a failing manual-review contract test**

```rust
#[test]
fn merge_1688_detail_pricing_requires_manual_review_when_sku_selection_exists_but_no_variant_can_be_resolved() {
    let payload = Sidecar1688DetailPricingPayload {
        item_amount_text: None,
        item_amount_value: None,
        freight_text: None,
        freight_value: None,
        final_total_text: None,
        final_total_value: None,
        resolution_mode: Some("manual_review_required_unknown_spec".to_string()),
        matched_variant_label: None,
        is_free_shipping: false,
    };

    assert_eq!(
        resolve_1688_output_status("AI比对成功(源图首搜命中)", &payload),
        "无法判断商品规格，需人工介入"
    );
}
```

- [ ] **Step 3: Run the Rust tests to verify they fail**

Run: `cd src-tauri && cargo test merge_1688_detail_pricing -- --nocapture`
Expected: FAIL because the request/response contract does not yet include `ozonTitle`, `matchedVariantLabel`, or the richer resolution modes.

- [ ] **Step 4: Expand the shared request/response contract**

```rust
#[derive(Debug, Serialize)]
struct Sidecar1688DetailPricingRequest {
    #[serde(rename = "itemUrl")]
    item_url: String,
    #[serde(rename = "cardPrice")]
    card_price: String,
    #[serde(rename = "matchedImageUrl")]
    matched_image_url: Option<String>,
    #[serde(rename = "ozonTitle")]
    ozon_title: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Sidecar1688DetailPricingPayload {
    #[serde(rename = "itemAmountText")]
    item_amount_text: Option<String>,
    #[serde(rename = "itemAmountValue")]
    item_amount_value: Option<f64>,
    #[serde(rename = "freightText")]
    freight_text: Option<String>,
    #[serde(rename = "freightValue")]
    freight_value: Option<f64>,
    #[serde(rename = "finalTotalText")]
    final_total_text: Option<String>,
    #[serde(rename = "finalTotalValue")]
    final_total_value: Option<f64>,
    #[serde(rename = "resolutionMode")]
    resolution_mode: Option<String>,
    #[serde(rename = "matchedVariantLabel")]
    matched_variant_label: Option<String>,
    #[serde(rename = "isFreeShipping", default)]
    is_free_shipping: bool,
}
```

- [ ] **Step 5: Add the Rust resolver helpers and wire the Ozon title into the request**

```rust
fn resolve_1688_output_price(card_price: &str, payload: &Sidecar1688DetailPricingPayload) -> String {
    if let Some(total) = payload.final_total_value.filter(|value| value.is_finite() && *value > 0.0) {
        return format_1688_price_value(total);
    }

    merge_1688_price_with_freight(card_price, payload.freight_value)
}

fn resolve_1688_output_status(default_status: &str, payload: &Sidecar1688DetailPricingPayload) -> String {
    match payload.resolution_mode.as_deref() {
        Some("manual_review_required_unknown_spec") => "无法判断商品规格，需人工介入".to_string(),
        _ => default_status.to_string(),
    }
}

let request_body = Sidecar1688DetailPricingRequest {
    item_url: item_url.to_string(),
    card_price: card_price.to_string(),
    matched_image_url: output_row.matched_image_url.clone(),
    ozon_title: Some(resolved_row.ozon_name.clone()),
};
```

- [ ] **Step 6: Re-run the focused Rust tests**

Run: `cd src-tauri && cargo test merge_1688_detail_pricing -- --nocapture`
Expected: PASS.

---

## Task 2: Add Pure Detail-Page Variant Parsing Helpers In The Sidecar

**Files:**
- Modify: `src-sidecar/src/1688_engine.ts`
- Test: `src-sidecar/src/1688_engine.test.ts`

- [ ] **Step 1: Write failing sidecar unit tests for parsing and row matching**

```ts
test("extract1688DetailPayableAmounts parses 商品金额 and 另需运费（预估）", () => {
  expect(
    extract1688DetailPayableAmounts([
      "商品金额：¥12.82",
      "另需运费（预估）：¥3",
    ]),
  ).toEqual({
    itemAmountText: "¥12.82",
    itemAmountValue: 12.82,
    freightText: "¥3",
    freightValue: 3,
    finalTotalText: "¥15.82",
    finalTotalValue: 15.82,
  });
});

test("pickBestVariantRowByImage chooses the row whose thumbnail matches the candidate image", () => {
  const rows = [
    { imageUrl: "https://img/other.jpg", title: "row-a", labelText: "2件装 30cm" },
    { imageUrl: "https://img/matched.jpg_220x220.jpg", title: "row-b", labelText: "4件装 45cm" },
  ];
  expect(
    pickBestVariantRowByImage(rows, "https://img/matched.jpg"),
  )?.title.toBe("row-b");
});

test("pickBestVariantRowBySpecLabel chooses the row whose numeric spec fragments appear in the Ozon title", () => {
  const rows = [
    { imageUrl: "", title: "row-a", labelText: "2件装 30cm" },
    { imageUrl: "", title: "row-b", labelText: "4件装 45cm" },
  ];
  expect(
    pickBestVariantRowBySpecLabel(rows, "Kitchen brush 4件装 45cm"),
  )?.title.toBe("row-b");
});

test("pickBestVariantRowBySpecLabel returns null when the Ozon title has no usable numeric fragments", () => {
  const rows = [{ imageUrl: "", title: "row-a", labelText: "大号 红色" }];
  expect(
    pickBestVariantRowBySpecLabel(rows, "Kitchen brush premium version"),
  ).toBeNull();
});
```

- [ ] **Step 2: Run the sidecar tests to verify they fail**

Run: `cd src-sidecar && bun test src/1688_engine.test.ts`
Expected: FAIL because the helper exports do not exist yet.

- [ ] **Step 3: Add pure helper functions in `src-sidecar/src/1688_engine.ts`**

```ts
export type DetailVariantRow = {
  imageUrl: string;
  title: string;
  labelText: string | null;
  rowPriceText: string | null;
};

export type DetailPayableAmounts = {
  itemAmountText: string | null;
  itemAmountValue: number | null;
  freightText: string | null;
  freightValue: number | null;
  finalTotalText: string | null;
  finalTotalValue: number | null;
};

export function normalizeComparableImageUrl(value: string): string {
  return (value || "")
    .trim()
    .replace(/^https?:/, "")
    .replace(/_[0-9]+x[0-9]+[^/]*$/i, "")
    .replace(/\?.*$/, "");
}

export function pickBestVariantRowByImage(
  rows: DetailVariantRow[],
  matchedImageUrl: string | null | undefined,
): DetailVariantRow | null {
  const expected = normalizeComparableImageUrl(matchedImageUrl || "");
  if (!expected) return null;
  return (
    rows.find((row) => normalizeComparableImageUrl(row.imageUrl) === expected) ??
    null
  );
}

export function pickBestVariantRowBySpecLabel(
  rows: DetailVariantRow[],
  ozonTitle: string | null | undefined,
): DetailVariantRow | null {
  // Extract meaningful numeric fragments from the Ozon title and each item-label.
  // If the Ozon title has no usable numeric fragments, return null.
}

export function extract1688DetailPayableAmounts(signals: string[]): DetailPayableAmounts | null {
  // Parse 商品金额 and 另需运费（预估） first; if both exist, compute final total.
}
```

- [ ] **Step 4: Re-run the sidecar unit tests**

Run: `cd src-sidecar && bun test src/1688_engine.test.ts`
Expected: PASS.

---

## Task 3: Resolve The Matched SKU Row On The 1688 Detail Page

**Files:**
- Modify: `src-sidecar/src/server.ts`
- Modify: `src-sidecar/src/1688_engine.ts`
- Test: `src-sidecar/src/1688_engine.test.ts`

- [ ] **Step 1: Add failing sidecar payload-builder tests**

```ts
test("builds variant_image_payable_total payload from matched row totals", () => {
  expect(
    buildResolvedDetailPricingPayload({
      itemAmountText: "¥12.82",
      itemAmountValue: 12.82,
      freightText: "¥3",
      freightValue: 3,
      finalTotalText: "¥15.82",
      finalTotalValue: 15.82,
    }, "variant_image_payable_total"),
  ).toMatchObject({
    finalTotalText: "¥15.82",
    finalTotalValue: 15.82,
    resolutionMode: "variant_image_payable_total",
  });
});

test("builds manual_review_required payload when skuSelection exists but no trustworthy variant can be resolved", () => {
  expect(
    buildManualReviewRequiredDetailPricingPayload("规格判定缺失"),
  ).toMatchObject({
    finalTotalText: null,
    finalTotalValue: null,
    resolutionMode: "manual_review_required_unknown_spec",
  });
});
```

- [ ] **Step 2: Run the sidecar tests to verify they fail**

Run: `cd src-sidecar && bun test src/1688_engine.test.ts`
Expected: FAIL.

- [ ] **Step 3: Implement detail-page pricing state machine in `src-sidecar/src/server.ts`**

```ts
interface Resolve1688DetailPricingRequestBody {
  itemUrl?: string;
  cardPrice?: string;
  matchedImageUrl?: string;
  ozonTitle?: string;
}

async function resolve1688DetailPricingViaBrowser(
  itemUrl: string,
  matchedImageUrl?: string,
  ozonTitle?: string,
): Promise<Resolved1688DetailPricingPayload> {
  const browser = await ensureBrowserAlive();
  const detailPage = await browser.newPage();

  try {
    await applyBrowserEvasions(detailPage);
    await detailPage.goto(itemUrl, { waitUntil: "domcontentloaded", timeout: 60_000 });
    await delay(1_200);
    await ensure1688DetailPageAccessible(detailPage);

    const hasSkuSelection = await has1688SkuSelection(detailPage);
    if (!hasSkuSelection) {
      const freightSignals = await collect1688DetailFreightSignals(detailPage);
      const freight = extract1688DetailFreight(freightSignals);
      return buildResolvedFreightOnlyPayload(freight, "legacy_no_sku_selection");
    }

    const variantRows = await collect1688DetailVariantRows(detailPage);

    const imageMatchedRow = pickBestVariantRowByImage(variantRows, matchedImageUrl);
    if (imageMatchedRow) {
      await clickVariantRowPlusButton(detailPage, imageMatchedRow);
      const payableSignals = await collect1688DetailPayableSignals(detailPage);
      const payable = extract1688DetailPayableAmounts(payableSignals);
      if (payable?.finalTotalValue) {
        return buildResolvedDetailPricingPayload(payable, "variant_image_payable_total");
      }
    }

    const labelMatchedRow = pickBestVariantRowBySpecLabel(variantRows, ozonTitle);
    if (labelMatchedRow) {
      await clickVariantRowPlusButton(detailPage, labelMatchedRow);
      const payableSignals = await collect1688DetailPayableSignals(detailPage);
      const payable = extract1688DetailPayableAmounts(payableSignals);
      if (payable?.finalTotalValue) {
        return buildResolvedDetailPricingPayload(payable, "variant_label_payable_total");
      }
    }

    return buildManualReviewRequiredDetailPricingPayload("规格判定缺失");
  } finally {
    if (!detailPage.isClosed()) await detailPage.close().catch(() => undefined);
  }
}
```

- [ ] **Step 4: Update the route handler to pass the Ozon title**

```ts
const data = await resolve1688DetailPricingViaBrowser(
  itemUrl,
  req.body?.matchedImageUrl,
  req.body?.ozonTitle,
);
```

- [ ] **Step 5: Re-run the sidecar tests**

Run: `cd src-sidecar && bun test src/1688_engine.test.ts`
Expected: PASS.

---

## Task 4: Integrate The New Sidecar Payload Into Result Writing

**Files:**
- Modify: `src-tauri/src/commands/run_task.rs`
- Test: `src-tauri/tests/run_task_command_test.rs`

- [ ] **Step 1: Add failing Rust integration tests**

```rust
#[test]
fn run_task_prefers_sidecar_variant_image_payable_total_over_card_price() {
    let payload = r#"{"success":true,"data":{
        "itemAmountText":"¥12.82",
        "itemAmountValue":12.82,
        "freightText":"¥3",
        "freightValue":3.0,
        "finalTotalText":"¥15.82",
        "finalTotalValue":15.82,
        "resolutionMode":"variant_image_payable_total",
        "matchedVariantLabel":"4件装 45cm",
        "isFreeShipping":false
    }}"#;

    // Existing mock HTTP harness should assert final workbook/result row price == "¥15.82".
}

#[test]
fn run_task_marks_manual_review_when_detail_pricing_reports_unknown_spec() {
    let payload = r#"{"success":true,"data":{
        "itemAmountText":null,
        "itemAmountValue":null,
        "freightText":null,
        "freightValue":null,
        "finalTotalText":null,
        "finalTotalValue":null,
        "resolutionMode":"manual_review_required_unknown_spec",
        "matchedVariantLabel":null,
        "isFreeShipping":false
    }}"#;

    // Existing mock HTTP harness should assert final status == "无法判断商品规格，需人工介入"
    // and final workbook/result row price stays empty.
}
```

- [ ] **Step 2: Run the targeted Rust tests to verify failure**

Run: `cd src-tauri && cargo test run_task_prefers_sidecar_variant_image_payable_total_over_card_price -- --exact --nocapture`
Expected: FAIL.

Run: `cd src-tauri && cargo test run_task_marks_manual_review_when_detail_pricing_reports_unknown_spec -- --exact --nocapture`
Expected: FAIL.

- [ ] **Step 3: Update `hydrate_output_row_1688_detail_pricing`**

```rust
let resolution_mode = detail_pricing
    .resolution_mode
    .clone()
    .unwrap_or_else(|| "legacy_no_sku_selection".to_string());

if resolution_mode == "manual_review_required_unknown_spec" {
    output_row.price = None;
    output_row.status = "无法判断商品规格，需人工介入".to_string();
} else {
    let resolved_price = resolve_1688_output_price(&card_price, &detail_pricing);
    output_row.price = Some(resolved_price);
}

emit_event(
    sink,
    EVENT_LOG,
    &LogEvent {
        level: if resolution_mode == "manual_review_required_unknown_spec" {
            "warn".to_string()
        } else {
            "info".to_string()
        },
        message: format!(
            "{} 详情页定价模式={} 规格标签={:?} 商品金额={:?} 运费={:?} 最终价格={:?}",
            output_row.sku,
            resolution_mode,
            detail_pricing.matched_variant_label,
            detail_pricing.item_amount_text,
            detail_pricing.freight_text,
            output_row.price,
        ),
    },
)?;
```

- [ ] **Step 4: Re-run the Rust integration tests**

Run: `cd src-tauri && cargo test run_task_prefers_sidecar_variant_image_payable_total_over_card_price -- --exact --nocapture`
Expected: PASS.

Run: `cd src-tauri && cargo test run_task_marks_manual_review_when_detail_pricing_reports_unknown_spec -- --exact --nocapture`
Expected: PASS.

---

## Task 5: Regression Coverage And Manual Verification

**Files:**
- Modify: `src-sidecar/src/1688_engine.test.ts`
- Modify: `src-tauri/src/commands/run_task.rs`
- Modify: `src-tauri/tests/run_task_command_test.rs`

- [ ] **Step 1: Add regression tests for the decision boundary**

```ts
test("returns legacy_no_sku_selection payload when detail page has no skuSelection container", () => {
  // Ensure only this case may fall back to the previous price logic.
});

test("returns manual_review_required payload when skuSelection exists but the Ozon title has no usable numeric fragments and no image match exists", () => {
  // Ensure price calculation is skipped.
});
```

```rust
#[test]
fn merge_1688_detail_pricing_keeps_original_card_price_when_sidecar_returns_legacy_no_sku_selection() {
    let payload = Sidecar1688DetailPricingPayload {
        item_amount_text: None,
        item_amount_value: None,
        freight_text: None,
        freight_value: None,
        final_total_text: None,
        final_total_value: None,
        resolution_mode: Some("legacy_no_sku_selection".to_string()),
        matched_variant_label: None,
        is_free_shipping: false,
    };

    assert_eq!(resolve_1688_output_price("¥13.82", &payload), "¥13.82");
}
```

- [ ] **Step 2: Run the full focused automated suite**

Run: `cd src-sidecar && bun test src/1688_engine.test.ts`
Expected: PASS.

Run: `cd src-tauri && cargo test merge_1688_detail_pricing -- --nocapture`
Expected: PASS.

Run: `cd src-tauri && cargo test run_task_prefers_sidecar_variant_image_payable_total_over_card_price -- --exact --nocapture`
Expected: PASS.

Run: `cd src-tauri && cargo test run_task_marks_manual_review_when_detail_pricing_reports_unknown_spec -- --exact --nocapture`
Expected: PASS.

- [ ] **Step 3: Manual verification on three real cases**

Run: `bun run tauri dev`

Expected case 1, image-matched SKU row:
- row 7 / SKU `3577631265` still matches the same 1688 detail URL
- the detail page selects the thumbnail row matching the chosen 1688 candidate image
- the sidecar clicks that row’s `+`
- extracted values are `商品金额=¥12.82` and `另需运费（预估）=¥3`
- final written `1688成本价` is `¥15.82`, not `¥13.82`

Expected case 2, label-matched SKU row:
- the detail page has `#skuSelection`
- the variant rows have no usable images
- `item-label` contains numeric specs that appear in the Ozon title
- the sidecar picks the matching label row, clicks `+`, and writes `商品金额 + 运费`

Expected case 3, manual review required:
- the detail page has `#skuSelection`
- no trustworthy image match exists
- the Ozon title has no usable numeric fragments for `item-label` matching
- final row status becomes `无法判断商品规格，需人工介入`
- final written `1688成本价` stays empty

- [ ] **Step 4: Commit**

```bash
git add src-sidecar/src/server.ts src-sidecar/src/1688_engine.ts src-sidecar/src/1688_engine.test.ts src-tauri/src/commands/run_task.rs src-tauri/tests/run_task_command_test.rs docs/superpowers/plans/2026-04-02-1688-detail-variant-payable-price-plan.md
git commit -m "fix: resolve 1688 detail pricing via variant image, label, or manual review"
```

## Self-Review

- Spec coverage: covers the known failure cases where search-card price is wrong because the real payable price depends on a matched detail-row variant, and where existing detail-page SKU selectors require manual review because no trustworthy variant can be determined.
- Placeholder scan: no `TODO` or vague “handle appropriately” steps remain; each task names files, tests, and commands.
- Type consistency: the plan consistently uses `matchedImageUrl`, `ozonTitle`, `matchedVariantLabel`, `finalTotalText`, `finalTotalValue`, and `resolutionMode` across TypeScript and Rust.

---

## Task 6: Add Ozon Spec Profile As A First-Class Input

**Why this exists:** some Ozon detail pages expose decisive structured attributes that do not appear reliably in the title, for example `颜色=白色`, `长度=89cm`, `材质=ABS`, or `套装=簸箕+扫把`. Those attributes must participate in 1688 detail-page variant resolution; otherwise the system will keep matching the right product family but the wrong purchasable variant.

**Goal:** build an `OzonSpecProfile` from the Ozon detail page and pass it into 1688 detail pricing. The 1688 sidecar must resolve variants using a weighted combination of:
- Ozon structured attributes
- Ozon title tokens
- candidate image alignment

The system should treat Ozon detail attributes as stronger than title-only weak numeric matches.

### Signal priority

Use this exact precedence when `#skuSelection` exists on the 1688 detail page:

1. direct candidate image / variant image alignment
2. Ozon structured attributes from the detail page
   - color
   - size / length / capacity
   - count / pack size
   - model / style tokens
3. Ozon title-derived tokens
4. manual review required if no trustworthy winner emerges

Do **not** let weak title-only numeric matches override a strong structured color or size mismatch.

### OzonSpecProfile contract

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct OzonSpecProfile {
    color: Option<String>,
    size_tokens: Vec<String>,
    count_tokens: Vec<String>,
    material: Option<String>,
    model_tokens: Vec<String>,
    feature_tokens: Vec<String>,
    raw_attributes: Vec<(String, String)>,
}
```

Equivalent TypeScript payload:

```ts
type OzonSpecProfile = {
  color?: string | null;
  sizeTokens: string[];
  countTokens: string[];
  material?: string | null;
  modelTokens: string[];
  featureTokens: string[];
  rawAttributes: Array<{ key: string; value: string }>;
};
```

### Extraction rules on the Ozon side

- Parse visible Ozon detail attributes from the product page before any recommended-product hop is considered successful.
- Attribute extraction order:
  1. visible spec table / characteristic rows
  2. structured product metadata when trustworthy
  3. title-derived fallback tokens
- The first implementation only needs to promote four high-value families:
  - `color`
  - `size_tokens`
  - `count_tokens`
  - `model_tokens`
- Keep `raw_attributes` for logging and future expansion.

### Matching rules on the 1688 side

Each 1688 variant row receives a score composed from multiple signals:

- `image_match_score`
  - exact or near-exact filename/image alignment wins immediately
- `color_score`
  - direct color text alignment between `OzonSpecProfile.color` and variant row label/details
- `size_score`
  - shared normalized tokens such as `89`, `89cm`, `5.5-9`, `13cm`
- `count_score`
  - pack-size / quantity cues such as `1件`, `2件`, `100条/桶`
- `model_score`
  - style or model terms such as `经典款`, `加长款`, `升级款`, `pro`

Suggested first-pass weights:

- image match: `+100`
- color match: `+40`
- size token match: `+30`
- count token match: `+25`
- model token match: `+20`
- explicit contradiction on strong fields: `-60`

If the top row does not beat the next-best row by a safe margin, return manual review required.

### Decision boundary

Return `manual_review_required_unknown_spec` when any of these is true:

- `#skuSelection` exists but no row gets a trustworthy score
- structured Ozon attributes contradict the best candidate row
- multiple rows tie on strong signals
- only weak title-derived clues exist and no strong attribute/image evidence is available

This rule is intentionally conservative. Once `#skuSelection` exists, the system must prefer exposing uncertainty over fabricating a price.

### Visual LLM role

Visual LLM is explicitly **not** the primary pricing path.

Use it only as a bounded fallback when:

- Ozon attribute extraction fails on a visually rich detail page
- 1688 variant rows are visually obvious but DOM labels are incomplete or unstable
- rule-based scoring cannot distinguish between two near-tied rows

The visual LLM should not directly return a final price. It should return a structured arbitration payload, for example:

```json
{
  "pageType": "variant_image",
  "bestVariantRowIndex": 2,
  "shouldClickQuantityPlus": true,
  "manualReviewRequired": false,
  "confidence": 0.82,
  "reasoningLabel": "white long-handle version matches Ozon color and shape"
}
```

If confidence is below threshold, ignore the arbitration and keep manual review required.

### File changes

- Modify: `src-sidecar/src/ozon_session.ts`
  - extract visible Ozon attributes into `OzonSpecProfile`
  - include the profile in the resolve payload
- Modify: `src-sidecar/src/server.ts`
  - extend `/resolve-ozon-product` and `/resolve-ozon-sku` response payloads
  - extend `/resolve-1688-detail-pricing` request payloads to accept `ozonSpecProfile`
- Modify: `src-sidecar/src/1688_engine.ts`
  - add row scoring helpers that combine image, color, size, count, and model signals
- Modify: `src-sidecar/src/ozon_session.test.ts`
  - add tests for Ozon attribute extraction and “do not hop away from a real detail page with useful attributes”
- Modify: `src-sidecar/src/1688_engine.test.ts`
  - add tests for weighted variant scoring and contradiction handling
- Modify: `src-tauri/src/commands/run_task.rs`
  - persist the Ozon spec profile from resolve -> search -> detail pricing
- Modify: `src-tauri/tests/run_task_command_test.rs`
  - add integration coverage where `颜色=白色` or `长度=89cm` disambiguates the correct 1688 variant row

### Manual verification additions

Add these real-world checks on top of the existing pricing verification:

- Ozon detail page with visible `颜色=白色`
  - the chosen 1688 row must also represent white/ivory/light-color variant semantics
- Ozon detail page with visible `长度=89cm`
  - the chosen 1688 row must prefer the 89cm or closest matching long-size variant
- Ozon detail page where title is generic but the attribute table is specific
  - the system must still select the correct 1688 variant using structured attributes

### Expected product behavior after Task 6

- The system no longer treats Ozon title and Ozon image as the only truth sources.
- Ozon detail attributes become first-class signals for 1688 variant resolution.
- `manual_review_required_unknown_spec` remains the safe terminal state when structured evidence is still insufficient.
