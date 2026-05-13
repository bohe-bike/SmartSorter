<script setup lang="ts">
import { onMounted, ref, watch } from "vue";
import { getVersion } from "@tauri-apps/api/app";
import { check, type Update } from "@tauri-apps/plugin-updater";

type UpdateStatus =
  | "idle"
  | "checking"
  | "available"
  | "up-to-date"
  | "downloading"
  | "ready"
  | "error";

const updateStatus = ref<UpdateStatus>("idle");
const pendingUpdate = ref<Update | null>(null);
const updateVersion = ref("");
const updateNotes = ref("");
const downloadProgress = ref(0);
const updateError = ref("");

async function checkForUpdate() {
  updateStatus.value = "checking";
  updateError.value = "";
  try {
    const update = await check();
    if (update) {
      updateStatus.value = "available";
      updateVersion.value = update.version;
      updateNotes.value = update.body ?? "";
      pendingUpdate.value = update;
    } else {
      updateStatus.value = "up-to-date";
    }
  } catch (e) {
    updateStatus.value = "error";
    updateError.value = String(e);
  }
}

async function doInstall() {
  if (!pendingUpdate.value) return;
  updateStatus.value = "downloading";
  downloadProgress.value = 0;
  let downloaded = 0;
  let contentLength = 0;
  try {
    await pendingUpdate.value.downloadAndInstall((event) => {
      switch (event.event) {
        case "Started":
          contentLength = event.data.contentLength ?? 0;
          break;
        case "Progress":
          downloaded += event.data.chunkLength;
          if (contentLength > 0) {
            downloadProgress.value = Math.round(
              (downloaded / contentLength) * 100,
            );
          }
          break;
        case "Finished":
          downloadProgress.value = 100;
          break;
      }
    });
    updateStatus.value = "ready";
  } catch (e) {
    updateStatus.value = "error";
    updateError.value = String(e);
  }
}

const theme = ref<"system" | "light" | "dark">(
  (localStorage.getItem("ss-theme") as any) || "system",
);
const defaultConflict = ref(localStorage.getItem("ss-conflict") || "skip");
const confirmDelete = ref(
  localStorage.getItem("ss-confirm-delete") !== "false",
);
const appVersion = ref("-");

watch(theme, (v) => {
  localStorage.setItem("ss-theme", v);
  applyTheme(v);
});

watch(defaultConflict, (v) => {
  localStorage.setItem("ss-conflict", v);
});

watch(confirmDelete, (v) => {
  localStorage.setItem("ss-confirm-delete", String(v));
});

function applyTheme(t: string) {
  const root = document.documentElement;
  root.removeAttribute("data-theme");
  if (t === "light" || t === "dark") {
    root.setAttribute("data-theme", t);
  }
}

// 初始化
applyTheme(theme.value);

onMounted(async () => {
  try {
    appVersion.value = await getVersion();
  } catch {
    appVersion.value = "dev";
  }
});
</script>

<template>
  <div class="settings-view">
    <h2>设置</h2>

    <section class="settings-group">
      <h3>外观</h3>
      <div class="setting-row">
        <label>主题模式</label>
        <select v-model="theme">
          <option value="system">跟随系统</option>
          <option value="light">浅色</option>
          <option value="dark">深色</option>
        </select>
      </div>
    </section>

    <section class="settings-group">
      <h3>文件操作</h3>
      <div class="setting-row">
        <label>默认冲突策略</label>
        <select v-model="defaultConflict">
          <option value="skip">跳过</option>
          <option value="overwrite">覆盖</option>
          <option value="auto_rename">自动重命名</option>
        </select>
      </div>
      <div class="setting-row">
        <label>删除前确认</label>
        <label class="switch">
          <input type="checkbox" v-model="confirmDelete" />
          <span class="slider"></span>
        </label>
      </div>
    </section>

    <section class="settings-group">
      <h3>关于</h3>
      <div class="about-info">
        <p><strong>SmartSorter</strong> v{{ appVersion }}</p>
        <p>桌面端文件智能整理工具</p>
        <p class="tech">Tauri 2 + Vue 3 + Rust</p>
      </div>
      <div class="update-section">
        <div v-if="updateStatus === 'idle'" class="setting-row">
          <label>软件更新</label>
          <button class="btn-check" @click="checkForUpdate">检查更新</button>
        </div>
        <div v-else-if="updateStatus === 'checking'" class="update-status">
          <span class="status-text">正在检查更新...</span>
        </div>
        <div v-else-if="updateStatus === 'up-to-date'" class="update-status">
          <span class="status-text status-ok">已是最新版本 ✓</span>
          <button class="btn-secondary" @click="updateStatus = 'idle'">
            关闭
          </button>
        </div>
        <div v-else-if="updateStatus === 'available'" class="update-available">
          <div class="update-info">
            <span class="update-version">发现新版本 v{{ updateVersion }}</span>
            <p v-if="updateNotes" class="update-notes">{{ updateNotes }}</p>
          </div>
          <div class="update-actions">
            <button class="btn-secondary" @click="updateStatus = 'idle'">
              暂不更新
            </button>
            <button class="btn-primary" @click="doInstall">下载并安装</button>
          </div>
        </div>
        <div v-else-if="updateStatus === 'downloading'" class="update-status">
          <span class="status-text">正在下载... {{ downloadProgress }}%</span>
          <div class="update-progress-bar">
            <div
              class="update-progress-fill"
              :style="{ width: downloadProgress + '%' }"
            ></div>
          </div>
        </div>
        <div v-else-if="updateStatus === 'ready'" class="update-status">
          <span class="status-text status-ok">安装完成，应用将自动重启 ✓</span>
        </div>
        <div v-else-if="updateStatus === 'error'" class="update-status">
          <span class="status-text status-error">{{ updateError }}</span>
          <button class="btn-secondary" @click="updateStatus = 'idle'">
            重试
          </button>
        </div>
      </div>
    </section>
  </div>
