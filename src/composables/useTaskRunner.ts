import { computed, onMounted, onUnmounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  createDefaultSettings,
  isValidProfitRatioInput,
  loadSettings,
  sanitizeProfitRatioInput,
  saveSettings,
  type AppSettings,
} from "../stores/settings";

interface RunTaskSummary {
  excel_path: string;
  processed_rows: number;
  total_rows: number;
  status: string;
  result_path: string | null;
}

interface UploadSummary {
  source_path: string;
  uploaded_path: string;
  file_name: string;
  total_bytes: number;
}

export interface UploadProgressPayload {
  uploaded_bytes: number;
  total_bytes: number;
  percent: number;
  status: string;
  file_name: string;
  source_path: string;
  target_path: string | null;
}

export function normalizeDroppedPath(input: string): string {
  if (!input) return "";
  if (input.startsWith("file://")) {
    return decodeURIComponent(input.replace(/^file:\/\//, ""));
  }
  return input;
}

export function isXlsxPath(path: string): boolean {
  if (!path) return false;
  const normalized = normalizeDroppedPath(path).trim().toLowerCase();
  return normalized.endsWith(".xlsx");
}

export function isAbsoluteXlsxPath(path: string): boolean {
  if (!path) return false;
  const normalized = normalizeDroppedPath(path).trim();
  if (!isXlsxPath(normalized)) return false;
  return normalized.startsWith("/") || /^[a-zA-Z]:\\/.test(normalized);
}

export function extractDisplayFileName(path: string): string {
  const normalized = normalizeDroppedPath(path).trim();
  if (!normalized) return "";
  const parts = normalized.split(/[\\/]/).filter((item) => item.length > 0);
  return parts[parts.length - 1] || normalized;
}

export function pickFirstUriPath(uriListText: string): string {
  if (!uriListText) return "";
  const line = uriListText
    .split(/\r?\n/)
    .map((item) => item.trim())
    .find((item) => item.length > 0 && !item.startsWith("#"));
  return line || "";
}

export function buildRunTaskPayload(
  excelPath: string,
  sourceExcelPath?: string,
): { excelPath: string; sourceExcelPath?: string } {
  return sourceExcelPath ? { excelPath, sourceExcelPath } : { excelPath };
}

export function createEmptyUploadProgress(): UploadProgressPayload {
  return {
    uploaded_bytes: 0,
    total_bytes: 0,
    percent: 0,
    status: "idle",
    file_name: "",
    source_path: "",
    target_path: null,
  };
}

export function shouldEnableRun(
  running: boolean,
  uploading: boolean,
  uploadedExcelPath: string,
  hasValidProfitRatio: boolean,
): boolean {
  return !running && !uploading && isAbsoluteXlsxPath(uploadedExcelPath) && hasValidProfitRatio;
}

export function formatFileSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value.toFixed(value >= 10 || unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
}

export function useTaskRunner() {
  const excelPath = ref("");
  const uploadedExcelPath = ref("");
  const running = ref(false);
  const uploading = ref(false);
  const uploadProgress = ref<UploadProgressPayload>(createEmptyUploadProgress());
  const errorMessage = ref("");
  const summary = ref<RunTaskSummary | null>(null);
  const uploadUnlisten = ref<UnlistenFn | null>(null);
  const settings = ref<AppSettings>(createDefaultSettings());
  const profitRatioInput = ref("");
  const profitRatioLoading = ref(true);
  const profitRatioSaving = ref(false);
  const profitRatioError = ref("");
  const hasLoadedProfitRatio = ref(false);
  let profitRatioSaveTimer: ReturnType<typeof setTimeout> | null = null;

  const hasValidProfitRatio = computed(() =>
    isValidProfitRatioInput(profitRatioInput.value),
  );

  function clearProfitRatioSaveTimer() {
    if (profitRatioSaveTimer) {
      clearTimeout(profitRatioSaveTimer);
      profitRatioSaveTimer = null;
    }
  }

  async function persistProfitRatio(): Promise<boolean> {
    clearProfitRatioSaveTimer();
    const sanitized = sanitizeProfitRatioInput(profitRatioInput.value);
    if (sanitized !== profitRatioInput.value) {
      profitRatioInput.value = sanitized;
    }

    if (sanitized && !isValidProfitRatioInput(sanitized)) {
      profitRatioError.value = "利润比例必须大于 0 且小于 100，并最多保留两位小数";
      return false;
    }

    profitRatioSaving.value = true;
    try {
      settings.value = {
        ...settings.value,
        profitRatio: sanitized,
      };
      await saveSettings(settings.value);
      profitRatioError.value = "";
      return true;
    } catch (error) {
      profitRatioError.value = error instanceof Error ? error.message : String(error);
      return false;
    } finally {
      profitRatioSaving.value = false;
    }
  }

  async function loadRunnerSettings() {
    profitRatioLoading.value = true;
    try {
      const loaded = await loadSettings();
      settings.value = loaded;
      profitRatioInput.value = sanitizeProfitRatioInput(loaded.profitRatio);
      profitRatioError.value = "";
    } catch (error) {
      profitRatioError.value = error instanceof Error ? error.message : String(error);
    } finally {
      profitRatioLoading.value = false;
      hasLoadedProfitRatio.value = true;
    }
  }

  function queueProfitRatioSave() {
    if (!hasLoadedProfitRatio.value) return;
    clearProfitRatioSaveTimer();
    profitRatioSaveTimer = setTimeout(() => {
      void persistProfitRatio();
    }, 300);
  }

  function updateProfitRatioInput(value: string) {
    profitRatioInput.value = sanitizeProfitRatioInput(value);
    if (!profitRatioInput.value) {
      profitRatioError.value = "请输入利润比例";
    } else if (!isValidProfitRatioInput(profitRatioInput.value)) {
      profitRatioError.value = "利润比例必须大于 0 且小于 100，并最多保留两位小数";
    } else {
      profitRatioError.value = "";
    }
    queueProfitRatioSave();
  }

  async function browseExcelFile(): Promise<"uploaded" | "cancelled" | "failed"> {
    errorMessage.value = "";
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "Excel Workbook", extensions: ["xlsx"] }],
      });

      if (typeof selected === "string") {
        const ok = await setDroppedPath(selected);
        return ok ? "uploaded" : "failed";
      }
      return "cancelled";
    } catch (error) {
      errorMessage.value = error instanceof Error ? error.message : String(error);
      return "failed";
    }
  }

  async function setDroppedPath(value: string): Promise<boolean> {
    const normalized = normalizeDroppedPath(value).trim();
    excelPath.value = normalized;
    uploadedExcelPath.value = "";
    uploadProgress.value = createEmptyUploadProgress();

    if (!isAbsoluteXlsxPath(normalized)) {
      errorMessage.value = "请选择绝对路径的 .xlsx 文件";
      return false;
    }

    errorMessage.value = "";
    uploading.value = true;
    try {
      const uploaded = await invoke<UploadSummary>("upload_excel_file", {
        excelPath: normalized,
      });
      uploadedExcelPath.value = normalizeDroppedPath(uploaded.uploaded_path);
      uploadProgress.value = {
        uploaded_bytes: uploaded.total_bytes,
        total_bytes: uploaded.total_bytes,
        percent: 100,
        status: "completed",
        file_name: uploaded.file_name,
        source_path: uploaded.source_path,
        target_path: uploaded.uploaded_path,
      };
      return true;
    } catch (error) {
      errorMessage.value = error instanceof Error ? error.message : String(error);
      return false;
    } finally {
      uploading.value = false;
    }
  }

  async function startUploadProgressListening() {
    if (uploadUnlisten.value) return;
    uploadUnlisten.value = await listen<UploadProgressPayload>(
      "upload_progress",
      (event) => {
        uploadProgress.value = event.payload;
      },
    );
  }

  function stopUploadProgressListening() {
    if (uploadUnlisten.value) {
      uploadUnlisten.value();
      uploadUnlisten.value = null;
    }
  }

  async function runTask() {
    errorMessage.value = "";
    summary.value = null;

    if (uploading.value) {
      errorMessage.value = "文件上传中，请稍候";
      return;
    }

    if (!isAbsoluteXlsxPath(uploadedExcelPath.value)) {
      errorMessage.value = "请先上传有效的 .xlsx 文件";
      return;
    }
    if (!hasValidProfitRatio.value) {
      profitRatioError.value = "请输入有效利润比例后再执行任务";
      return;
    }
    const profitRatioPersisted = await persistProfitRatio();
    if (!profitRatioPersisted) {
      errorMessage.value = "利润比例保存失败，请修正后重试";
      return;
    }

    running.value = true;
    try {
      summary.value = await invoke<RunTaskSummary>(
        "run_task",
        buildRunTaskPayload(uploadedExcelPath.value, excelPath.value),
      );
    } catch (error) {
      errorMessage.value = error instanceof Error ? error.message : String(error);
    } finally {
      running.value = false;
    }
  }

  onMounted(() => {
    void startUploadProgressListening();
    void loadRunnerSettings();
  });

  onUnmounted(() => {
    stopUploadProgressListening();
    clearProfitRatioSaveTimer();
  });

  return {
    excelPath,
    uploadedExcelPath,
    running,
    uploading,
    uploadProgress,
    errorMessage,
    summary,
    profitRatioInput,
    profitRatioLoading,
    profitRatioSaving,
    profitRatioError,
    hasValidProfitRatio,
    browseExcelFile,
    setDroppedPath,
    updateProfitRatioInput,
    runTask,
  };
}
