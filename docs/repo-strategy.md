# Repository Strategy

## Decision

Use `desktop_app/` as an independent Git repository for the Tauri consolidation work.

## Scope

- `brain_core/` and `browser_engine/` stay as read-only migration references during the implementation.
- All new implementation, CI, and release tags for the desktop app live in this repository.

## Ownership of delivery artifacts

- Release tags: created from `desktop_app/`.
- CI workflow location: `desktop_app/.github/workflows/release.yml`.
- Commit execution path: run all plan commits from `desktop_app/`.
