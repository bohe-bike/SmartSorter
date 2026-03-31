<script setup lang="ts">
import { ref, computed, onUnmounted } from "vue";
import {
  scanDuplicates,
  deleteDuplicates,
  pickFolder,
  listenProgress,
} from "../utils/tauriApi";
import ProgressBar from "../components/ProgressBar.vue";
import type { DuplicateResult, DuplicateGroup, ProgressEvent } from "../types";

const sourcePaths = ref<string[]>([]);
const recursive = ref(true);
const scanning = ref(false);
const deleting = ref(false);
const result = ref<DuplicateResult | null>(null);

// 进度状态
const progress = ref({ current: 0, total: 0, currentFile: "", phase: "" });
let unlistenProgress: (() => void) | null = null;

const totalWastedMB = computed(() => {
  if (!result.value) return "0";
  return (result.value.total_wasted_bytes / 1048576).toFixed(1);
});

async function addFolder() {
  const path = await pickFolder();
  if (path && !sourcePaths.value.includes(path)) {
    sourcePaths.value.push(path);
  }
}

function removePath(index: number) {
  sourcePaths.value.splice(index, 1);
}

async function runScan() {
  if (sourcePaths.value.length === 0) return;
  scanning.value = true;
  progress.value = { current: 0, total: 0, currentFile: "", phase: "scanning" };

  // 监听后端进度事件
  unlistenProgress = await listenProgress((e: ProgressEvent) => {
    progress.value = {
      current: e.current,
      total: e.total,
      currentFile: e.current_file,
      phase: e.phase,
    };
  });

  try {
    result.value = await scanDuplicates(sourcePaths.value, recursive.value);
  } catch (e: any) {
    alert("扫描失败: " + e);
  } finally {
    scanning.value = false;
    if (unlistenProgress) {
      unlistenProgress();
      unlistenProgress = null;
    }
  }
}

onUnmounted(() => {
  if (unlistenProgress) {
    unlistenProgress();
    unlistenProgress = null;
  }
});

function toggleKeep(group: DuplicateGroup, fileIndex: number) {
  // 取消其他的 keep，设当前为 keep
  group.files.forEach((f, i) => {
    f.keep = i === fileIndex;
  });
}

function keepNewest(group: DuplicateGroup) {
  let newestIdx = 0;
  for (let i = 1; i < group.files.length; i++) {
    if (group.files[i].modified_at > group.files[newestIdx].modified_at) {
      newestIdx = i;
    }
  }
  toggleKeep(group, newestIdx);
}

function keepOldest(group: DuplicateGroup) {
  let oldestIdx = 0;
  for (let i = 1; i < group.files.length; i++) {
    if (group.files[i].modified_at < group.files[oldestIdx].modified_at) {
      oldestIdx = i;
    }
  }
  toggleKeep(group, oldestIdx);
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1048576) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1073741824) return `${(bytes / 1048576).toFixed(1)} MB`;
  return `${(bytes / 1073741824).toFixed(1)} GB`;
}

function formatDate(iso: string): string {
  if (!iso) return "-";
  try {
    return new Date(iso).toLocaleString("zh-CN");
  } catch {
    return iso;
  }
}

// 计算待删除文件路径
const pathsToDelete = computed(() => {
  if (!result.value) return [];
  const paths: string[] = [];
  for (const group of result.value.groups) {
    for (const file of group.files) {
      if (!file.keep) paths.push(file.path);
    }
  }
  return paths;
});

// Toast 通知
const toast = ref<{ visible: boolean; success: boolean; message: string }>({
  visible: false,
  success: true,
  message: "",
});
let toastTimer: ReturnType<typeof setTimeout> | null = null;

function showToast(success: boolean, message: string) {
  if (toastTimer) clearTimeout(toastTimer);
  toast.value = { visible: true, success, message };
  if (success) {
    toastTimer = setTimeout(() => {
      toast.value.visible = false;
    }, 4000);
  }
}

function dismissToast() {
  toast.value.visible = false;
  if (toastTimer) {
    clearTimeout(toastTimer);
    toastTimer = null;
  }
}

async function runDelete() {
  if (pathsToDelete.value.length === 0) return;
  const confirmMsg = `确定要删除 ${pathsToDelete.value.length} 个重复文件吗？此操作不可撤销。`;
  if (!confirm(confirmMsg)) return;

  deleting.value = true;
  progress.value = { current: 0, total: 0, currentFile: "", phase: "deleting" };

  unlistenProgress = await listenProgress((e: ProgressEvent) => {
    progress.value = {
      current: e.current,
      total: e.total,
      currentFile: e.current_file,
      phase: e.phase,
    };
  });

  try {
    const msg = await deleteDuplicates(pathsToDelete.value);
    showToast(true, msg);
    // 清除结果，提示用户重新扫描
    result.value = null;
  } catch (e: any) {
    showToast(false, "删除失败: " + e);
  } finally {
    deleting.value = false;
    if (unlistenProgress) {
      unlistenProgress();
      unlistenProgress = null;
    }
  }
}
</script>

