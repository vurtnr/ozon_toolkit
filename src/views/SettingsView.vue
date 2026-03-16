<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import {
  browseChromeExecutable,
  createDefaultSettings,
  getRuntimePlatform,
  loadSettings,
  saveSettings,
  type AppSettings,
  type RuntimePlatform,
} from "../stores/settings";

const settings = ref<AppSettings>(createDefaultSettings());
const runtimePlatform = ref<RuntimePlatform>("unknown");
const loading = ref(true);
const saving = ref(false);
const errorMessage = ref("");
const successMessage = ref("");

const platformLabel = computed(() => {
  if (runtimePlatform.value === "macos") return "macOS";
  if (runtimePlatform.value === "windows") return "Windows";
  if (runtimePlatform.value === "linux") return "Linux";
  return "Unknown";
});

const chromeStatusTone = computed(() =>
  settings.value.chromeExecutablePath.trim() ? "ready" : "warn",
);
const keyStatusTone = computed(() =>
  settings.value.dashscopeApiKey.trim() ? "ready" : "info",
);

async function fetchSettings() {
  loading.value = true;
  errorMessage.value = "";
  successMessage.value = "";
  try {
    runtimePlatform.value = await getRuntimePlatform();
    settings.value = await loadSettings();
  } catch (error) {
    errorMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    loading.value = false;
  }
}

async function handleBrowseChrome() {
  errorMessage.value = "";
  try {
    const selected = await browseChromeExecutable(runtimePlatform.value);
    if (selected) {
      settings.value.chromeExecutablePath = selected;
    }
  } catch (error) {
    errorMessage.value = error instanceof Error ? error.message : String(error);
  }
}

async function handleSaveSettings() {
  saving.value = true;
  errorMessage.value = "";
  successMessage.value = "";

  try {
    await saveSettings(settings.value);
    settings.value.dashscopeApiKey = "";
    successMessage.value = "设置已保存。本次会话环境变量已更新，API Key 不会落盘。";
  } catch (error) {
    errorMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    saving.value = false;
  }
}

onMounted(fetchSettings);
</script>

<template>
  <section class="settings-panel panel-surface">
    <header class="panel-header">
      <div>
        <p class="section-kicker">Environment Matrix</p>
        <h2 class="section-title">运行环境与密钥配置</h2>
      </div>
      <div class="meta-pills">
        <span class="status-pill" data-tone="info">{{ platformLabel }}</span>
        <span class="status-pill" :data-tone="chromeStatusTone">
          {{ settings.chromeExecutablePath.trim() ? "Chrome 路径已指定" : "Chrome 自动探测" }}
        </span>
        <span class="status-pill" :data-tone="keyStatusTone">
          {{ settings.dashscopeApiKey.trim() ? "本次会话已填入 Key" : "运行时检查 Key" }}
        </span>
      </div>
    </header>

    <p class="muted-copy">
      这里配置 DashScope API Key 与 Chrome 启动路径。Chrome 可留空自动探测，API Key 只注入当前运行时环境。
    </p>

    <div v-if="loading" class="state-box">
      <strong>正在读取本机配置...</strong>
      <small>桌面端会在启动时检查平台与本地设置。</small>
    </div>

    <form v-else class="settings-form" @submit.prevent="handleSaveSettings">
      <label class="field">
        <span class="field-label">DashScope API Key</span>
        <input
          id="dashscope-api-key"
          v-model="settings.dashscopeApiKey"
          type="password"
          autocomplete="off"
          placeholder="输入 DASHSCOPE_API_KEY"
        />
      </label>

      <label class="field">
        <span class="field-label">Chrome 浏览器路径</span>
        <div class="chrome-row">
          <input
            id="chrome-path"
            v-model="settings.chromeExecutablePath"
            type="text"
            placeholder="留空将自动探测"
          />
          <button type="button" class="secondary-btn" @click="handleBrowseChrome">浏览</button>
        </div>
      </label>

      <div class="action-row">
        <button class="primary-btn" type="button" :disabled="saving" @click="handleSaveSettings">
          {{ saving ? "写入中..." : "保存运行配置" }}
        </button>
      </div>

      <p v-if="errorMessage" class="feedback feedback--error">{{ errorMessage }}</p>
      <p v-if="successMessage" class="feedback feedback--success">{{ successMessage }}</p>
    </form>
  </section>
</template>

<style scoped>
.settings-panel {
  display: grid;
  gap: 1rem;
}

.panel-header {
  display: grid;
  gap: 0.9rem;
}

.meta-pills {
  display: flex;
  flex-wrap: wrap;
  gap: 0.6rem;
}

.state-box {
  display: grid;
  gap: 0.35rem;
  padding: 1rem;
  border-radius: var(--radius-lg);
  border: 1px solid rgba(111, 232, 255, 0.14);
  background: rgba(12, 23, 37, 0.82);
  color: var(--text-muted);
}

.settings-form {
  display: grid;
  gap: 0.95rem;
}

.field {
  display: grid;
  gap: 0.55rem;
}

.field-label {
  color: var(--text-muted);
  font-size: 0.84rem;
  letter-spacing: 0.04em;
  text-transform: uppercase;
}

input {
  width: 100%;
  min-height: 3rem;
  padding: 0.8rem 0.95rem;
  border-radius: var(--radius-md);
  border: 1px solid rgba(125, 169, 214, 0.2);
  background: rgba(4, 11, 18, 0.86);
  color: var(--text-primary);
  transition: border-color 0.2s ease, box-shadow 0.2s ease;
}

input:focus {
  outline: none;
  border-color: rgba(111, 232, 255, 0.45);
  box-shadow: 0 0 0 3px rgba(111, 232, 255, 0.12);
}

.chrome-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 0.65rem;
}

.action-row {
  display: flex;
  justify-content: flex-start;
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
  font-size: 0.92rem;
}

.feedback--error {
  color: var(--danger);
}

.feedback--success {
  color: var(--accent-green);
}
</style>
