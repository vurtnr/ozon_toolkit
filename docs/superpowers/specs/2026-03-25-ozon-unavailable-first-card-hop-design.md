# Design: Ozon Unavailable Page First-Card Hop

## Problem

Directly visiting an Ozon product URL does not always land on a final product detail page.
For some URLs, Ozon renders an unavailable-product page that still contains a visible multi-card product container.
The current sidecar treats this page as terminal `unavailable` and exits early, so no source title/image is recovered.

## Requirement

When a visited Ozon product URL lands on an unavailable page that still exposes a visible product-card container:

1. Detect that this is not yet the final usable product page
2. Select only the first visible product card inside the first usable multi-product container
3. Enter that product detail page
4. Resume the existing title + main-image extraction flow
5. Attempt this hop at most once per source URL
6. If the hop still does not produce a valid detail page, keep the existing unavailable/failure behavior

## Scope

In scope:
- `desktop_app/src-sidecar/src/ozon_session.ts`
- `desktop_app/src-sidecar/src/ozon_session.test.ts`

Out of scope:
- Rust orchestration changes
- 1688 matching logic
- Multi-hop recommendation traversal
- Choosing the cheapest or most similar recommended product on Ozon

## Approach

### 1. Keep the current direct-detail flow as the primary path

`resolveOzonProductViaSession()` should still:
- canonicalize the incoming Ozon product URL
- `page.goto()` that URL
- run the current snapshot-based resolution loop

### 2. Add an unavailable-page escape hatch

When `classifyOzonSnapshot()` reports `unavailable`, do not immediately fail.
Instead:

- inspect the DOM for visible product links under visible container elements
- identify the first usable multi-product container
- select the first product card in that container
- click it and wait for navigation

### 3. Define “first usable multi-product container”

Use a structural heuristic instead of class-name coupling:

- candidate links must be visible anchors with `href` pointing to `/product/...`
- exclude the current product URL
- walk upward to find the nearest visible ancestor container that contains at least 2 visible product links
- require that container to have a meaningful card-grid footprint
- choose the earliest visible container in reading order
- inside that container, choose the first visible product link in reading order

This matches the user constraint:
- only the first product
- only one container
- no secondary ranking logic

### 4. Single-hop guard

Add a local guard in `resolveOzonProductViaSession()`:

- `recommendedProductHopAttempted = false`

Behavior:
- first time `unavailable` is seen, try the hop
- if hop succeeds, continue polling as normal
- if hop fails or another unavailable page is reached after the hop, stop and return the existing unavailable error

### 5. Navigation strategy

Preferred:
- trigger a real click on the chosen product card link

Fallback:
- if the click does not cause navigation within a short timeout, navigate to the chosen href directly

This preserves the “click first card” business rule while keeping the flow robust.

## Test Strategy

Add sidecar unit coverage for the selection core:

1. choose the first product from the first valid multi-product container
2. ignore the current product URL
3. ignore single-link side containers
4. return `null` when no valid multi-product container exists

The DOM-click/navigation behavior itself will stay thin and use the pure selection helper.
