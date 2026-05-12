# Repository Guidelines

## Project Structure & Module Organization
`src/` contains the Vue 3 + TypeScript UI, including views, composables, stores, and `__tests__` folders. `src-tauri/` contains the Rust desktop backend, Tauri commands, lifecycle logic, and integration tests under `src-tauri/tests/`. `src-sidecar/` contains the Bun-based crawler/service used by the desktop pipeline. Supporting docs and release notes live in `docs/` and `.github/workflows/`. Treat `dist/` and `src-tauri/target/` as generated output, not hand-edited source.

## Build, Test, and Development Commands
Use Bun from the repo root unless noted otherwise.

- `bun run tauri dev` starts Vite and launches the Tauri app for local development.
- `bun run build` runs `vue-tsc --noEmit` and produces the frontend bundle.
- `bun run tauri build` packages the desktop app using the current frontend bundle and configured Tauri binaries.
- `cd src-sidecar && bun test` runs Bun tests for the sidecar and shared TS helpers.
- `cd src-tauri && cargo test` runs the Rust integration and unit tests.
- `bash scripts/e2e-smoke.sh /absolute/path/to/input.xlsx` runs the end-to-end smoke check when sidecar binaries and required env vars are present.

## Coding Style & Naming Conventions
Vue files use `<script setup lang="ts">`, PascalCase component names, and colocated composables such as `useTaskRunner.ts`. Keep TypeScript imports in double quotes and follow the existing semicolon-terminated style. Rust follows standard `rustfmt` output: 4-space indentation, `snake_case` modules/functions, and command modules under `src-tauri/src/commands/`. Prefer descriptive filenames such as `MonitorView.vue`, `settings.ts`, and `run_task_command_test.rs`.

## Testing Guidelines
Frontend tests live beside features in `src/**/__tests__/*.test.ts` and use `bun:test`. Rust tests live in `src-tauri/tests/*_test.rs`; keep them focused on command behavior, recovery flows, and pipeline regressions. Add or update targeted tests with each behavior change, especially when touching task execution, settings persistence, or Ozon resolution logic.

## Commit & Pull Request Guidelines
Recent history follows short Conventional Commit prefixes: `feat:`, `fix:`, `docs:`, and `test:`. Keep subjects imperative and specific, for example `fix: harden ozon source resolution`. PRs should describe the user-visible change, list the commands you ran, link the relevant issue or plan doc, and include screenshots when UI behavior changes. Call out platform-specific effects if the change impacts Tauri packaging or sidecar binaries.

## Configuration & Release Notes
Do not commit secrets. Release-sensitive notes live in `docs/release-secrets.md`, and unsigned Windows packaging is documented in `README.md` and `.github/workflows/windows-unsigned.yml`. If you touch release logic, verify the sidecar binary expectations under `src-tauri/binaries/` and update the related docs in the same change.


## Codebase Functional Overview
### Product Purpose
这是一个面向 Ozon x 1688 选品/比价场景的 Tauri 桌面应用。用户上传包含 Ozon 商品数据的 `.xlsx` 文件后，应用会先解析 Ozon 商品标题与主图，再驱动本地 Chrome / Sidecar 对 1688 做以图搜货，随后通过 DashScope 多模态能力进行候选初筛和终审，最后把结果与诊断信息回写到源 Excel 同级目录的 `result.xlsx`。

### End-to-End Runtime Flow
1. 前端 `TaskRunnerView` 负责拖拽/选择 Excel，并先调用 `upload_excel_file` 将文件复制到本地临时任务区，同时通过 `upload_progress` 事件实时显示上传进度。
2. 用户点击运行后，前端调用 Tauri 命令 `run_task`；Rust 后端会校验 Excel 路径、运行环境、DashScope Key、Chrome/sidecar 可用性，并清理历史缓存。
3. Rust 在真正执行 1688 搜索前，会先进入 Ozon 预处理阶段：解析每一行的 Ozon 商品信息、标题、主图以及可见属性表，无法解析的行会直接标记为最终失败结果。
4. 若仍有可执行行，Rust 会拉起 Bun sidecar，并等待 1688 登录状态就绪；若遇到登录缺失、Chrome 缺失、Ozon 风控/验证码等情况，会通过 `blocking_alert` 和 `task_phase` 事件把任务切到人工介入状态。
5. 对每一条可执行行，Rust 会把 Ozon 源图落盘后调用 sidecar `/search` 做 1688 图片搜索；sidecar 会先用源图做首搜，必要时自动拉满 1688 裁剪框并执行第二遍整图重搜。Rust 再通过 `core/orchestrator.rs` 里的 staged AI review（召回候选 → 初筛 → 终审）选出最优商品。
6. 当最终候选确定后，Rust 会再调用 sidecar `/resolve-1688-detail-pricing` 进入详情页重定价链路：若详情页存在 `#skuSelection`，则 sidecar 会按“规格图匹配 -> Ozon 规格画像打分 -> 标题数字补充 -> 人工介入”这条状态机选中规格、点击数量 `+`，并读取 `#submitOrder` 中的商品金额与运费，计算真实 `1688成本价`。
7. 每一行的状态、图片、价格、链接、耗时会持续通过 `row_result` / `progress` / `log` / `task_done` 事件推到前端监控面板，而不是等整批任务结束后一次性刷新。
8. 全部处理完成后，Rust 会输出 `result.xlsx`；在命中失败、候选不足、AI 拒绝、详情页定价失败等场景下，还会额外生成诊断产物或结构化日志，便于复核问题行。

