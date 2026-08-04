<script setup lang="ts">
import { computed, onUnmounted, ref } from "vue";
import ProgressBar from "../components/ProgressBar.vue";
import {
  executeMediaTagCleanup,
  listenProgress,
  pickFolder,
  scanMediaTagCleanup,
} from "../utils/tauriApi";
import type {
  AuthorFolderMode,
  MediaTagCleanupFile,
  MediaTagCleanupResult,
  ProgressEvent,
  TagCleanupMode,
} from "../types";

const sourcePaths = ref<string[]>([]);
const recursive = ref(true);
const authorFolderMode = ref<AuthorFolderMode | null>(null);
const cleanupMode = ref<TagCleanupMode>("creator_only");
const verifyContentHash = ref(false);
const scanning = ref(false);
const executing = ref(false);
const result = ref<MediaTagCleanupResult | null>(null);
const message = ref("");
const progress = ref({ current: 0, total: 0, currentFile: "", phase: "" });

let unlistenProgress: (() => void) | null = null;

const selectedFiles = computed(() =>
  (result.value?.files ?? []).filter(
    (file) =>
      file.checked && file.supported && Boolean(file.target_artist?.trim()),
  ),
);

const unsupportedCount = computed(
  () => (result.value?.files ?? []).filter((file) => !file.supported).length,
);

const unresolvedCount = computed(
  () =>
    (result.value?.files ?? []).filter(
      (file) => file.supported && !file.target_artist?.trim(),
    ).length,
);

const authorFolderModeLabel = computed(() =>
  authorFolderMode.value === "children"
    ? "所选目录的下一级文件夹是作者"
    : authorFolderMode.value === "selected"
      ? "所选目录本身就是作者文件夹"
      : "尚未选择作者目录层级",
);

const verificationModeLabel = computed(() =>
  verifyContentHash.value ? "严格 SHA-256" : "快速路径与大小校验",
);

async function addFolder() {
  const path = await pickFolder();
  if (path && !sourcePaths.value.includes(path)) {
    sourcePaths.value.push(path);
    result.value = null;
    message.value = "";
  }
}

function removePath(index: number) {
  sourcePaths.value.splice(index, 1);
  result.value = null;
  message.value = "";
}

function invalidateScanResult() {
  result.value = null;
  message.value = "";
}

function resetProgress(phase: string) {
  progress.value = { current: 0, total: 0, currentFile: "", phase };
}

async function prepareProgressListener() {
  cleanupProgress();
  unlistenProgress = await listenProgress((event: ProgressEvent) => {
    progress.value = {
      current: event.current,
      total: event.total,
      currentFile: event.current_file,
      phase: event.phase,
    };
  });
}

function cleanupProgress() {
  if (unlistenProgress) {
    unlistenProgress();
    unlistenProgress = null;
  }
}

async function scanFiles() {
  const folderMode = authorFolderMode.value;
  if (scanning.value || sourcePaths.value.length === 0 || !folderMode) return;

  scanning.value = true;
  message.value = "";
  result.value = null;
  resetProgress("collecting");
  try {
    await prepareProgressListener();
    result.value = await scanMediaTagCleanup(
      sourcePaths.value,
      recursive.value,
      folderMode,
      cleanupMode.value,
      verifyContentHash.value,
    );
  } catch (error) {
    message.value = "扫描失败: " + String(error);
  } finally {
    scanning.value = false;
    cleanupProgress();
  }
}

function updateArtist(file: MediaTagCleanupFile, value: string) {
  file.target_artist = value;
  if (file.supported) {
    file.checked = Boolean(value.trim());
  }
}

function selectAllReady(checked: boolean) {
  for (const file of result.value?.files ?? []) {
    if (file.supported && file.target_artist?.trim()) {
      file.checked = checked;
    }
  }
}

function fileStatus(file: MediaTagCleanupFile): string {
  if (!file.supported) return file.skip_reason ?? "格式不支持";
  if (!file.target_artist?.trim()) return "请填写作者";
  return "可清洗";
}

