# Ozon Preflight And 1688 Login-Gated Flow Design

## Goal

Align the desktop app runtime flow with the confirmed business process:

1. User clicks `开始执行`
2. App resolves Ozon product detail pages in the background
3. Only if at least one row has a valid source image, the app launches Chrome / 1688
4. If 1688 is not logged in, the app pauses and waits for login
5. After login is ready, the app starts the existing 1688 image search + VLM matching flow

This design intentionally keeps Ozon source acquisition headless over HTTP and keeps 1688 as the only visible browser automation surface.

## Confirmed Business Flow

### Expected behavior

For a URL-based workbook:

1. Load workbook rows
2. Validate runtime prerequisites that do not require launching Chrome
3. Resolve each Ozon row in the background:
   - fetch product detail page
   - extract product title
   - extract first main image
4. Split rows into:
   - executable rows
   - source-failure rows
5. If zero executable rows remain:
   - do not launch Chrome
   - do not start sidecar
   - export `result.xlsx` directly
6. If one or more executable rows remain:
   - launch sidecar
   - open Chrome on 1688
   - block until login is ready
7. Run the current serial 1688 + VLM pipeline only for executable rows

### Explicit non-goal

The app must not open a visible Ozon product page window.

Ozon detail fetching is an internal HTTP stage only.

## Current Gap

The current implementation does not match the target flow in one critical area:

- it starts sidecar / Chrome before row-level Ozon resolution

That means Chrome is launched too early, before the app knows whether:

- any row is actually executable
- all rows are invalid or off-shelf
- the task should end without ever touching 1688

This mismatch also makes failures hard to understand because:

- Ozon resolution is invisible by design
- task-level startup errors occur before row-level work becomes obvious

## Recommended Approach

### Chosen approach

Introduce a task-level preflight stage before sidecar startup.

The new runtime becomes:

1. Task preflight
2. Ozon source resolution
3. Conditional sidecar launch
4. 1688 login gate
5. Existing row execution pipeline

### Why this is the right approach

- Matches the confirmed business process exactly
- Avoids launching Chrome when every row already failed at source resolution
- Makes login gating deterministic and easier to explain in UI
- Preserves the current stable 1688 / VLM internals

### Rejected alternatives

#### Keep current startup order and only improve error messages

Rejected because it still violates the required business flow.

#### Launch Chrome immediately and resolve Ozon rows in parallel behind it

Rejected because it wastes a browser session and still creates user confusion when all source rows fail early.

## Runtime Architecture

### Stage 1: Task preflight

This stage runs before any browser is launched.

Responsibilities:

- validate uploaded workbook path
- validate `DASHSCOPE_API_KEY`
- validate that the sidecar binary exists
- load workbook rows

If any of these fail, the app should stop with a task-level error message.

### Stage 2: Ozon source resolution

This stage resolves the input workbook into two groups.

#### Executable row

A row is executable only when:

- the Ozon URL is valid
- the product page is accessible
- the title is extracted
- the first main image is extracted and downloaded

#### Source-failure row

A row is terminal before 1688 when any of the following occurs:

- `Ozon链接无效`
- `Ozon商品已下架或不可访问`
- `未解析到Ozon商品标题`
- `未解析到Ozon商品主图`

These rows are finalized immediately and never consume sidecar, Chrome, 1688, or VLM capacity.

### Stage 3: Conditional sidecar launch

After preflight resolution:

- if executable row count is `0`, skip sidecar entirely
- if executable row count is `> 0`, start sidecar and launch Chrome

This is the main behavioral correction.

### Stage 4: 1688 login gate

Once sidecar is up:

1. query `/session-state`
2. if status is `login_required`, show blocking UI and pause
3. if status is `anti_bot_challenge`, show challenge UI and pause
4. only continue when status becomes `ready`

No row should enter 1688 image search before login is confirmed ready.

### Stage 5: Existing serial row execution

Only rows from the executable set continue into:

- search image planning
- search image generation
- 1688 image search
- VLM screening
- VLM final review
- cheapest match selection

Rows that failed in Stage 2 remain in the final export and monitor table, but are skipped during Stage 5.

## UI / Status Model

The monitor should distinguish task-level stages from row-level stages.

### Task-level stages

Suggested task-level phases:

- `校验运行环境`
- `解析 Ozon 商品源`
- `等待 1688 登录`
- `执行 1688 搜款与 AI 复核`

### Row-level stages

Rows should continue using row events, but URL rows must visibly show:

- `resolving_ozon_product`
- then either:
  - terminal source-failure status
  - or `planning_search_image`

### Key UX improvement

When Chrome does not open, the user must be able to tell whether the task is:

- blocked before sidecar startup
- still resolving Ozon sources
- skipped sidecar because all rows already failed
- waiting for login

## Export Rules

### Source-failure rows

- `处理状态`: source failure reason
- `AI分析结论`: empty
- `1688成本价`: empty
- `1688链接`: empty
- `匹配图`: empty

### Executed rows

Keep the current behavior:

- `处理状态`: processing outcome
- `AI分析结论`: AI comparison outcome when applicable

## Error Handling

### Preflight errors

Task-level hard stop:

- missing API key
- invalid Excel path
- sidecar binary missing

### Source-resolution errors

Row-level terminal stop:

- invalid Ozon URL
- unavailable product
- missing title
- missing image

### Sidecar / browser errors

Task-level blocking errors:

- Chrome not found
- sidecar start timeout
- login required
- anti-bot challenge

## Testing Strategy

### Automated tests

Add or update tests to cover:

1. all rows fail Ozon resolution
   - task exports directly
   - sidecar is not started
2. some rows succeed Ozon resolution
   - sidecar starts
   - login gate is checked before search
3. login-required state
   - row execution does not start before login is ready
4. mixed workbook
   - failed rows export terminal source status
   - successful rows continue into current pipeline

### Manual verification

Expected manual paths:

1. all-invalid Ozon workbook
   - no Chrome opens
   - `result.xlsx` still exports
2. partially valid Ozon workbook
   - Chrome opens once
   - waits for login if needed
   - then only valid rows continue
3. valid workbook with existing login session
   - Chrome opens
   - immediately enters 1688 workflow

## Risks

### Risk 1: Preflight takes noticeable time before Chrome appears

This is acceptable because it matches the confirmed business process.

Mitigation:

- expose task-level `解析 Ozon 商品源` status clearly

### Risk 2: Users think “nothing happened” because Ozon is headless

Mitigation:

- explicit task-level progress
- explicit row-level `resolving_ozon_product`

### Risk 3: Mixed success/failure rows complicate progress counts

Mitigation:

- progress should count processed rows uniformly
- but only executable rows enter browser-dependent stages

## Implementation Boundary

This design changes runtime orchestration order only.

It does not change:

- the existing search-image generation algorithm
- the existing 1688 image search algorithm
- the existing VLM screening/final-review logic
- the existing serial execution model
