<script setup lang="ts">
import { computed } from "vue";
import { useTaskEvents } from "../composables/useTaskEvents";
import BlockingAlert from "../components/BlockingAlert.vue";
import {
  getLogTone,
  getStagePresentation,
  summarizeMonitorBoard,
} from "./monitorViewModel";

const monitor = useTaskEvents();

const boardSummary = computed(() =>
  summarizeMonitorBoard(
    monitor.state.rows,
    monitor.state.logs,
    monitor.state.progress,
  ),
);
const activeStage = computed(() => {
  if (!boardSummary.value.activeRow) {
    return {
      label: "等待任务启动",
      tone: "info" as const,
      emphasis: "live" as const,
    };
  }

  return getStagePresentation(boardSummary.value.activeRow);
});
const doneSummary = computed(() => {
  if (!monitor.state.done) return null;
  return `${monitor.state.done.processed_rows}/${monitor.state.done.total_rows}`;
});

function resolveMatchedImage(url: string | null, fallback: string | null): string | null {
  return url || fallback;
}

function dismissAlert() {
  monitor.state.alert = null;
}
</script>

<template>
  <section class="monitor-panel panel-surface">
    <BlockingAlert :alert="monitor.state.alert" @dismiss="dismissAlert" />
    <header class="panel-header">
      <div>
        <p class="section-kicker">Operations Board</p>
        <h2 class="section-title">实时任务监控</h2>
      </div>
      <div class="meta-pills">
        <span class="status-pill" data-tone="info">进度 {{ boardSummary.progressText }}</span>
        <span class="status-pill" :data-tone="activeStage.tone">
          当前阶段 {{ activeStage.label }}
        </span>
        <span v-if="doneSummary" class="status-pill" data-tone="ready">
          已完成 {{ doneSummary }}
        </span>
      </div>
    </header>

    <div class="overview-grid">
      <article class="summary-card">
        <span class="summary-label">活跃任务</span>
        <strong>{{ boardSummary.activeRow?.sku || "等待新任务" }}</strong>
        <small>
          {{
            boardSummary.activeRow?.status ||
            "上传 Excel 后，系统会从排队、搜索到 AI 复核分阶段更新。"
          }}
        </small>
      </article>
      <article class="summary-card">
        <span class="summary-label">已锁定结果</span>
        <strong>{{ boardSummary.completedCount }}</strong>
        <small>已找到最终价格与 1688 商品链接的行数。</small>
      </article>
      <article class="summary-card">
        <span class="summary-label">待处理 / 待复核</span>
        <strong>{{ boardSummary.pendingCount }}</strong>
        <small>正在执行浏览器搜索、AI 初筛或严格终审的行数。</small>
      </article>
      <article class="summary-card">
        <span class="summary-label">未锁定结果</span>
        <strong>{{ boardSummary.failedCount }}</strong>
        <small>已完成处理但未找到可交付商品的行数。</small>
      </article>
    </div>

    <div class="content-grid">
      <section class="board-card">
        <div class="board-head">
          <div>
            <h3>行级追踪</h3>
            <p>每一行会在排队、搜索、AI 复核和出结果之间持续更新，而不是尾部一次性回填。</p>
          </div>
        </div>

        <div v-if="!monitor.state.rows.length" class="empty-state">
          <strong>等待任务开始</strong>
          <small>任务启动后，这里会先出现队列和搜索阶段，再补齐价格、耗时与商品链接。</small>
        </div>

        <div v-else class="table-scroll">
          <table class="monitor-table">
            <thead>
              <tr>
                <th>行号</th>
                <th>SKU / 状态</th>
                <th>阶段</th>
                <th>原图</th>
                <th>匹配图</th>
                <th>耗时</th>
                <th>价格</th>
                <th>链接</th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="row in monitor.state.rows"
                :key="`${row.rowIndex}-${row.sku}`"
                :data-active="!row.isFinal"
              >
                <td class="index-cell">#{{ row.rowIndex }}</td>
                <td class="status-cell">
                  <strong>{{ row.sku }}</strong>
                  <small>{{ row.status }}</small>
                </td>
                <td>
                  <span class="stage-pill" :data-tone="getStagePresentation(row).tone">
                    {{ getStagePresentation(row).label }}
                  </span>
                </td>
                <td class="thumb-cell">
                  <div v-if="row.originalImageUrl" class="thumb-frame">
                    <img
                      class="thumb-image"
                      :src="row.originalImageUrl"
                      :alt="`${row.sku} 原图`"
                      loading="lazy"
                    />
                  </div>
                  <span v-else class="muted-token">--</span>
                </td>
                <td class="thumb-cell">
                  <div
                    v-if="resolveMatchedImage(row.matchedImageUrl, row.imageUrl)"
                    class="thumb-frame"
                  >
                    <img
                      class="thumb-image"
                      :src="resolveMatchedImage(row.matchedImageUrl, row.imageUrl) || undefined"
                      :alt="`${row.sku} 匹配图`"
                      loading="lazy"
                      referrerpolicy="no-referrer"
                    />
                  </div>
                  <span v-else class="muted-token">--</span>
                </td>
                <td>{{ row.elapsedText || "--" }}</td>
                <td class="price-cell">{{ row.price || "--" }}</td>
                <td>
                  <a
                    v-if="row.itemUrl"
                    class="link-btn"
                    :href="row.itemUrl"
                    target="_blank"
                    rel="noreferrer"
                  >
                    打开 1688
                  </a>
                  <span v-else class="muted-token">--</span>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </section>

      <aside class="side-rail">
        <article class="rail-card">
          <div class="rail-head">
            <h3>当前聚焦</h3>
            <span class="status-pill" :data-tone="activeStage.tone">{{ activeStage.label }}</span>
          </div>
          <div v-if="boardSummary.activeRow" class="focus-body">
            <strong>{{ boardSummary.activeRow.sku }}</strong>
            <p>{{ boardSummary.activeRow.status }}</p>
            <small>耗时 {{ boardSummary.activeRow.elapsedText || "执行中" }}</small>
          </div>
          <div v-else class="focus-body">
            <strong>暂无活跃行</strong>
            <p>等待上传文件并启动任务。</p>
            <small>完成后会自动在结果区回显导出状态。</small>
          </div>
        </article>

        <article class="rail-card">
          <div class="rail-head">
            <h3>最近日志</h3>
            <span class="status-pill" data-tone="info">{{ boardSummary.recentLogs.length }} 条</span>
          </div>
          <ul v-if="boardSummary.recentLogs.length" class="log-list">
            <li v-for="(entry, index) in boardSummary.recentLogs" :key="`${entry.message}-${index}`">
              <span class="log-pill" :data-tone="getLogTone(entry.level)">{{ entry.level }}</span>
              <p>{{ entry.message }}</p>
            </li>
          </ul>
          <div v-else class="log-empty">
            <strong>尚无日志</strong>
            <small>开始执行后，会在这里看到 Chrome 搜索、AI 复核与诊断写入进度。</small>
          </div>
        </article>
      </aside>
    </div>
  </section>