async function executeCleanup() {
  if (!result.value || selectedFiles.value.length === 0 || executing.value) return;
  if (
    !confirm(
      `目录模式：${authorFolderModeLabel.value}\n内容校验：${verificationModeLabel.value}\n\n将清洗 ${selectedFiles.value.length} 个文件，并写入 Artist 和 AlbumArtist。系统会先创建完整原文件备份，完成后可在历史记录中撤销；备份会占用额外磁盘空间。是否继续？`,
    )
  ) {
    return;
  }

  executing.value = true;
  message.value = "";
  resetProgress("executing");
  try {
    await prepareProgressListener();
    const authorAssignments = Object.fromEntries(
      selectedFiles.value.map((file) => [file.path, file.target_artist?.trim() ?? ""]),
    );
    message.value = await executeMediaTagCleanup({
      task_id: result.value.task_id,
      selected_paths: selectedFiles.value.map((file) => file.path),
      author_assignments: authorAssignments,
    });
    result.value = null;
  } catch (error) {
    message.value = "执行失败: " + String(error);
  } finally {
    executing.value = false;
    cleanupProgress();
  }
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1048576) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1073741824) return `${(bytes / 1048576).toFixed(1)} MB`;
  return `${(bytes / 1073741824).toFixed(1)} GB`;
}

function mediaIcon(type: string): string {
  return type === "video" ? "🎬" : "🎧";
}

onUnmounted(cleanupProgress);
</script>

