<script setup lang="ts">
import { computed, onUnmounted, ref } from "vue";
import ProgressBar from "../components/ProgressBar.vue";
import {
  executeMediaClassify,
  listenProgress,
  pickFolder,
  previewMediaClassify,
  scanMediaAuthors,
} from "../utils/tauriApi";
import type {
  AuthorGroup,
  ClassifyAction,
  ClassifyPreviewResult,
  MediaClassifyResult,
  ProgressEvent,
} from "../types";

const sourcePaths = ref<string[]>([]);
const recursive = ref(true);
const mediaTypeOptions = ref([
  { key: "image", label: "图片", checked: true },
  { key: "audio", label: "音频", checked: true },
  { key: "video", label: "视频", checked: true },
  { key: "ebook", label: "电子书", checked: true },
]);
const action = ref<ClassifyAction>("move_to_author_folder");
const renameTemplate = ref("{author} - {filename}");
const scanning = ref(false);
const executing = ref(false);
const result = ref<MediaClassifyResult | null>(null);
const preview = ref<ClassifyPreviewResult | null>(null);
const executionMessage = ref("");
const progress = ref({ current: 0, total: 0, currentFile: "", phase: "" });

let unlistenProgress: (() => void) | null = null;

const selectedMediaTypes = computed(() =>
  mediaTypeOptions.value.filter((item) => item.checked).map((item) => item.key),
);

const checkedPaths = computed(() => {
  if (!result.value) return [] as string[];
  return result.value.groups.flatMap((group) =>
    group.files.filter((file) => file.checked).map((file) => file.path),
  );
});

const totalSelected = computed(() => checkedPaths.value.length);