</template>

<style scoped>
.monitor-panel {
  display: grid;
  gap: 1rem;
}

.panel-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 0.75rem;
}

.meta-pills {
  display: flex;
  flex-wrap: wrap;
  gap: 0.55rem;
}

.overview-grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 0.8rem;
}

.summary-card,
.board-card,
.rail-card {
  border-radius: var(--radius-lg);
  border: 1px solid rgba(255, 255, 255, 0.06);
  background: rgba(7, 15, 25, 0.84);
}

.summary-card {
  display: grid;
  gap: 0.42rem;
  padding: 1rem;
}

.summary-label {
  color: var(--text-dim);
  font-size: 0.76rem;
  letter-spacing: 0.14em;
  text-transform: uppercase;
}

.summary-card strong {
  font-size: 1.45rem;
}

.summary-card small,
.board-head p,
.focus-body p,
.log-empty small {
  color: var(--text-muted);
  line-height: 1.6;
}

.content-grid {
  display: grid;
  grid-template-columns: minmax(0, 1.65fr) minmax(19rem, 0.85fr);
  gap: 1rem;
  align-items: start;
}

.board-card,
.rail-card {
  display: grid;
  gap: 0.95rem;
  padding: 1rem;
}

.board-head h3,
.rail-head h3 {
  margin: 0;
  font-size: 0.98rem;
}

.board-head p,
.focus-body p,
.log-list p,
.log-empty strong {
  margin: 0;
}

.table-scroll {
  overflow-x: auto;
}

