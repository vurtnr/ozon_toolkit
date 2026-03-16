<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  extractDisplayFileName,
  formatFileSize,
  isXlsxPath,
  normalizeDroppedPath,
  pickFirstUriPath,
  shouldEnableRun,
  useTaskRunner,
} from "../composables/useTaskRunner";

const runner = useTaskRunner();
const dragActive = ref(false);
const dropHint = ref("");
const dropHintType = ref<"success" | "error" | "">("");
let unlistenNativeDrop: UnlistenFn | null = null;

const canRun = computed(
  () =>
    shouldEnableRun(
      runner.running.value,
      runner.uploading.value,
      runner.uploadedExcelPath.value,
    ),
);
const selectedFileName = computed(() =>
  extractDisplayFileName(runner.excelPath.value),
);
const hasSelectedExcel = computed(() => runner.excelPath.value.trim().length > 0);
const uploadPercent = computed(() =>
  Math.max(0, Math.min(100, Math.round(runner.uploadProgress.value.percent || 0))),
);
const uploadBytesText = computed(
  () =>
    `${formatFileSize(runner.uploadProgress.value.uploaded_bytes || 0)} / ${formatFileSize(
      runner.uploadProgress.value.total_bytes || 0,
    )}`,
);
const runStatusTone = computed(() => {
  if (runner.running.value) return "ready";
  if (runner.uploading.value) return "info";
  if (canRun.value) return "ready";
  return "warn";
});
const runStatusText = computed(() => {
  if (runner.running.value) return "任务执行中";
  if (runner.uploading.value) return "文件上传中";
  if (canRun.value) return "环境已就绪，可启动任务";
  return "等待 Excel 文件";
});

async function handleDrop(event: DragEvent) {
  event.preventDefault();
  dragActive.value = false;
  dropHint.value = "";
  dropHintType.value = "";

  const uriList = event.dataTransfer?.getData("text/uri-list") || "";
  const uriPath = pickFirstUriPath(uriList);
  if (uriPath) {
    const normalizedUriPath = normalizeDroppedPath(uriPath);
    if (!isXlsxPath(normalizedUriPath)) {
      dropHint.value = "仅支持 .xlsx 文件，当前文件不符合要求，上传失败";
      dropHintType.value = "error";
      return;
    }
    const ok = await runner.setDroppedPath(normalizedUriPath);
    if (ok) {
      dropHint.value = "已识别 Excel 文件，上传完成后可开始执行";
      dropHintType.value = "success";
    } else {
      dropHint.value = runner.errorMessage.value || "上传失败，请重试";
      dropHintType.value = "error";
    }
    return;
  }

  const file = event.dataTransfer?.files?.[0] as File & { path?: string };
  const candidatePath = file?.path || file?.name || "";
  const normalizedCandidate = normalizeDroppedPath(candidatePath);

  if (!normalizedCandidate) {
    dropHint.value = "未读取到文件路径，请使用“选择文件”按钮";
    dropHintType.value = "error";
    return;
  }

  if (!isXlsxPath(normalizedCandidate)) {
    dropHint.value = "仅支持 .xlsx 文件，当前文件不符合要求，上传失败";
    dropHintType.value = "error";
    return;
  }

  const ok = await runner.setDroppedPath(normalizedCandidate);
  if (ok) {
    dropHint.value = "已识别 Excel 文件，上传完成后可开始执行";
    dropHintType.value = "success";
  } else {
    dropHint.value = runner.errorMessage.value || "上传失败，请重试";
    dropHintType.value = "error";
  }
}

function handleDragOver(event: DragEvent) {
  event.preventDefault();
  dragActive.value = true;
}

function handleDragLeave() {
  dragActive.value = false;
}

async function handleNativeDroppedPath(path: string) {
  const normalizedPath = normalizeDroppedPath(path).trim();
  if (!normalizedPath) return;

  dragActive.value = false;
  dropHint.value = "";
  dropHintType.value = "";

  if (!isXlsxPath(normalizedPath)) {
    dropHint.value = "仅支持 .xlsx 文件，当前文件不符合要求，上传失败";
    dropHintType.value = "error";
    return;
  }

  const ok = await runner.setDroppedPath(normalizedPath);
  if (ok) {
    dropHint.value = "已识别 Excel 文件，上传完成后可开始执行";
    dropHintType.value = "success";
  } else {
    dropHint.value = runner.errorMessage.value || "上传失败，请重试";
    dropHintType.value = "error";
  }
}