<template>
  <div class="duplicate-view">
    <div class="header">
      <h2>重复文件检测</h2>
    </div>

    <!-- 数据源 -->
    <section class="source-section">
      <div class="source-header">
        <span class="label">扫描目录</span>
        <label class="recursive-toggle">
          <input type="checkbox" v-model="recursive" /> 递归子目录
        </label>
      </div>
      <div class="source-list">
        <div v-for="(p, i) in sourcePaths" :key="p" class="source-item">
          <span>📂 {{ p }}</span>
          <button class="btn-x" @click="removePath(i)">✕</button>
        </div>
        <button class="btn-add" @click="addFolder">+ 选择文件夹</button>
      </div>
      <button
        class="btn-scan"
        :disabled="sourcePaths.length === 0 || scanning"
        @click="runScan"
      >
        {{ scanning ? "扫描中…" : "🔍 开始扫描" }}
      </button>
      <ProgressBar
        v-if="scanning || deleting"
        :current="progress.current"
        :total="progress.total"
        :current-file="progress.currentFile"
        :phase="progress.phase"
      />
    </section>

    <!-- 结果 -->
    <section v-if="result" class="result-section">
      <div class="summary">
        <div class="stat">
          <span class="stat-val">{{ result.scanned_count }}</span>
          <span class="stat-lbl">扫描文件</span>
        </div>
        <div class="stat">
          <span class="stat-val highlight">{{ result.total_groups }}</span>
          <span class="stat-lbl">重复组</span>
        </div>
        <div class="stat">
          <span class="stat-val danger">{{ totalWastedMB }} MB</span>
          <span class="stat-lbl">可释放空间</span>
        </div>
        <button
          v-if="pathsToDelete.length > 0"
          class="btn-delete"
          :disabled="deleting"
          @click="runDelete"
        >
          {{
            deleting ? "删除中…" : `🗑 删除 ${pathsToDelete.length} 个重复文件`
          }}
        </button>
      </div>

      <div v-if="result.groups.length === 0" class="empty">
        未发现重复文件 🎉
      </div>

      <div v-else class="group-list">
        <div
          v-for="group in result.groups"
          :key="group.group_id"
          class="dup-group"
        >
          <div class="group-header">
            <span class="group-size">{{ formatSize(group.file_size) }}</span>
            <span class="group-count">{{ group.files.length }} 份</span>
            <span class="group-hash" :title="group.hash"
              >SHA256: {{ group.hash.substring(0, 12) }}…</span
            >
            <div class="group-actions">
              <button class="btn-sm" @click="keepNewest(group)">
                保留最新
              </button>
              <button class="btn-sm" @click="keepOldest(group)">
                保留最旧
              </button>
            </div>
          </div>
          <div class="file-list">
            <div
              v-for="(file, fi) in group.files"
              :key="file.path"
              class="file-row"
              :class="{ kept: file.keep, marked: !file.keep }"
            >
              <label class="keep-radio">
                <input
                  type="radio"
                  :name="group.group_id"
                  :checked="file.keep"
                  @change="toggleKeep(group, fi)"
                />
                保留
              </label>
              <span class="file-path">{{ file.path }}</span>
              <span class="file-date">{{ formatDate(file.modified_at) }}</span>
              <span v-if="!file.keep" class="delete-badge">待删除</span>
            </div>
          </div>
        </div>
      </div>
    </section>

    <div v-else class="placeholder">
      选择文件夹后点击「开始扫描」检测重复文件
    </div>

    <!-- Toast 通知 -->
    <Transition name="toast">
      <div
        v-if="toast.visible"
        class="toast"
        :class="toast.success ? 'toast-success' : 'toast-error'"
      >
        <span class="toast-icon">{{ toast.success ? "✅" : "❌" }}</span>
        <span class="toast-msg">{{ toast.message }}</span>
        <button class="toast-close" @click="dismissToast">✕</button>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.duplicate-view {
  padding: 16px;
  height: 100%;
  overflow-y: auto;
}

.header h2 {
  font-size: 18px;
  font-weight: 600;
  margin-bottom: 16px;
}

.source-section {
  background: var(--color-surface);
  border-radius: 8px;
  padding: 14px;
  margin-bottom: 12px;
}

.source-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 8px;
}

.label {
  font-size: 14px;
  font-weight: 600;
}

.recursive-toggle {
  font-size: 12px;
  color: var(--color-text-secondary);
  display: flex;
  align-items: center;
  gap: 4px;
  cursor: pointer;
}

