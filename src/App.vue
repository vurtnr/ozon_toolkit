<script setup lang="ts">
import SettingsView from "./views/SettingsView.vue";
import TaskRunnerView from "./views/TaskRunnerView.vue";
import MonitorView from "./views/MonitorView.vue";
</script>

<template>
  <main class="app-shell">
    <section class="hero-panel panel-surface">
      <div class="hero-copy">
        <p class="eyebrow">OZON x 1688 AUTOMATION CONSOLE</p>
        <h1>桌面任务控制台</h1>
        <p class="hero-text">
          单浏览器、低风控节奏、分阶段 AI 复核、本地结果导出。界面会实时展示每一行商品所处阶段，而不是等整条链路结束后再回写。
        </p>
      </div>

      <div class="hero-metrics">
        <article class="metric-card">
          <span class="metric-label">浏览器策略</span>
          <strong>Single Browser</strong>
          <small>只保留 1 个 Chrome 会话，避免多标签页放大风控</small>
        </article>
        <article class="metric-card">
          <span class="metric-label">复核策略</span>
          <strong>Staged AI Review</strong>
          <small>先候选召回，再严格终审，尽量缩短每行等待</small>
        </article>
        <article class="metric-card">
          <span class="metric-label">交付策略</span>
          <strong>Local Result Export</strong>
          <small>结果与诊断都落回源 Excel 同级目录，便于直接复核</small>
        </article>
      </div>
    </section>

    <section class="control-grid">
      <SettingsView />
      <TaskRunnerView />
    </section>

    <MonitorView />
  </main>
</template>

<style scoped>
.app-shell {
  position: relative;
  z-index: 1;
  display: grid;
  gap: 1.5rem;
  max-width: 1440px;
  margin: 0 auto;
}

.hero-panel {
  display: grid;
  grid-template-columns: minmax(0, 1.3fr) minmax(0, 1fr);
  gap: 1.5rem;
  align-items: stretch;
}

.hero-copy {
  display: grid;
  gap: 0.9rem;
}

.eyebrow {
  margin: 0;
  font-size: 0.78rem;
  letter-spacing: 0.24em;
  text-transform: uppercase;
  color: var(--accent-cyan);
}

h1 {
  margin: 0;
  font-size: clamp(2.2rem, 4vw, 3.8rem);
  line-height: 0.96;
  letter-spacing: -0.05em;
}

.hero-text {
  max-width: 54rem;
  margin: 0;
  color: var(--text-muted);
  font-size: 1rem;
  line-height: 1.7;
}

.hero-metrics {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 0.9rem;
}

.metric-card {
  display: grid;
  gap: 0.45rem;
  padding: 1rem;
  min-height: 9rem;
  border-radius: 1rem;
  background:
    linear-gradient(180deg, rgba(10, 24, 38, 0.82), rgba(7, 14, 24, 0.9)),
    var(--panel-secondary);
  border: 1px solid rgba(105, 227, 255, 0.12);
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.05);
}

.metric-label {
  color: var(--text-dim);
  font-size: 0.76rem;
  text-transform: uppercase;
  letter-spacing: 0.12em;
}

.metric-card strong {
  font-size: 1.08rem;
}

.metric-card small {
  color: var(--text-muted);
  line-height: 1.6;
}

.control-grid {
  display: grid;
  grid-template-columns: minmax(20rem, 24rem) minmax(0, 1fr);
  gap: 1.5rem;
  align-items: start;
}

@media (max-width: 1120px) {
  .hero-panel,
  .control-grid {
    grid-template-columns: 1fr;
  }

  .hero-metrics {
    grid-template-columns: 1fr;
  }
}
</style>
