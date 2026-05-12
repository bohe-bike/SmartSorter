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
  KeywordGroup,
  ClassifyPreviewResult,
  MediaClassifyResult,
  ProgressEvent,
} from "../types";

const sourcePaths = ref<string[]>([]);
const recursive = ref(false);
const mediaTypeOptions = ref([
  { key: "image", label: "图片", checked: true },
  { key: "audio", label: "音频", checked: true },
  { key: "video", label: "视频", checked: true },
  { key: "ebook", label: "电子书", checked: true },
]);
const keywordSourceOptions = ref([
  { key: "folder_name", label: "子文件夹名称", checked: true },
  { key: "artist", label: "作者/艺术家", checked: true },
  { key: "album_artist", label: "专辑艺术家", checked: true },
  { key: "album", label: "专辑名", checked: true },
  { key: "composer", label: "作曲家", checked: true },
]);
const scanning = ref(false);
const executing = ref(false);
const result = ref<MediaClassifyResult | null>(null);
const preview = ref<ClassifyPreviewResult | null>(null);
const executionMessage = ref("");
const progress = ref({ current: 0, total: 0, currentFile: "", phase: "" });
// 用户对多关键字匹配文件的手动选择：文件路径 → 选定关键字
const keywordAssignments = ref<Record<string, string>>({});
// 分组折叠状态：存储已折叠的关键字
const collapsedGroups = ref(new Set<string>());
// 关键字分组搜索过滤词
const keywordFilter = ref("");

let unlistenProgress: (() => void) | null = null;

const selectedMediaTypes = computed(() =>
  mediaTypeOptions.value.filter((item) => item.checked).map((item) => item.key),
);

const selectedKeywordSources = computed(() =>
  keywordSourceOptions.value
    .filter((item) => item.checked)
    .map((item) => item.key),
);

const checkedPaths = computed(() => {
  if (!result.value) return [] as string[];
  const groupedPaths = result.value.groups.flatMap((group) =>
    group.files.filter((file) => file.checked).map((file) => file.path),
  );
  const unmatchedPaths = result.value.unmatched_files
    .filter((file) => file.checked && keywordAssignments.value[file.path])
    .map((file) => file.path);
  return [...groupedPaths, ...unmatchedPaths];
});

const totalSelected = computed(() => checkedPaths.value.length);

const totalSelectedSize = computed(() => {
  if (!result.value) return 0;
  const groupedSize = result.value.groups.reduce((sum, group) => {
    return (
      sum +
      group.files
        .filter((file) => file.checked)
        .reduce((groupSum, file) => groupSum + file.size_bytes, 0)
    );
  }, 0);
  const unmatchedSize = result.value.unmatched_files
    .filter((file) => file.checked && keywordAssignments.value[file.path])
    .reduce((sum, file) => sum + file.size_bytes, 0);
  return groupedSize + unmatchedSize;
});

// 多关键字匹配的文件列表
const multiMatchFiles = computed(() => {
  if (!result.value) return [];
  const files: { path: string; fileName: string; keywords: string[] }[] = [];
  for (const group of result.value.groups) {
    for (const file of group.files) {
      if (file.matched_keywords.length > 1) {
        files.push({
          path: file.path,
          fileName: file.file_name,
          keywords: file.matched_keywords,
        });
      }
    }
  }
  return files;
});

// 未匹配的文件列表
const unmatchedFiles = computed(() => {
  if (!result.value) return [];
  return result.value.unmatched_files;
});

// 所有可用关键字（用于未匹配文件的手动选择）
const allKeywords = computed(() => {
  if (!result.value) return [];
  return result.value.keywords.map((k) => k.keyword);
});

// 合并信息
const mergedKeywords = computed(() => {
  if (!result.value) return [];
  return result.value.keywords.filter((k) => k.merged_from.length > 0);
});

// 按关键字过滤后的分组列表
const filteredGroups = computed(() => {
  if (!result.value) return [];
  const q = keywordFilter.value.trim().toLowerCase();
  if (!q) return result.value.groups;
  return result.value.groups.filter((g) => g.keyword.toLowerCase().includes(q));
});

