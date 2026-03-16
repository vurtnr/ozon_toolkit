<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import {
  isResumeActionRequired,
  mapBlockingAlertTitle,
  type BlockingAlertModel,
} from "./blockingAlert";

const props = defineProps<{
  alert: BlockingAlertModel | null;
}>();

const emit = defineEmits<{
  dismiss: [];
}>();

const visible = computed(() => !!props.alert?.blocking);
const title = computed(() => mapBlockingAlertTitle(props.alert?.code || ""));
const canResume = computed(() => isResumeActionRequired(props.alert));
const resumePending = ref(false);
const resumeError = ref("");
const tone = computed(() => {
  if (props.alert?.code === "CHROME_NOT_FOUND") return "danger";
  if (props.alert?.code === "LOGIN_REQUIRED") return "info";
  if (props.alert?.code === "ANTI_BOT_CHALLENGE") return "warn";
  if (props.alert?.code === "RESUME_REQUIRED") return "warn";
  return "info";
});
const helperText = computed(() => {
  if (props.alert?.code === "CHROME_NOT_FOUND") {
    return "请先在设置面板中指定可执行的 Chrome 路径，再重新启动任务。";
  }
  if (props.alert?.code === "LOGIN_REQUIRED") {
    return "系统已经拉起本地 Chrome。请在该窗口内完成 1688 登录，登录态会复用到后续任务。";
  }
  if (props.alert?.code === "ANTI_BOT_CHALLENGE") {
    return "请远程接管当前机器，在 Chrome 中完成 1688 风控验证。验证结束后点击继续恢复串行任务。";
  }
  if (props.alert?.code === "RESUME_REQUIRED") {
    return "任务已经暂停。确认 Chrome 侧状态恢复正常后，点击继续从当前节点往下执行。";
  }
  return "当前任务被运行前置条件阻断，需要用户确认后才能继续。";
});

watch(
  () => props.alert,
  () => {
    resumePending.value = false;
    resumeError.value = "";
  },
);

async function handleResume() {
  resumePending.value = true;
  resumeError.value = "";
  try {
    await invoke("resume_after_challenge");
    emit("dismiss");
  } catch (error) {
    resumeError.value = error instanceof Error ? error.message : String(error);
  } finally {
    resumePending.value = false;
  }
}
</script>

<template>
  <teleport to="body">
    <div v-if="visible" class="alert-mask">
      <article class="alert-card panel-surface" :data-tone="tone">
        <div class="alert-head">
          <p class="alert-kicker">Execution Gate</p>
          <span class="alert-pill" :data-tone="tone">{{ title }}</span>
        </div>

        <div class="alert-copy">
          <h3>{{ title }}</h3>
          <p>{{ props.alert?.message }}</p>
          <small>{{ helperText }}</small>
        </div>

        <div class="action-row">
          <button
            v-if="canResume"
            type="button"
            class="primary-btn"
            :disabled="resumePending"
            @click="handleResume"
          >
            {{
              resumePending
                ? "恢复中..."
                : props.alert?.action_label || "已验证，继续执行"
            }}
          </button>
          <button
            type="button"
            class="secondary-btn"
            :disabled="resumePending"
            @click="emit('dismiss')"
          >
            {{ canResume ? "稍后处理" : "关闭" }}
          </button>
        </div>

        <p v-if="resumeError" class="feedback">{{ resumeError }}</p>
      </article>
    </div>
  </teleport>
</template>

<style scoped>
.alert-mask {
  position: fixed;
  inset: 0;
  padding: 1.5rem;
  background:
    radial-gradient(circle at top, rgba(73, 141, 255, 0.2), transparent 20rem),
    rgba(2, 7, 12, 0.72);
  backdrop-filter: blur(18px);
  display: grid;
  place-items: center;
  z-index: 1200;
}

.alert-card {
  width: min(100%, 42rem);
  display: grid;
  gap: 1rem;
}

.alert-card[data-tone="warn"] {
  border-color: rgba(255, 182, 92, 0.18);
}

.alert-card[data-tone="danger"] {
  border-color: rgba(255, 127, 150, 0.18);
}

.alert-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.8rem;
}

.alert-kicker {
  margin: 0;
  color: var(--text-dim);
  font-size: 0.76rem;
  letter-spacing: 0.18em;
  text-transform: uppercase;
}

.alert-pill {
  display: inline-flex;
  align-items: center;
  min-height: 2rem;
  padding: 0.35rem 0.75rem;
  border-radius: 999px;
  border: 1px solid rgba(255, 255, 255, 0.08);
  background: rgba(255, 255, 255, 0.04);
  color: var(--text-muted);
  font-size: 0.8rem;
}

.alert-pill[data-tone="info"] {
  border-color: rgba(111, 232, 255, 0.24);
  background: rgba(111, 232, 255, 0.12);
  color: var(--accent-cyan);
}

.alert-pill[data-tone="warn"] {
  border-color: rgba(255, 182, 92, 0.24);
  background: rgba(255, 182, 92, 0.12);
  color: var(--accent-amber);
}

.alert-pill[data-tone="danger"] {
  border-color: rgba(255, 127, 150, 0.24);
  background: rgba(255, 127, 150, 0.12);
  color: var(--danger);
}

.alert-copy {
  display: grid;
  gap: 0.5rem;
}

.alert-copy h3,
.alert-copy p,
.alert-copy small {
  margin: 0;
}

.alert-copy p,
.alert-copy small {
  color: var(--text-muted);
  line-height: 1.7;
}

.action-row {
  display: flex;
  justify-content: flex-start;
  gap: 0.75rem;
}

button {
  min-height: 3rem;
  padding: 0.8rem 1.1rem;
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

.feedback {
  margin: 0;
  color: var(--danger);
  font-size: 0.92rem;
}

@media (max-width: 720px) {
  .alert-head,
  .action-row {
    flex-direction: column;
    align-items: flex-start;
  }
}
</style>