.source-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-bottom: 10px;
}

.source-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 10px;
  background: var(--color-bg);
  border-radius: 4px;
  font-size: 13px;
}

.btn-x {
  background: none;
  border: none;
  cursor: pointer;
  color: var(--color-text-secondary);
}

.btn-add {
  border: 2px dashed var(--color-border);
  border-radius: 6px;
  padding: 8px;
  background: transparent;
  color: var(--color-text-secondary);
  cursor: pointer;
  font-size: 13px;
}

.btn-add:hover {
  border-color: var(--color-primary);
  color: var(--color-primary);
}

.btn-scan {
  width: 100%;
  height: 36px;
  border: none;
  border-radius: 6px;
  background: var(--color-primary);
  color: #fff;
  cursor: pointer;
  font-size: 14px;
  font-weight: 500;
}

.btn-scan:hover:not(:disabled) {
  background: var(--color-primary-hover);
}

.btn-scan:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* 结果 */
.result-section {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.summary {
  display: flex;
  gap: 24px;
  padding: 12px;
  background: var(--color-surface);
  border-radius: 8px;
}

.stat {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
}

.stat-val {
  font-size: 20px;
  font-weight: 700;
}

.stat-val.highlight {
  color: var(--color-primary);
}

.stat-val.danger {
  color: var(--color-danger);
}

.stat-lbl {
  font-size: 12px;
  color: var(--color-text-secondary);
}

.empty {
  text-align: center;
  padding: 48px;
  color: var(--color-success);
  font-size: 16px;
}

.group-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.dup-group {
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: 8px;
  padding: 12px;
}

.group-header {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 8px;
  font-size: 13px;
}

.group-size {
  font-weight: 700;
}

.group-count {
  color: var(--color-primary);
}

.group-hash {
  color: var(--color-text-secondary);
  font-size: 11px;
  font-family: monospace;
}

.group-actions {
  margin-left: auto;
  display: flex;
  gap: 6px;
}

.btn-sm {
  height: 26px;
  padding: 0 10px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  background: var(--color-bg);
  color: var(--color-text);
  cursor: pointer;
  font-size: 12px;
}

.btn-sm:hover {
  border-color: var(--color-primary);
  color: var(--color-primary);
}

.file-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.file-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 8px;
  border-radius: 4px;
  font-size: 12px;
}

.file-row.kept {
  background: rgba(56, 161, 105, 0.06);
}

.file-row.marked {
  background: rgba(229, 62, 62, 0.04);
  opacity: 0.7;
}

.keep-radio {
  display: flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
  cursor: pointer;
  font-size: 12px;
  color: var(--color-text-secondary);
}

.file-path {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.file-date {
  flex-shrink: 0;
  color: var(--color-text-secondary);
}

.delete-badge {
  flex-shrink: 0;
  font-size: 11px;
  padding: 1px 6px;
  border-radius: 3px;
  background: rgba(229, 62, 62, 0.1);
  color: var(--color-danger);
}

.placeholder {
  color: var(--color-text-secondary);
  text-align: center;
  padding: 64px;
  font-size: 13px;
}

.btn-delete {
  margin-left: auto;
  height: 36px;
  padding: 0 18px;
  border: none;
  border-radius: 6px;
  background: var(--color-danger);
  color: #fff;
  cursor: pointer;
  font-size: 13px;
  font-weight: 500;
  white-space: nowrap;
}

.btn-delete:hover:not(:disabled) {
  opacity: 0.9;
}

.btn-delete:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* Toast 通知 */
.toast {
  position: fixed;
  bottom: 20px;
  right: 20px;
  min-width: 300px;
  max-width: 480px;
  border-radius: 8px;
  padding: 12px 16px;
  box-shadow: 0 4px 20px rgba(0, 0, 0, 0.15);
  z-index: 9999;
  font-size: 13px;
  display: flex;
  align-items: center;
  gap: 8px;
}

.toast-success {
  background: var(--color-surface);
  border-left: 4px solid var(--color-success);
}

.toast-error {
  background: var(--color-surface);
  border-left: 4px solid var(--color-danger);
}

.toast-icon {
  font-size: 16px;
  flex-shrink: 0;
}

.toast-msg {
  flex: 1;
  font-weight: 500;
}

.toast-close {
  background: none;
  border: none;
  cursor: pointer;
  color: var(--color-text-secondary);
  font-size: 14px;
  padding: 0 2px;
}

.toast-close:hover {
  color: var(--color-text);
}

.toast-enter-active {
  animation: toast-in 0.3s ease;
}

.toast-leave-active {
  animation: toast-in 0.2s ease reverse;
}

@keyframes toast-in {
  from {
    opacity: 0;
    transform: translateY(16px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}
</style>