// 剩余未分配关键字的未匹配文件数（随用户选择实时更新）
const remainingUnmatched = computed(() => {
  if (!result.value) return 0;
  return result.value.unmatched_files.filter(
    (file) => !keywordAssignments.value[file.path],
  ).length;
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
  if (
    sourcePaths.value.length === 0 ||
    selectedMediaTypes.value.length === 0 ||
    selectedKeywordSources.value.length === 0
  ) {
    return;
  }

  scanning.value = true;
  preview.value = null;
  executionMessage.value = "";
  keywordAssignments.value = {};
  collapsedGroups.value = new Set();
  keywordFilter.value = "";
  resetProgress("scanning");
  await prepareProgressListener();

  try {
    result.value = await scanMediaAuthors(
      sourcePaths.value,
      recursive.value,
      selectedMediaTypes.value,
      selectedKeywordSources.value,
    );
  } catch (error) {
    alert("扫描失败: " + error);
  } finally {
    scanning.value = false;
    cleanupProgress();
  }
}

function toggleGroup(group: KeywordGroup, checked: boolean) {
  group.files.forEach((file) => {
    file.checked = checked;
  });
}

function groupCheckedCount(group: KeywordGroup): number {
  return group.files.filter((file) => file.checked).length;
}

function assignKeyword(filePath: string, keyword: string) {
  keywordAssignments.value[filePath] = keyword;
}

function toggleCollapse(keyword: string) {
  if (collapsedGroups.value.has(keyword)) {
    collapsedGroups.value.delete(keyword);
  } else {
    collapsedGroups.value.add(keyword);
  }
  // 触发响应式更新
  collapsedGroups.value = new Set(collapsedGroups.value);
}

function clearPreview() {
  preview.value = null;
}

async function generatePreview() {
  if (!result.value || checkedPaths.value.length === 0) {
    return;
  }

  // 构建 keyword_assignments：所有选中文件都需要有一个关键字
  const assignments: Record<string, string> = { ...keywordAssignments.value };
  for (const group of result.value.groups) {
    for (const file of group.files) {
      if (file.checked && !assignments[file.path]) {
        assignments[file.path] = group.keyword;
      }
    }
  }

  try {
    preview.value = await previewMediaClassify({
      task_id: result.value.task_id,
      keyword_assignments: assignments,
      selected_paths: checkedPaths.value,
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
      <h2>媒体归类</h2>
      <span class="header-tip">按关键字批量归类，支持预览后执行</span>
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

      <div class="filter-row">
        <span class="filter-label">关键字来源</span>
        <label
          v-for="item in keywordSourceOptions"
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
          selectedMediaTypes.length === 0 ||
          selectedKeywordSources.length === 0
        "
        @click="runScan"
      >
        {{ scanning ? "扫描中…" : "开始扫描" }}
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
          <span class="stat-value highlight">{{ result.total_keywords }}</span>
          <span class="stat-label">关键字分组</span>
        </div>
        <div class="stat-card">
          <span class="stat-value warning">{{ remainingUnmatched }}</span>
          <span class="stat-label">未匹配</span>
        </div>
        <div class="stat-card">
          <span class="stat-value">{{ totalSelected }}</span>
          <span class="stat-label">已选文件</span>
        </div>
      </div>

      <!-- 合并提示 -->
      <div v-if="mergedKeywords.length > 0" class="merge-notice">
        <span class="merge-icon">🔗</span>
        <div class="merge-text">
          <div
            v-for="mk in mergedKeywords"
            :key="mk.keyword"
            class="merge-line"
          >
            「{{ mk.merged_from.join("、") }}」已合并到「{{ mk.keyword }}」
          </div>
        </div>
      </div>

      <!-- 多关键字匹配提示 -->
      <div v-if="multiMatchFiles.length > 0" class="multi-match-section">
        <div class="panel-title">⚠ 多关键字匹配文件（请选择归类目标）</div>
        <div
          v-for="mf in multiMatchFiles"
          :key="mf.path"
          class="multi-match-row"
        >
          <span class="multi-match-name">{{ mf.fileName }}</span>
          <select
            class="keyword-select"
            :value="keywordAssignments[mf.path] || mf.keywords[0]"
            @change="
              assignKeyword(mf.path, ($event.target as HTMLSelectElement).value)
            "
          >
            <option v-for="kw in mf.keywords" :key="kw" :value="kw">
              {{ kw }}
            </option>
          </select>
        </div>
      </div>

      <!-- 未匹配文件提示 -->
      <div v-if="unmatchedFiles.length > 0" class="unmatched-section">
        <div class="panel-title">📭 未匹配文件（请手动选择关键字归类）</div>
        <div
          v-for="file in unmatchedFiles"
          :key="file.path"
          class="unmatched-row"
        >
          <input type="checkbox" v-model="file.checked" />
          <span class="unmatched-name">{{ file.file_name }}</span>
          <select
            class="keyword-select"
            :value="keywordAssignments[file.path] || ''"
            :disabled="!file.checked"
            @change="
              assignKeyword(
                file.path,
                ($event.target as HTMLSelectElement).value,
              )
            "
          >
            <option value="" disabled>请选择关键字</option>
            <option v-for="kw in allKeywords" :key="kw" :value="kw">
              {{ kw }}
            </option>
          </select>
        </div>
      </div>

      <div class="action-panel">
        <div class="action-panel-head">
          <div>
            <div class="panel-title">操作设置</div>
            <div class="panel-subtitle">
              移动到关键字文件夹并重命名为「关键字-主题.后缀」，已选
              {{ totalSelected }} 个文件，约
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

        <div v-if="preview" class="preview-box">
          <div class="preview-head">
            <span class="panel-title">预览结果（{{ preview.total }} 项）</span>
            <div class="preview-head-actions">
              <button class="btn-clear-preview" @click="clearPreview">
                ✕ 取消
              </button>
              <button
                class="btn-execute"
                :disabled="executing || preview.total === 0"
                @click="executeChanges"
              >
                {{ executing ? "执行中…" : "确认执行" }}
              </button>
            </div>
          </div>
          <div class="preview-list">
            <div
              v-for="item in preview.items"
              :key="item.source_path"
              class="preview-item"
            >
              <div class="preview-desc">
                {{ item.action_desc }} · {{ formatSize(item.size_bytes) }}
              </div>
              <div class="preview-path">{{ item.source_path }}</div>
              <div class="preview-arrow">→</div>
              <div class="preview-path target">{{ item.target_path }}</div>
            </div>
          </div>
        </div>

        <div v-if="remainingUnmatched > 0" class="message-box">
          有
          {{ remainingUnmatched }} 个文件未匹配到任何关键字，将保持原位不动。
        </div>

        <div v-if="executionMessage" class="message-box success-msg">
          {{ executionMessage }}
        </div>
      </div>

      <div v-if="result.groups.length === 0" class="placeholder">
        未找到符合条件的媒体文件
      </div>

      <div v-else class="group-list">
        <div class="keyword-filter-bar">
          <input
            v-model="keywordFilter"
            class="keyword-filter-input"
            placeholder="搜索关键字分组…"
          />
          <span class="keyword-filter-count"
            >{{ filteredGroups.length }} / {{ result.groups.length }}</span
          >
        </div>
        <div
          v-for="group in filteredGroups"
          :key="group.keyword"
          class="author-group"
        >
          <div class="group-head">
            <div>
              <div class="group-author">{{ group.keyword }}</div>
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
              <button
                class="btn-sm btn-collapse"
                @click="toggleCollapse(group.keyword)"
              >
                {{ collapsedGroups.has(group.keyword) ? "▶" : "▼" }}
              </button>
            </div>
          </div>

          <div v-show="!collapsedGroups.has(group.keyword)" class="file-list">
            <label
              v-for="file in group.files"
              :key="file.path"
              class="file-row"
              :class="{ 'multi-match': file.matched_keywords.length > 1 }"
            >
              <input v-model="file.checked" type="checkbox" />
              <span class="file-type">{{ mediaIcon(file.media_type) }}</span>
              <span class="file-name">{{ file.file_name }}</span>
              <span v-if="file.matched_keywords.length > 1" class="match-badge">
                {{ file.matched_keywords.length }}匹配
              </span>
              <span class="file-date">{{ formatDate(file.modified_at) }}</span>
              <span class="file-path" :title="file.path">{{ file.path }}</span>
            </label>
          </div>
        </div>
      </div>
    </section>

    <div v-else class="placeholder">
      选择目录并扫描后，按关键字查看媒体文件分组
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
  margin-top: 8px;
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

/* 合并提示 */
.merge-notice {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 10px 14px;
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: 8px;
  font-size: 12px;
}

.merge-icon {
  font-size: 16px;
  flex-shrink: 0;
  margin-top: 1px;
}

.merge-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.merge-line {
  color: var(--color-text-secondary);
}

/* 多关键字匹配 */
.multi-match-section {
  background: var(--color-surface);
  border: 1px solid var(--color-warning);
  border-radius: 10px;
  padding: 14px;
}

.multi-match-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 0;
}

.multi-match-name {
  flex: 1;
  font-size: 13px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* 未匹配文件 */
.unmatched-section {
  background: var(--color-surface);
  border: 1px solid var(--color-text-secondary);
  border-radius: 10px;
  padding: 14px;
}

.unmatched-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 0;
}

.unmatched-name {
  flex: 1;
  font-size: 13px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--color-text-secondary);
}

.keyword-select {
  height: 28px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  padding: 0 8px;
  font-size: 12px;
  background: var(--color-bg);
  color: var(--color-text);
  min-width: 100px;
}

.file-row.multi-match {
  border-left: 3px solid var(--color-warning);
}

.match-badge {
  flex-shrink: 0;
  font-size: 10px;
  padding: 1px 6px;
  border-radius: 3px;
  background: rgba(221, 156, 0, 0.12);
  color: var(--color-warning);
}

.success-msg {
  border-left: 3px solid var(--color-success);
}

/* 预览头部操作区 */
.preview-head-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.btn-clear-preview {
  height: 28px;
  padding: 0 10px;
  border: 1px solid var(--color-border);
  border-radius: 6px;
  background: transparent;
  color: var(--color-text-secondary);
  cursor: pointer;
  font-size: 12px;
}

.btn-clear-preview:hover {
  color: var(--color-text);
  border-color: var(--color-text-secondary);
}

/* 关键字过滤栏 */
.keyword-filter-bar {
  display: flex;
  align-items: center;
  gap: 8px;
}

.keyword-filter-input {
  flex: 1;
  height: 32px;
  border: 1px solid var(--color-border);
  border-radius: 6px;
  padding: 0 10px;
  font-size: 13px;
  background: var(--color-bg);
  color: var(--color-text);
}

.keyword-filter-count {
  color: var(--color-text-secondary);
  font-size: 12px;
  white-space: nowrap;
}

/* 折叠按钮 */
.btn-collapse {
  width: 28px;
  padding: 0;
  text-align: center;
  font-size: 10px;
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
