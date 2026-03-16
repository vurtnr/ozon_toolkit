# E2E Acceptance Checklist

## Preconditions

- [ ] `DASHSCOPE_API_KEY` is set in environment.
- [ ] A real workbook exists and is reachable by absolute path.
- [ ] Sidecar binaries are present under `src-tauri/binaries/`.

## Core Flow Validation

- [ ] Open app and save settings (`API Key`, optional `Chrome path`).
- [ ] Run one `.xlsx` task from Task Runner.
- [ ] Verify monitor table streams row-level updates.
- [ ] Verify progress reaches `N/N` and completion status is `completed`.

## Human-in-the-loop Validation

- [ ] Simulate or trigger `CHROME_NOT_FOUND` and confirm blocking alert appears.
- [ ] Simulate or trigger `ANTI_BOT_CHALLENGE` and confirm resume button appears.
- [ ] Click `已验证，继续执行` and confirm queue resumes.

## Output Validation

- [ ] Confirm generated result workbook path is reported by backend summary.
- [ ] Spot-check at least 3 SKU rows for data accuracy.

## Release Validation

- [ ] `actionlint .github/workflows/release.yml` passes.
- [ ] Release workflow creates platform installers (`.msi/.exe`, `.app/.dmg`).
