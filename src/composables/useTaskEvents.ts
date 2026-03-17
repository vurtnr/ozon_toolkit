import { onMounted, onUnmounted, reactive } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  BlockingAlertEventPayload,
  LogEventPayload,
  MonitorRow,
  MonitorState,
  ProgressEventPayload,
  RowResultEventPayload,
  TaskDoneEventPayload,
} from "../types/events";

export function createEmptyMonitor(): MonitorState {
  return {
    rows: [],
    progress: { processed: 0, total: 0 },
    logs: [],
    done: null,
    alert: null,
  };
}

export function appendRowResult(
  state: MonitorState,
  payload: RowResultEventPayload,
): void {
  const existingRow = state.rows.find((row) => row.rowIndex === payload.row_index);
  const nextRow: MonitorRow = {
    rowIndex: payload.row_index,
    sku: payload.sku,
    stage: payload.stage,
    status: payload.status,
    imageUrl: payload.matched_image_url ?? payload.image_url ?? existingRow?.imageUrl ?? null,
    originalImageUrl:
      payload.original_image_url ?? existingRow?.originalImageUrl ?? null,
    matchedImageUrl:
      payload.matched_image_url ?? payload.image_url ?? existingRow?.matchedImageUrl ?? null,
    itemUrl: payload.item_url ?? existingRow?.itemUrl ?? null,
    price: payload.price ?? existingRow?.price ?? null,
    elapsedText: payload.elapsed_text ?? existingRow?.elapsedText ?? null,
    isFinal: payload.is_final,
  };
  const existingIndex = state.rows.findIndex(
    (row) => row.rowIndex === payload.row_index,
  );
  if (existingIndex >= 0) {
    state.rows.splice(existingIndex, 1, nextRow);
  } else {
    state.rows.push(nextRow);
    state.rows.sort((left, right) => left.rowIndex - right.rowIndex);
  }
}

export function updateProgress(
  state: MonitorState,
  payload: ProgressEventPayload,
): void {
  if (payload.processed === 0) {
    state.rows = [];
    state.logs = [];
    state.done = null;
    state.alert = null;
  }
  state.progress.processed = payload.processed;
  state.progress.total = payload.total;
}

export function appendLog(state: MonitorState, payload: LogEventPayload): void {
  state.logs.push(payload);
}

export function markTaskDone(
  state: MonitorState,
  payload: TaskDoneEventPayload,
): void {
  state.done = payload;
}

export function setBlockingAlert(
  state: MonitorState,
  payload: BlockingAlertEventPayload,
): void {
  state.alert = payload;
}

export function useTaskEvents() {
  const state = reactive(createEmptyMonitor());
  const unlisten: UnlistenFn[] = [];

  async function startListening() {
    if (unlisten.length > 0) return;

    unlisten.push(
      await listen<ProgressEventPayload>("progress", (event) => {
        updateProgress(state, event.payload);
      }),
    );

    unlisten.push(
      await listen<RowResultEventPayload>("row_result", (event) => {
        appendRowResult(state, event.payload);
      }),
    );

    unlisten.push(
      await listen<LogEventPayload>("log", (event) => {
        appendLog(state, event.payload);
      }),
    );

    unlisten.push(
      await listen<TaskDoneEventPayload>("task_done", (event) => {
        markTaskDone(state, event.payload);
      }),
    );

    unlisten.push(
      await listen<BlockingAlertEventPayload>("blocking_alert", (event) => {
        setBlockingAlert(state, event.payload);
      }),
    );
  }

  function stopListening() {
    while (unlisten.length > 0) {
      const off = unlisten.pop();
      if (off) off();
    }
  }

  function reset() {
    state.rows.splice(0, state.rows.length);
    state.logs.splice(0, state.logs.length);
    state.done = null;
    state.alert = null;
    state.progress.processed = 0;
    state.progress.total = 0;
  }

  onMounted(() => {
    void startListening();
  });

  onUnmounted(() => {
    stopListening();
  });

  return {
    state,
    startListening,
    stopListening,
    reset,
  };
}