<template>
  <div class="tag-cleanup-view">
    <div class="header">
      <div>
        <h2>媒体标签清洗</h2>
        <p>按整理后的作者目录写入标准 Artist 与 AlbumArtist</p>
      </div>
    </div>

    <section class="source-section">
      <div class="section-head">
        <span class="label">扫描目录</span>
        <label class="recursive-toggle">
          <input v-model="recursive" type="checkbox" @change="invalidateScanResult" /> 递归子目录
        </label>
      </div>
      <div class="source-list">
        <div v-for="(path, index) in sourcePaths" :key="path" class="source-item">
          <span class="source-text">📂 {{ path }}</span>
          <button class="btn-x" :title="`移除 ${path}`" @click="removePath(index)">×</button>
        </div>
        <button class="btn-add" @click="addFolder">+ 选择文件夹</button>
      </div>
      <div class="folder-mode-row">
        <span class="label">作者目录层级</span>
        <div class="folder-mode-control">
          <label :class="{ active: authorFolderMode === 'children' }">
            <input v-model="authorFolderMode" type="radio" value="children" @change="invalidateScanResult" />
            下一级是作者
          </label>
          <label :class="{ active: authorFolderMode === 'selected' }">
            <input v-model="authorFolderMode" type="radio" value="selected" @change="invalidateScanResult" />
            所选目录是作者
          </label>
        </div>
      </div>
      <div class="folder-mode-row">
        <span class="label">清洗模式</span>
        <div class="folder-mode-control">
          <label :class="{ active: cleanupMode === 'creator_only' }">
            <input v-model="cleanupMode" type="radio" value="creator_only" @change="invalidateScanResult" />
            仅修复创作者（推荐）
          </label>
          <label :class="{ active: cleanupMode === 'full' }">
            <input v-model="cleanupMode" type="radio" value="full" @change="invalidateScanResult" />
            完整清洗标签
          </label>
        </div>
      </div>
      <div class="scan-note">
        仅修复创作者会保留标题、专辑、曲目号、作曲者、封面等现有标签；完整清洗会删除这些可写描述标签，仅保留封面并写入 Artist / AlbumArtist。损坏或无法安全写入的标签会标为只读不可清洗。
      </div>
      <div class="folder-mode-row">
        <span class="label">内容校验</span>
        <div class="folder-mode-control">
          <label :class="{ active: !verifyContentHash }">
            <input v-model="verifyContentHash" type="radio" :value="false" @change="invalidateScanResult" />
            快速：路径 + 大小
          </label>
          <label :class="{ active: verifyContentHash }">
            <input v-model="verifyContentHash" type="radio" :value="true" @change="invalidateScanResult" />
            严格：SHA-256
          </label>
        </div>
      </div>
      <div class="scan-note">
        当前模式：{{ authorFolderModeLabel }}。无法从目录推断时才使用已有 Artist 标签。支持 MP3、FLAC、M4A、OGG、WAV、MP4、M4V。
      </div>
      <button class="btn-scan" :disabled="scanning || sourcePaths.length === 0 || !authorFolderMode" @click="scanFiles">
        {{ scanning ? "扫描中…" : result ? "重新扫描" : "扫描媒体标签" }}
      </button>
      <ProgressBar
        v-if="scanning || executing"
        :current="progress.current"
        :total="progress.total"
        :current-file="progress.currentFile"
        :phase="progress.phase"
      />
    </section>

    <div v-if="message" class="message-box" :class="message.startsWith('执行失败') || message.startsWith('扫描失败') ? 'error-msg' : 'success-msg'">
      {{ message }}
    </div>

    <section v-if="result" class="result-section">
      <div class="summary-row">
        <span><strong>{{ result.scanned_count }}</strong> 个音视频文件</span>
        <span><strong>{{ selectedFiles.length }}</strong> 个待清洗</span>
        <span v-if="unresolvedCount > 0" class="warning"><strong>{{ unresolvedCount }}</strong> 个待填写作者</span>
        <span v-if="unsupportedCount > 0" class="muted"><strong>{{ unsupportedCount }}</strong> 个格式暂不支持</span>
      </div>

      <div class="warning-note">
        清洗会删除可写描述标签，仅保留内嵌封面，并写入 Artist / AlbumArtist。每个文件都会创建完整备份，可从历史记录撤销。
      </div>

      <div class="table-toolbar">
        <span class="label">清洗预览</span>
        <div class="table-actions">
          <button class="btn-sm" @click="selectAllReady(true)">全选可清洗项</button>
          <button class="btn-sm" @click="selectAllReady(false)">清空选择</button>
          <button class="btn-execute" :disabled="executing || selectedFiles.length === 0" @click="executeCleanup">
            {{ executing ? "清洗中…" : `执行清洗（${selectedFiles.length}）` }}
          </button>
        </div>
      </div>

      <div class="file-list">
        <div v-for="file in result.files" :key="file.path" class="file-row" :class="{ skipped: !file.supported || !file.target_artist?.trim() }">
          <input v-model="file.checked" type="checkbox" :disabled="!file.supported || !file.target_artist?.trim()" />
          <span class="file-type">{{ mediaIcon(file.media_type) }}</span>
          <div class="file-main">
            <div class="file-name">{{ file.file_name }}</div>
            <div class="file-path" :title="file.path">{{ file.path }} · {{ formatSize(file.size_bytes) }}</div>
          </div>
          <div class="tag-value" :title="file.current_artist ?? '未设置'">
            <span class="value-label">当前</span>{{ file.current_artist || "未设置" }}
          </div>
          <label class="target-artist">
            <span class="value-label">写入</span>
            <input
              :value="file.target_artist ?? ''"
              :disabled="!file.supported"
              placeholder="作者名称"
              @input="updateArtist(file, ($event.target as HTMLInputElement).value)"
            />
          </label>
          <div class="file-status" :class="{ warning: !file.supported || !file.target_artist?.trim() }" :title="file.skip_reason ?? file.author_source">
            {{ file.supported && file.target_artist?.trim() ? file.author_source : fileStatus(file) }}
          </div>
        </div>
      </div>
    </section>

    <div v-else-if="!scanning && !message" class="placeholder">
      选择已整理的媒体目录后开始扫描
    </div>
  </div>
</template>

<style scoped>
.tag-cleanup-view {
  min-height: 100%;
  padding: 16px;
}

.header {
  margin-bottom: 16px;
}

.header h2 {
  font-size: 18px;
  font-weight: 600;
}

.header p,
.scan-note,
.muted,
.file-path,
.value-label {
  color: var(--color-text-secondary);
  font-size: 12px;
}

.header p {
  margin-top: 2px;
}

.source-section,
.result-section {
  border: 1px solid var(--color-border);
  background: var(--color-surface);
  border-radius: 8px;
  padding: 14px;
}

.section-head,
.table-toolbar,
.table-actions,
.source-item,
.file-row,
.summary-row {
  display: flex;
  align-items: center;
}

.section-head,
.table-toolbar {
  justify-content: space-between;
}

.label {
  font-weight: 600;
}

.recursive-toggle,
.source-item,
.file-row {
  gap: 8px;
}

.source-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-top: 10px;
}

.folder-mode-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-top: 10px;
}