const totalSelectedSize = computed(() => {
  if (!result.value) return 0;
  return result.value.groups.reduce((sum, group) => {
    return (
      sum +
      group.files
        .filter((file) => file.checked)
        .reduce((groupSum, file) => groupSum + file.size_bytes, 0)
    );
  }, 0);
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

function resetProgress(phase: string) {
  progress.value = { current: 0, total: 0, currentFile: "", phase };
}

async function prepareProgressListener() {
  if (unlistenProgress) {
    unlistenProgress();
    unlistenProgress = null;
  }

  unlistenProgress = await listenProgress((event: ProgressEvent) => {
    progress.value = {
      current: event.current,
      total: event.total,
      currentFile: event.current_file,
      phase: event.phase,
    };
  });
}

async function runScan() {
  if (sourcePaths.value.length === 0 || selectedMediaTypes.value.length === 0) {
    return;
  }

  scanning.value = true;
  preview.value = null;
  executionMessage.value = "";
  resetProgress("extracting");
  await prepareProgressListener();

  try {
    result.value = await scanMediaAuthors(
      sourcePaths.value,
      recursive.value,
      selectedMediaTypes.value,
    );
  } catch (error) {
    alert("扫描失败: " + error);
  } finally {
    scanning.value = false;
    cleanupProgress();
  }
}

function toggleGroup(group: AuthorGroup, checked: boolean) {
  group.files.forEach((file) => {
    file.checked = checked;
  });
}

function groupCheckedCount(group: AuthorGroup): number {
  return group.files.filter((file) => file.checked).length;
}

async function generatePreview() {
  if (!result.value || checkedPaths.value.length === 0) {
    return;
  }

  try {
    preview.value = await previewMediaClassify({
      task_id: result.value.task_id,
      action: action.value,
      rename_template: action.value === "rename" ? renameTemplate.value : null,
      checked_paths: checkedPaths.value,
    });
    executionMessage.value = "";
  } catch (error) {
    alert("生成预览失败: " + error);
  }
}

async function executeChanges() {
  if (!preview.value) {
    return;
  }

  executing.value = true;
  resetProgress("executing");
  await prepareProgressListener();

  try {
    executionMessage.value = await executeMediaClassify(preview.value.task_id);
    preview.value = null;
    result.value = null;
    sourcePaths.value = [];
  } catch (error) {
    executionMessage.value = String(error);
    alert("执行失败: " + error);
  } finally {
    executing.value = false;
    cleanupProgress();
  }
}

function cleanupProgress() {
  if (unlistenProgress) {
    unlistenProgress();
    unlistenProgress = null;
  }
}

onUnmounted(() => {
  cleanupProgress();
});

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

function mediaIcon(type: string): string {
  if (type === "image") return "🖼";
  if (type === "audio") return "🎧";
  if (type === "video") return "🎬";
  if (type === "ebook") return "📚";
  return "📄";
}
</script>

<template>
  <div class="media-classify-view">
    <div class="header">
      <h2>媒体作者归类</h2>
      <span class="header-tip">按作者批量归类并支持预览后执行</span>
    </div>

    <section class="source-section">
      <div class="section-head">
        <span class="label">扫描目录</span>
        <label class="recursive-toggle">
          <input v-model="recursive" type="checkbox" /> 递归子目录
        </label>
      </div>

      <div class="source-list">
        <div
          v-for="(path, index) in sourcePaths"
          :key="path"
          class="source-item"
        >
          <span class="source-text">📂 {{ path }}</span>
          <button class="btn-x" @click="removePath(index)">✕</button>
        </div>
        <button class="btn-add" @click="addFolder">+ 选择文件夹</button>
      </div>

      <div class="filter-row">
        <span class="filter-label">媒体类型</span>
        <label
          v-for="item in mediaTypeOptions"
          :key="item.key"
          class="filter-chip"
        >
          <input v-model="item.checked" type="checkbox" /> {{ item.label }}
        </label>
      </div>

      <button
        class="btn-scan"
        :disabled="
          sourcePaths.length === 0 ||
          scanning ||
          selectedMediaTypes.length === 0
        "
        @click="runScan"
      >
        {{ scanning ? "扫描中…" : "开始扫描作者" }}
      </button>

      <ProgressBar
        v-if="scanning || executing"
        :current="progress.current"
        :total="progress.total"
        :current-file="progress.currentFile"
        :phase="progress.phase"
      />
    </section>

    <section v-if="result" class="result-section">
      <div class="summary-grid">
        <div class="stat-card">
          <span class="stat-value">{{ result.scanned_count }}</span>
          <span class="stat-label">扫描文件</span>
        </div>
        <div class="stat-card">
          <span class="stat-value highlight">{{ result.total_authors }}</span>
          <span class="stat-label">作者分组</span>
        </div>
        <div class="stat-card">
          <span class="stat-value warning">{{ result.no_author_count }}</span>
          <span class="stat-label">未识别作者</span>
        </div>
        <div class="stat-card">
          <span class="stat-value">{{ totalSelected }}</span>
          <span class="stat-label">已选文件</span>
        </div>
      </div>

      <div class="action-panel">
        <div class="action-panel-head">
          <div>
            <div class="panel-title">操作设置</div>
            <div class="panel-subtitle">
              当前已选 {{ totalSelected }} 个文件，约
              {{ formatSize(totalSelectedSize) }}
            </div>
          </div>
          <button
            class="btn-preview"
            :disabled="totalSelected === 0"
            @click="generatePreview"
          >
            预览变更
          </button>
        </div>

        <div class="action-options">
          <label class="action-option">
            <input
              v-model="action"
              type="radio"
              value="move_to_author_folder"
            />
            移动到作者子文件夹
          </label>
          <label class="action-option">
            <input v-model="action" type="radio" value="rename" />
            批量重命名
          </label>
        </div>

        <div v-if="action === 'rename'" class="rename-box">
          <label class="rename-label">命名模板</label>
          <input v-model="renameTemplate" class="rename-input" />
          <div class="rename-tip">
            支持变量：{{ "{author}" }}、{{ "{filename}" }}、{{ "{extension}" }}
          </div>
        </div>

        <div v-if="preview" class="preview-box">
          <div class="preview-head">
            <span class="panel-title">预览结果</span>
            <button
              class="btn-execute"
              :disabled="executing || preview.total === 0"
              @click="executeChanges"
            >
              {{ executing ? "执行中…" : "确认执行" }}
            </button>
          </div>
          <div class="preview-list">
            <div
              v-for="item in preview.items"
              :key="item.source_path"
              class="preview-item"
            >
              <div class="preview-desc">{{ item.action_desc }}</div>
              <div class="preview-path">{{ item.source_path }}</div>
              <div class="preview-arrow">→</div>
              <div class="preview-path target">{{ item.target_path }}</div>
            </div>
          </div>
        </div>

        <div v-if="result.no_author_count > 0" class="message-box">
          有
          {{
            result.no_author_count
          }}
          个文件未识别到作者，已保持原位置不参与归类。
        </div>

        <div v-if="executionMessage" class="message-box">
          {{ executionMessage }}
        </div>
      </div>

      <div v-if="result.groups.length === 0" class="placeholder">
        未找到符合条件的媒体文件
      </div>

      <div v-else class="group-list">
        <div
          v-for="group in result.groups"
          :key="group.author"
          class="author-group"
        >
          <div class="group-head">
            <div>
              <div class="group-author">{{ group.author }}</div>
              <div class="group-meta">
                {{ group.file_count }} 个文件 ·
                {{ formatSize(group.total_size) }}
              </div>
            </div>
            <div class="group-tools">
              <span class="group-selected"
                >已选 {{ groupCheckedCount(group) }}</span
              >
              <button class="btn-sm" @click="toggleGroup(group, true)">
                全选
              </button>
              <button class="btn-sm" @click="toggleGroup(group, false)">
                清空
              </button>
            </div>
          </div>

          <div class="file-list">
            <label
              v-for="file in group.files"
              :key="file.path"
              class="file-row"
            >
              <input v-model="file.checked" type="checkbox" />
              <span class="file-type">{{ mediaIcon(file.media_type) }}</span>
              <span class="file-name">{{ file.file_name }}</span>
              <span class="file-date">{{ formatDate(file.modified_at) }}</span>
              <span class="file-path" :title="file.path">{{ file.path }}</span>
            </label>
          </div>
        </div>
      </div>
    </section>

    <div v-else class="placeholder">
      选择目录并扫描后，按作者查看媒体文件分组
    </div>
  </div>
</template>

<style scoped>
.media-classify-view {
  padding: 16px;
  height: 100%;
  overflow-y: auto;
}

.header {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  margin-bottom: 16px;
}

.header h2 {
  font-size: 18px;
  font-weight: 600;
}

.header-tip {
  color: var(--color-text-secondary);
  font-size: 12px;
}

.source-section,
.action-panel,
.author-group,
.preview-box {
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: 10px;
}

.source-section,
.action-panel {
  padding: 14px;
}

.section-head,
.action-panel-head,
.group-head,
.preview-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.label,
.filter-label,
.panel-title,
.group-author {
  font-weight: 600;
}

.recursive-toggle,
.filter-chip,
.action-option,
.file-row {
  display: flex;
  align-items: center;
  gap: 6px;
}

.source-list,
.group-list,
.file-list,
.preview-list {
  display: flex;
  flex-direction: column;
}

.source-list {
  gap: 6px;
  margin: 10px 0;
}

.source-item,
.file-row,
.preview-item {
  background: var(--color-bg);
  border-radius: 6px;
}

.source-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 10px;
}

.source-text,
.file-path,
.preview-path {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.btn-x,
.btn-add,
.btn-scan,
.btn-preview,
.btn-execute,
.btn-sm {
  cursor: pointer;
}

.btn-x {
  border: none;
  background: transparent;
  color: var(--color-text-secondary);
}

.btn-add {
  border: 2px dashed var(--color-border);
  border-radius: 6px;
  background: transparent;
  color: var(--color-text-secondary);
  padding: 10px;
}

.btn-scan,
.btn-preview,
.btn-execute {
  height: 36px;
  border: none;
  border-radius: 6px;
  color: #fff;
  background: var(--color-primary);
  padding: 0 16px;
}

.btn-scan {
  width: 100%;
  margin-top: 12px;
}

.btn-scan:disabled,
.btn-preview:disabled,
.btn-execute:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.filter-row,
.action-options {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
}

.filter-row {
  align-items: center;
}

.filter-chip {
  padding: 6px 10px;
  border: 1px solid var(--color-border);
  border-radius: 999px;
  background: var(--color-bg);
}

.result-section {
  display: flex;
  flex-direction: column;
  gap: 12px;
  margin-top: 12px;
}

.summary-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
  gap: 12px;
}

.stat-card {
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: 10px;
  padding: 14px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.stat-value {
  font-size: 22px;
  font-weight: 700;
}

.stat-label,
.panel-subtitle,
.group-meta,
.group-selected,
.file-date,
.rename-tip,
.preview-desc,
.message-box {
  color: var(--color-text-secondary);
  font-size: 12px;
}

.highlight {
  color: var(--color-primary);
}

.warning {
  color: var(--color-warning);
}

.rename-box {
  margin-top: 12px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.rename-input {
  height: 36px;
  border: 1px solid var(--color-border);
  border-radius: 6px;
  padding: 0 12px;
  background: var(--color-bg);
  color: var(--color-text);
}

.preview-box {
  margin-top: 12px;
  padding: 12px;
}

.preview-list,
.group-list {
  gap: 10px;
}

.preview-item {
  padding: 10px;
}

.preview-arrow {
  margin: 6px 0;
  color: var(--color-primary);
}

.preview-path.target {
  color: var(--color-primary);
}

.message-box {
  margin-top: 12px;
  padding: 10px 12px;
  border-radius: 6px;
  background: var(--color-bg);
}

.author-group {
  padding: 14px;
}

.group-tools {
  display: flex;
  align-items: center;
  gap: 8px;
}

.btn-sm {
  height: 28px;
  padding: 0 10px;
  border: 1px solid var(--color-border);
  border-radius: 6px;
  background: var(--color-bg);
  color: var(--color-text);
}

.file-list {
  gap: 6px;
  margin-top: 12px;
}

.file-row {
  padding: 8px 10px;
  display: grid;
  grid-template-columns: 20px 28px minmax(120px, 220px) 160px 1fr;
  gap: 10px;
}

.file-type {
  text-align: center;
}

.file-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.placeholder {
  padding: 64px;
  text-align: center;
  color: var(--color-text-secondary);
}

@media (max-width: 900px) {
  .header,
  .section-head,
  .action-panel-head,
  .group-head,
  .preview-head {
    flex-direction: column;
    align-items: flex-start;
    gap: 10px;
  }

  .file-row {
    grid-template-columns: 20px 28px 1fr;
  }

  .file-date,
  .file-path {
    grid-column: 3;
  }
}
</style>