### Responsibility Split
- `src/`: Vue 3 前端控制台。`SettingsView` 管理运行时配置，`TaskRunnerView` 管理文件上传与任务启动，`MonitorView` 订阅事件并展示行级状态、阻断提醒、日志和阶段摘要。
- `src-tauri/`: 主业务编排层。`commands/run_task.rs` 是核心入口，负责 Excel 读取、Ozon 预处理、sidecar 生命周期、AI 编排、结果导出、事件派发与恢复控制；`commands/settings.rs` 管理配置持久化；`commands/upload.rs` 负责大文件分块复制与上传进度通知。
- `src-tauri/src/core/`: 领域能力层，包括 Excel 图片提取、1688 候选匹配、Ozon 结果缓存、VLM 调用与两阶段候选编排。
- `src-sidecar/`: Bun + Puppeteer 浏览器自动化服务。`server.ts` 暴露 `/search`、`/resolve-ozon-product`、`/resolve-1688-detail-pricing`、`/capture-image`、`/session-state`、`/shutdown` 等 HTTP 接口，负责维持单浏览器会话、检测 1688 登录态、处理 Ozon 页面解析，并执行 1688 以图搜图与详情页定价。

### Key Runtime Characteristics
- 单浏览器/低风控：UI 文案和 sidecar 设计都强调单浏览器、串行处理、人工登录/验证码恢复，目标是降低站点风控概率。
- 行级实时反馈：前端监控面板并不依赖最终导出文件，而是基于 Tauri 事件实时更新每一行的执行阶段。
- API Key 不落盘：`DASHSCOPE_API_KEY` 只写入当前运行时环境，配置文件里不会持久化密钥。
- Chrome 路径可手动指定：未指定时按平台自动探测；macOS `.app` 路径会被标准化到实际可执行文件路径。
- 结果可审计：除了 `result.xlsx`，后端还会按需输出诊断文件，并在详情页定价链路中输出结构化日志，方便排查“无候选”“终审拒绝”“源图不可用”“规格未命中”“数量 + 未点中”“submitOrder 未刷新”等问题。
- sidecar 二进制不是自动热更新的：桌面端实际运行的是 `src-tauri/binaries/engine-*`；修改 `src-sidecar/` 后，需要先重建 sidecar 二进制，再运行 `bun run tauri dev`，否则 Tauri 会继续使用旧二进制。

### Important Domain Concepts
- Ozon 预处理：先确认 Ozon 商品标题和主图是否可用，避免无效源图进入 1688 搜索阶段。
- Source Image Two-Pass Recall：默认直接拿 Ozon 源图做 1688 首搜；如果无法确认 1688 默认裁剪已覆盖整图，就自动拉满裁剪框并做第二遍整图重搜。
- Staged AI Review：先消费最终一轮召回候选做 AI 初筛，再做严格终审，最终从确认同款的候选中选最低价或标记无匹配。
- Recovery Gate：当 sidecar 返回登录缺失、Chrome 缺失、反爬挑战等错误时，Rust 会暂停任务，等待用户在浏览器中处理完成后再继续。
- 1688 Detail Pricing：搜索结果卡片价只用于候选筛选；真正写入 `1688成本价` 的优先来源是 1688 详情页 `#submitOrder` 区域中的商品金额与运费之和。
- OzonSpecProfile：从 Ozon 详情页属性表和标题中提取的规格画像，当前重点包括 `color`、`sizeTokens`、`countTokens`、`material`、`modelTokens`。它会作为 1688 详情页规格判断的输入，而不再只依赖标题和图片。
- Variant Resolution State Machine：当 1688 详情页存在 `#skuSelection` 时，sidecar 会优先用规格图匹配，再用 Ozon 规格画像打分，再退回标题数字匹配；如果仍不能稳定区分规格，就返回 `manual_review_required_unknown_spec`，Rust 会把该行标记为 `无法判断商品规格，需人工介入` 并留空价格。
- Detail Pricing Diagnostics：详情页定价成功时会记录 `mode / matchedVariantLabel / quantityPlusClicked / 商品金额 / 运费 / submitOrderText / 最终价格`；失败时会把同一组诊断信息串进 warn 日志，便于定位是规格命中失败、数量 `+` 没点中，还是价格区 selector 漂移。