</template>

<style scoped>
.settings-view {
  padding: 16px;
  max-width: 600px;
}

.settings-view h2 {
  font-size: 18px;
  font-weight: 600;
  margin-bottom: 20px;
}

.settings-group {
  background: var(--color-surface);
  border-radius: 8px;
  padding: 16px;
  margin-bottom: 12px;
}

.settings-group h3 {
  font-size: 13px;
  font-weight: 600;
  color: var(--color-text-secondary);
  text-transform: uppercase;
  letter-spacing: 1px;
  margin-bottom: 12px;
}

.setting-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 0;
  border-bottom: 1px solid var(--color-border);
}

.setting-row:last-child {
  border-bottom: none;
}

.setting-row label {
  font-size: 14px;
}

.setting-row select {
  height: 32px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  padding: 0 10px;
  font-size: 13px;
  background: var(--color-bg);
  color: var(--color-text);
  min-width: 140px;
}

/* Toggle switch */
.switch {
  position: relative;
  display: inline-block;
  width: 42px;
  height: 24px;
}

.switch input {
  opacity: 0;
  width: 0;
  height: 0;
}

.slider {
  position: absolute;
  cursor: pointer;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background-color: var(--color-border);
  border-radius: 24px;
  transition: 0.2s;
}

.slider::before {
  content: "";
  position: absolute;
  height: 18px;
  width: 18px;
  left: 3px;
  bottom: 3px;
  background-color: white;
  border-radius: 50%;
  transition: 0.2s;
}

.switch input:checked + .slider {
  background-color: var(--color-primary);
}

.switch input:checked + .slider::before {
  transform: translateX(18px);
}

.about-info {
  font-size: 13px;
  line-height: 1.8;
}

.about-info .tech {
  color: var(--color-text-secondary);
  font-size: 12px;
}

.update-section {
  margin-top: 12px;
  border-top: 1px solid var(--color-border);
  padding-top: 12px;
}

.btn-check,
.btn-primary {
  padding: 6px 14px;
  border-radius: 4px;
  border: none;
  cursor: pointer;
  font-size: 13px;
  background: var(--color-primary);
  color: #fff;
}

.btn-secondary {
  padding: 6px 14px;
  border-radius: 4px;
  border: 1px solid var(--color-border);
  cursor: pointer;
  font-size: 13px;
  background: transparent;
  color: var(--color-text);
}

.update-status {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 0;
  flex-wrap: wrap;
}

.status-text {
  font-size: 13px;
  flex: 1;
}

.status-ok {
  color: var(--color-success, #22c55e);
}

.status-error {
  color: var(--color-danger, #ef4444);
}

.update-available {
  padding: 8px 0;
}

.update-version {
  font-size: 13px;
  font-weight: 600;
  color: var(--color-primary);
}

.update-notes {
  font-size: 12px;
  color: var(--color-text-secondary);
  margin: 4px 0 8px;
  white-space: pre-wrap;
}

.update-actions {
  display: flex;
  gap: 8px;
  justify-content: flex-end;
}

.update-progress-bar {
  width: 100%;
  height: 6px;
  background: var(--color-border);
  border-radius: 3px;
  overflow: hidden;
}

.update-progress-fill {
  height: 100%;
  background: var(--color-primary);
  border-radius: 3px;
  transition: width 0.2s;
}
</style>