async function handleChooseFileClick() {
  dropHint.value = "正在打开文件选择器...";
  dropHintType.value = "success";
  const result = await runner.browseExcelFile();
  if (result === "uploaded") {
    dropHint.value = "已选择并上传 Excel 文件，可点击“开始执行”";
    dropHintType.value = "success";
    return;
  }
  if (result === "cancelled") {
    dropHint.value = "未选择文件";
    dropHintType.value = "error";
    return;
  }
  dropHint.value = runner.errorMessage.value || "上传失败，请重试";
  dropHintType.value = "error";
}

onMounted(async () => {
  try {
    const webview = getCurrentWebview();
    unlistenNativeDrop = await webview.onDragDropEvent((event) => {
      if (event.payload.type === "enter" || event.payload.type === "over") {
        dragActive.value = true;
        return;
      }
      if (event.payload.type === "leave") {
        dragActive.value = false;
        return;
      }
      if (event.payload.type === "drop") {
        const firstPath = event.payload.paths?.[0] || "";
        void handleNativeDroppedPath(firstPath);
      }
    });
  } catch {
    // Non-tauri environment fallback uses HTML5 drag events only.
  }
});

onUnmounted(() => {
  if (unlistenNativeDrop) {
    unlistenNativeDrop();
    unlistenNativeDrop = null;
  }
});
</script>

<template>
  <section class="task-runner panel-surface">
    <header class="panel-header">
      <div>
        <p class="section-kicker">Task Deck</p>
        <h2 class="section-title">Excel 任务入口</h2>
      </div>
      <span class="status-pill" :data-tone="runStatusTone">{{ runStatusText }}</span>
    </header>

    <p class="muted-copy">
      拖拽或选择 Ozon 数据格式 Excel，桌面端会先上传到本地任务区，再串行调用 Chrome 完成 1688 搜索和 AI 复核。
    </p>

    <div
      class="drop-zone"
      :class="{ 'drop-zone--active': dragActive, 'drop-zone--ready': hasSelectedExcel }"
      @dragover="handleDragOver"
      @dragleave="handleDragLeave"
      @drop="handleDrop"
    >
      <div v-if="hasSelectedExcel" class="file-ready">
        <span class="file-icon" aria-hidden="true">XLSX</span>
        <div class="file-meta">
          <strong class="file-name">{{ selectedFileName }}</strong>
          <small>文件已装载到本地任务缓冲区，可直接开始执行</small>
        </div>
      </div>
      <div v-else class="drop-placeholder">
        <strong>{{ dragActive ? "松开鼠标以添加文件" : "将 .xlsx 文件拖到这里" }}</strong>
        <small>桌面端会校验路径、复制文件并实时回显上传进度</small>
      </div>
    </div>

    <div v-if="runner.uploading.value || runner.uploadProgress.value.total_bytes > 0" class="upload-progress">
      <div class="upload-head">
        <span>{{ runner.uploading.value ? "上传中..." : "上传完成" }}</span>
        <strong>{{ uploadPercent }}%</strong>
      </div>
      <div class="progress-track">
        <span class="progress-bar" :style="{ width: `${uploadPercent}%` }"></span>
      </div>
      <small class="upload-meta">{{ uploadBytesText }}</small>
    </div>

    <div class="action-row">
      <button type="button" class="secondary-btn" :disabled="runner.uploading.value" @click="handleChooseFileClick">
        选择文件
      </button>
      <button type="button" class="primary-btn" :disabled="!canRun" @click="runner.runTask">
        {{ runner.uploading.value ? "上传中..." : runner.running.value ? "执行中..." : "开始执行" }}
      </button>
    </div>

    <div class="tips-grid">
      <article class="tip-card">
        <span>01</span>
        <p>搜索始终保持单浏览器节奏，优先兼顾稳定性与风控安全。</p>
      </article>
      <article class="tip-card">
        <span>02</span>
        <p>实时监控会先显示阶段状态，再补齐最终价格与 1688 链接。</p>
      </article>
      <article class="tip-card">
        <span>03</span>
        <p>完成后会在源 Excel 同级目录导出 `result.xlsx` 与诊断目录。</p>
      </article>
    </div>

    <p v-if="dropHint" :class="dropHintType === 'success' ? 'feedback feedback--success' : 'feedback feedback--error'">
      {{ dropHint }}
    </p>
    <p v-if="runner.errorMessage.value" class="feedback feedback--error">{{ runner.errorMessage.value }}</p>
    <p v-if="runner.summary.value" class="feedback feedback--success">
      已完成：{{ runner.summary.value.processed_rows }}/{{ runner.summary.value.total_rows }}
    </p>
  </section>
