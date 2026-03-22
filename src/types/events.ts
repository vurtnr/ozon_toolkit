export interface ProgressEventPayload {
  processed: number;
  total: number;
}

export interface LogEventPayload {
  level: string;
  message: string;
}

export interface RowResultEventPayload {
  row_index: number;
  sku: string;
  stage: string;
  status: string;
  image_url: string | null;
  original_image_url: string | null;
  matched_image_url: string | null;
  item_url: string | null;
  price: string | null;
  elapsed_text: string | null;
  is_final: boolean;
}

export interface TaskDoneEventPayload {
  excel_path: string;
  status: string;
  processed_rows: number;
  total_rows: number;
  result_path: string | null;
}

export interface BlockingAlertEventPayload {
  code: string;
  message: string;
  blocking: boolean;
  action_label: string | null;
}

export interface TaskPhaseEventPayload {
  phase: string;
  label: string;
  detail: string;
  blocking: boolean;
}

export interface MonitorRow {
  rowIndex: number;
  sku: string;
  stage: string;
  status: string;
  imageUrl: string | null;
  originalImageUrl: string | null;
  matchedImageUrl: string | null;
  itemUrl: string | null;
  price: string | null;
  elapsedText: string | null;
  isFinal: boolean;
}

export interface MonitorState {
  rows: MonitorRow[];
  progress: ProgressEventPayload;
  logs: LogEventPayload[];
  done: TaskDoneEventPayload | null;
  alert: BlockingAlertEventPayload | null;
  taskPhase: TaskPhaseEventPayload | null;
}