.folder-mode-control {
  display: flex;
  min-height: 32px;
  border: 1px solid var(--color-border);
  border-radius: 6px;
  overflow: hidden;
}

.folder-mode-control label {
  display: flex;
  align-items: center;
  cursor: pointer;
  padding: 0 10px;
  color: var(--color-text-secondary);
  font-size: 12px;
}

.folder-mode-control label + label {
  border-left: 1px solid var(--color-border);
}

.folder-mode-control label.active {
  background: var(--color-primary);
  color: #fff;
}

.folder-mode-control input {
  position: absolute;
  opacity: 0;
  pointer-events: none;
}

.source-item {
  min-height: 32px;
  border: 1px solid var(--color-border);
  background: var(--color-bg);
  padding: 5px 8px;
}

.source-text,
.file-main {
  min-width: 0;
  flex: 1;
}

.source-text,
.file-path,
.file-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.btn-x,
.btn-add,
.btn-scan,
.btn-sm,
.btn-execute {
  cursor: pointer;
}

.btn-x {
  border: 0;
  background: transparent;
  color: var(--color-text-secondary);
  font-size: 18px;
}

.btn-add {
  min-height: 36px;
  border: 1px dashed var(--color-border);
  background: transparent;
  color: var(--color-text-secondary);
  border-radius: 6px;
}

.scan-note {
  margin-top: 10px;
  line-height: 1.6;
}

.btn-scan,
.btn-execute {
  min-height: 36px;
  border: 0;
  border-radius: 6px;
  background: var(--color-primary);
  color: #fff;
  padding: 0 16px;
}

.btn-scan {
  width: 100%;
  margin-top: 12px;
}

.btn-scan:disabled,
.btn-execute:disabled {
  cursor: not-allowed;
  opacity: 0.5;
}

.message-box,
.warning-note {
  margin-top: 12px;
  border-radius: 6px;
  padding: 10px 12px;
  font-size: 12px;
}

.message-box {
  background: var(--color-surface);
  border: 1px solid var(--color-border);
}

.success-msg {
  border-left: 3px solid var(--color-success);
}

.error-msg {
  border-left: 3px solid var(--color-danger);
  color: var(--color-danger);
}

.result-section {
  margin-top: 12px;
}

.summary-row {
  flex-wrap: wrap;
  gap: 16px;
  font-size: 13px;
}

.summary-row strong {
  color: var(--color-primary);
}

.warning {
  color: var(--color-warning);
}

.warning-note {
  background: rgba(214, 158, 46, 0.08);
  border: 1px solid var(--color-warning);
  color: var(--color-warning);
}

.table-toolbar {
  gap: 12px;
  margin-top: 14px;
}

.table-actions {
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 8px;
}

.btn-sm {
  min-height: 30px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  background: var(--color-bg);
  color: var(--color-text-secondary);
  padding: 0 10px;
  font-size: 12px;
}

.file-list {
  margin-top: 10px;
  border-top: 1px solid var(--color-border);
}

.file-row {
  min-height: 58px;
  border-bottom: 1px solid var(--color-border);
  padding: 8px 2px;
}

.file-row.skipped {
  opacity: 0.68;
}

.file-type {
  width: 20px;
  flex: 0 0 auto;
}

.file-name {
  font-size: 13px;
}

.tag-value,
.target-artist,
.file-status {
  width: 150px;
  min-width: 0;
  font-size: 12px;
}

.tag-value {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.value-label {
  display: block;
  margin-bottom: 2px;
}

.target-artist input {
  width: 100%;
  height: 28px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  background: var(--color-bg);
  color: var(--color-text);
  padding: 0 7px;
  font-size: 12px;
}

.file-status {
  color: var(--color-success);
}

.file-status.warning {
  color: var(--color-warning);
}

.placeholder {
  padding: 64px;
  color: var(--color-text-secondary);
  text-align: center;
}

@media (max-width: 920px) {
  .folder-mode-row {
    align-items: stretch;
    flex-direction: column;
  }

  .folder-mode-control label {
    flex: 1;
    justify-content: center;
  }

  .file-row {
    align-items: flex-start;
    flex-wrap: wrap;
  }

  .file-main {
    flex-basis: calc(100% - 56px);
  }

  .tag-value,
  .target-artist,
  .file-status {
    width: calc(33.33% - 6px);
  }
}
</style>