</template>

<style scoped>
.task-runner {
  display: grid;
  gap: 1rem;
}

.panel-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 0.75rem;
}

.drop-zone {
  min-height: 13rem;
  padding: 1.1rem;
  border: 1px dashed rgba(111, 232, 255, 0.22);
  border-radius: var(--radius-xl);
  background:
    radial-gradient(circle at top right, rgba(111, 232, 255, 0.12), transparent 10rem),
    rgba(5, 12, 19, 0.92);
  transition: border-color 0.2s ease, transform 0.2s ease, background 0.2s ease;
}

.drop-zone--active {
  border-color: rgba(111, 232, 255, 0.52);
  transform: translateY(-2px);
}

.drop-zone--ready {
  border-color: rgba(83, 242, 178, 0.36);
}

.file-ready,
.drop-placeholder {
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 0.9rem;
  text-align: left;
}

.drop-placeholder {
  flex-direction: column;
  text-align: center;
}

.drop-placeholder strong {
  font-size: 1.02rem;
}

.drop-placeholder small {
  color: var(--text-muted);
}

.file-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 4rem;
  height: 4rem;
  border-radius: 1rem;
  background: linear-gradient(135deg, rgba(73, 140, 255, 0.9), rgba(83, 242, 178, 0.72));
  color: #03111d;
  font-weight: 800;
  letter-spacing: 0.08em;
}

.file-meta {
  display: grid;
  gap: 0.3rem;
}

.file-name {
  font-size: 1rem;
}

.file-meta small,
.upload-meta {
  color: var(--text-muted);
}

.upload-progress {
  display: grid;
  gap: 0.55rem;
  padding: 0.95rem 1rem;
  border-radius: var(--radius-lg);
  background: rgba(10, 21, 33, 0.9);
  border: 1px solid rgba(111, 232, 255, 0.12);
}

.upload-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.progress-track {
  height: 0.55rem;
  border-radius: 999px;
  overflow: hidden;
  background: rgba(255, 255, 255, 0.06);
}

.progress-bar {
  display: block;
  height: 100%;
  border-radius: inherit;
  background: linear-gradient(90deg, rgba(73, 140, 255, 0.95), rgba(111, 232, 255, 0.95));
}

.action-row {
  display: flex;
  gap: 0.75rem;
}

button {
  min-height: 3rem;
  padding: 0.78rem 1.15rem;
  border-radius: 999px;
  border: 1px solid transparent;
  cursor: pointer;
  transition: transform 0.18s ease, border-color 0.18s ease, background 0.18s ease;
}

button:hover:not(:disabled) {
  transform: translateY(-1px);
}

button:disabled {
  cursor: not-allowed;
  opacity: 0.58;
}

.primary-btn {
  background: linear-gradient(135deg, rgba(73, 140, 255, 0.92), rgba(111, 232, 255, 0.9));
  color: #041018;
  font-weight: 700;
}

.secondary-btn {
  border-color: rgba(111, 232, 255, 0.2);
  background: rgba(14, 28, 44, 0.9);
  color: var(--text-primary);
}

.tips-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 0.75rem;
}

.tip-card {
  display: grid;
  gap: 0.45rem;
  padding: 0.95rem;
  border-radius: var(--radius-lg);
  border: 1px solid rgba(255, 255, 255, 0.06);
  background: rgba(8, 16, 26, 0.74);
}

.tip-card span {
  color: var(--accent-cyan);
  font-size: 0.78rem;
  letter-spacing: 0.16em;
}

.tip-card p {
  margin: 0;
  color: var(--text-muted);
  line-height: 1.65;
}

.feedback {
  margin: 0;
  font-size: 0.92rem;
}

.feedback--error {
  color: var(--danger);
}

.feedback--success {
  color: var(--accent-green);
}

@media (max-width: 900px) {
  .panel-header,
  .action-row {
    flex-direction: column;
  }

  .tips-grid {
    grid-template-columns: 1fr;
  }
}
</style>