.monitor-table {
  width: 100%;
  border-collapse: collapse;
  min-width: 72rem;
}

.monitor-table th,
.monitor-table td {
  padding: 0.85rem 0.75rem;
  text-align: left;
  border-bottom: 1px solid rgba(255, 255, 255, 0.06);
  vertical-align: top;
}

.monitor-table th {
  color: var(--text-dim);
  font-size: 0.76rem;
  letter-spacing: 0.12em;
  text-transform: uppercase;
}

.monitor-table tbody tr[data-active="true"] {
  background: rgba(73, 141, 255, 0.08);
}

.index-cell,
.price-cell {
  white-space: nowrap;
}

.thumb-cell {
  width: 7.5rem;
}

.status-cell {
  min-width: 16rem;
}

.status-cell strong {
  display: block;
  margin-bottom: 0.2rem;
}

.status-cell small,
.muted-token {
  color: var(--text-muted);
}

.price-cell {
  color: var(--accent-green);
  font-weight: 700;
}

.thumb-frame {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 5.5rem;
  height: 5.5rem;
  overflow: hidden;
  border-radius: 1rem;
  border: 1px solid rgba(111, 232, 255, 0.14);
  background:
    linear-gradient(160deg, rgba(9, 25, 39, 0.92), rgba(5, 12, 19, 0.94));
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.05),
    0 18px 32px rgba(0, 0, 0, 0.18);
}

.thumb-image {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.stage-pill,
.log-pill {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-height: 1.9rem;
  padding: 0.35rem 0.7rem;
  border-radius: 999px;
  border: 1px solid rgba(255, 255, 255, 0.08);
  background: rgba(255, 255, 255, 0.04);
  font-size: 0.78rem;
  color: var(--text-muted);
}

.stage-pill[data-tone="ready"],
.log-pill[data-tone="ready"] {
  border-color: rgba(83, 242, 178, 0.24);
  background: rgba(83, 242, 178, 0.12);
  color: var(--accent-green);
}

.stage-pill[data-tone="warn"],
.log-pill[data-tone="warn"] {
  border-color: rgba(255, 182, 92, 0.24);
  background: rgba(255, 182, 92, 0.12);
  color: var(--accent-amber);
}

.stage-pill[data-tone="danger"],
.log-pill[data-tone="danger"] {
  border-color: rgba(255, 127, 150, 0.24);
  background: rgba(255, 127, 150, 0.12);
  color: var(--danger);
}

.stage-pill[data-tone="info"],
.log-pill[data-tone="info"] {
  border-color: rgba(111, 232, 255, 0.24);
  background: rgba(111, 232, 255, 0.12);
  color: var(--accent-cyan);
}

.link-btn {
  display: inline-flex;
  align-items: center;
  min-height: 2rem;
  padding: 0.35rem 0.75rem;
  border-radius: 999px;
  border: 1px solid rgba(111, 232, 255, 0.24);
  background: rgba(8, 21, 33, 0.86);
  color: var(--text-primary);
  text-decoration: none;
}

.side-rail {
  display: grid;
  gap: 1rem;
}

.rail-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.65rem;
}

.focus-body {
  display: grid;
  gap: 0.45rem;
}

.focus-body strong,
.log-empty strong {
  font-size: 1rem;
}

.log-list {
  display: grid;
  gap: 0.75rem;
  padding: 0;
  margin: 0;
  list-style: none;
}

.log-list li {
  display: grid;
  gap: 0.45rem;
  padding-bottom: 0.75rem;
  border-bottom: 1px solid rgba(255, 255, 255, 0.05);
}

.log-empty,
.empty-state {
  display: grid;
  gap: 0.35rem;
  padding: 1rem;
  border-radius: var(--radius-lg);
  border: 1px dashed rgba(111, 232, 255, 0.16);
  background: rgba(4, 11, 18, 0.74);
}

.empty-state strong,
.log-empty strong {
  font-size: 0.98rem;
}

.empty-state small {
  color: var(--text-muted);
}

@media (max-width: 1180px) {
  .overview-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .content-grid {
    grid-template-columns: 1fr;
  }
}

@media (max-width: 900px) {
  .panel-header,
  .rail-head {
    flex-direction: column;
    align-items: flex-start;
  }

  .overview-grid {
    grid-template-columns: 1fr;
  }
}
</style>
