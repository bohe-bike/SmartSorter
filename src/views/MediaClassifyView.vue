<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import ProgressBar from "../components/ProgressBar.vue";
import {
  executeMediaClassify,
  applyMediaKeywordGroup,
  deleteMediaKeywordGroup,
  loadCreatorExclusions,
  loadMediaKeywordGroups,
  listenProgress,
  pickFolder,
  previewMediaClassify,
  scanMediaAuthors,
  saveCreatorExclusions,
  saveMediaKeywordGroup,
} from "../utils/tauriApi";
import type {
  KeywordGroup,
  ClassifyPreviewResult,
  MediaFile,
  MediaClassifyResult,
  MediaKeywordGroup,
  ProgressEvent,
} from "../types";

const sourcePaths = ref<string[]>([]);
const recursive = ref(false);
const verifyContentHash = ref(false);
const mediaTypeOptions = ref([
  { key: "image", label: "图片", checked: true },
  { key: "audio", label: "音频", checked: true },
  { key: "video", label: "视频", checked: true },
  { key: "ebook", label: "电子书", checked: true },
]);
const allKeywordSources = [
  "folder_name",
  "artist",
  "album_artist",
  "album",
  "composer",
];
const scanning = ref(false);
const executing = ref(false);
const result = ref<MediaClassifyResult | null>(null);
const preview = ref<ClassifyPreviewResult | null>(null);
const executionMessage = ref("");
const progress = ref({ current: 0, total: 0, currentFile: "", phase: "" });
// 用户对多关键字匹配文件的手动选择：文件路径 → 选定关键字
const keywordAssignments = ref<Record<string, string>>({});
const manualKeywordInputs = ref<Record<string, string>>({});
// 分组折叠状态：存储已折叠的关键字
const collapsedGroups = ref(new Set<string>());
// 关键字分组搜索过滤词
const keywordFilter = ref("");
const creatorExclusions = ref<string[]>([]);
const creatorExclusionInput = ref("");
const savingCreatorExclusions = ref(false);
const workflowMode = ref<"keywords" | "classify">("keywords");
const keywordGroups = ref<MediaKeywordGroup[]>([]);
const selectedKeywordGroupId = ref("");
const editingKeywordGroupId = ref<string | undefined>(undefined);
const keywordGroupName = ref("");
const editableKeywords = ref<string[]>([]);
const keywordInput = ref("");
const showKeywordEditor = ref(false);
const savingKeywordGroup = ref(false);
const keywordSaveMessage = ref("");
const keywordSaveState = ref<"success" | "error" | null>(null);

let unlistenProgress: (() => void) | null = null;

function invalidateVerificationResult() {
  result.value = null;
  preview.value = null;
  executionMessage.value = "";
  keywordAssignments.value = {};
  manualKeywordInputs.value = {};
}

const selectedMediaTypes = computed(() =>
  mediaTypeOptions.value.filter((item) => item.checked).map((item) => item.key),
);

const checkedPaths = computed(() => {
  if (!result.value) return [] as string[];
  const groupedPaths = result.value.groups.flatMap((group) =>
    group.files
      .filter(
        (file) =>
          file.checked &&
          (!file.requires_confirmation || keywordAssignments.value[file.path]),
      )
      .map((file) => file.path),
  );
  const unmatchedPaths = result.value.unmatched_files
    .filter((file) => unmatchedFileReady(file))
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
        .filter(
          (file) =>
            file.checked &&
            (!file.requires_confirmation || keywordAssignments.value[file.path]),
        )
        .reduce((groupSum, file) => groupSum + file.size_bytes, 0)
    );
  }, 0);
  const unmatchedSize = result.value.unmatched_files
    .filter((file) => unmatchedFileReady(file))
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

function updateManualKeyword(filePath: string, value: string) {
  manualKeywordInputs.value[filePath] = value;
  const normalized = value.trim().toLocaleLowerCase();
  const keyword = allKeywords.value.find(
    (item) => item.toLocaleLowerCase() === normalized,
  );
  assignKeyword(filePath, keyword ?? "");
}

function hasInvalidManualKeyword(filePath: string): boolean {
  return Boolean(
    manualKeywordInputs.value[filePath]?.trim() &&
      !keywordAssignments.value[filePath],
  );
}

function unmatchedFileReady(file: MediaFile): boolean {
  if (!file.checked) return false;
  if (keywordAssignments.value[file.path]) return true;
  return Boolean(
    file.release_to_root && !manualKeywordInputs.value[file.path]?.trim(),
  );
}

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
    (file) =>
      !keywordAssignments.value[file.path] &&
      (!file.release_to_root || Boolean(manualKeywordInputs.value[file.path]?.trim())),
  ).length;
});

const releaseToRootCount = computed(() => {
  if (!result.value) return 0;
  return result.value.unmatched_files.filter(
    (file) =>
      file.release_to_root &&
      file.checked &&
      !keywordAssignments.value[file.path] &&
      !manualKeywordInputs.value[file.path]?.trim(),
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

function clearKeywordSaveMessage() {
  keywordSaveMessage.value = "";
  keywordSaveState.value = null;
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

async function generateKeywords() {
  if (
    scanning.value ||
    sourcePaths.value.length === 0 ||
    selectedMediaTypes.value.length === 0
  ) {
    return;
  }

  const hadKeywordEditor = showKeywordEditor.value;
  scanning.value = true;
  preview.value = null;
  executionMessage.value = "";
  keywordAssignments.value = {};
  manualKeywordInputs.value = {};
  collapsedGroups.value = new Set();
  keywordFilter.value = "";
  showKeywordEditor.value = false;
  resetProgress("collecting");

  try {
    await prepareProgressListener();
    const generated = await scanMediaAuthors(
      sourcePaths.value,
      recursive.value,
      selectedMediaTypes.value,
      allKeywordSources,
      "all",
      verifyContentHash.value,
    );
    editableKeywords.value = generated.keywords.map((item) => item.keyword);
    keywordGroupName.value = "";
    editingKeywordGroupId.value = undefined;
    clearKeywordSaveMessage();
    showKeywordEditor.value = true;
    result.value = null;
  } catch (error) {
    showKeywordEditor.value = hadKeywordEditor;
    alert("扫描失败: " + error);
  } finally {
    scanning.value = false;
    cleanupProgress();
  }
}

async function loadKeywordGroups() {
  try {
    keywordGroups.value = await loadMediaKeywordGroups();
  } catch (error) {
    alert("加载关键词组失败: " + error);
  }
}

function addKeyword() {
  const keyword = keywordInput.value.trim();
  if (!keyword) return;
  editableKeywords.value.push(keyword);
  keywordInput.value = "";
  clearKeywordSaveMessage();
}

function removeKeyword(index: number) {
  editableKeywords.value.splice(index, 1);
  clearKeywordSaveMessage();
}

async function saveKeywordGroup() {
  if (!keywordGroupName.value.trim() || editableKeywords.value.length === 0) {
    return;
  }
  const isUpdate = Boolean(editingKeywordGroupId.value);
  savingKeywordGroup.value = true;
  clearKeywordSaveMessage();
  try {
    const saved = await saveMediaKeywordGroup({
      id: editingKeywordGroupId.value,
      name: keywordGroupName.value,
      classificationDimension: "all",
      keywordSources: allKeywordSources,
      keywords: editableKeywords.value,
    });
    const index = keywordGroups.value.findIndex((group) => group.id === saved.id);
    if (index >= 0) keywordGroups.value.splice(index, 1, saved);
    else keywordGroups.value.push(saved);
    keywordGroups.value.sort((left, right) => left.name.localeCompare(right.name));
    selectedKeywordGroupId.value = saved.id;
    editingKeywordGroupId.value = saved.id;
    editableKeywords.value = [...saved.keywords];
    keywordSaveState.value = "success";
    keywordSaveMessage.value = `已${isUpdate ? "更新" : "保存"}关键词组「${saved.name}」，共 ${saved.keywords.length} 个关键词`;
  } catch (error) {
    keywordSaveState.value = "error";
    keywordSaveMessage.value = "保存失败: " + String(error);
  } finally {
    savingKeywordGroup.value = false;
  }
}

function editKeywordGroup(group: MediaKeywordGroup) {
  workflowMode.value = "keywords";
  selectedKeywordGroupId.value = group.id;
  editingKeywordGroupId.value = group.id;
  keywordGroupName.value = group.name;
  editableKeywords.value = [...group.keywords];
  clearKeywordSaveMessage();
  showKeywordEditor.value = true;
  result.value = null;
  preview.value = null;
}

function editSelectedKeywordGroup() {
  const group = keywordGroups.value.find(
    (item) => item.id === selectedKeywordGroupId.value,
  );
  if (group) editKeywordGroup(group);
}

async function deleteKeywordGroup() {
  const group = keywordGroups.value.find(
    (item) => item.id === selectedKeywordGroupId.value,
  );
  if (!group || !confirm(`确定删除关键词组「${group.name}」吗？`)) return;
  try {
    await deleteMediaKeywordGroup(group.id);
    keywordGroups.value = keywordGroups.value.filter((item) => item.id !== group.id);
    selectedKeywordGroupId.value = "";
    if (editingKeywordGroupId.value === group.id) {
      editingKeywordGroupId.value = undefined;
      keywordGroupName.value = "";
      editableKeywords.value = [];
      clearKeywordSaveMessage();
      showKeywordEditor.value = false;
    }
    result.value = null;
    preview.value = null;
  } catch (error) {
    alert("删除关键词组失败: " + error);
  }
}

async function applyKeywordGroup() {
  if (
    scanning.value ||
    !selectedKeywordGroupId.value ||
    sourcePaths.value.length === 0 ||
    selectedMediaTypes.value.length === 0
  ) {
    return;
  }
  scanning.value = true;
  preview.value = null;
  executionMessage.value = "";
  keywordAssignments.value = {};
  manualKeywordInputs.value = {};
  collapsedGroups.value = new Set();
  keywordFilter.value = "";
  resetProgress("collecting");
  try {
    await prepareProgressListener();
    const classifyResult = await applyMediaKeywordGroup(
      sourcePaths.value,
      recursive.value,
      selectedMediaTypes.value,
      selectedKeywordGroupId.value,
      verifyContentHash.value,
    );
    result.value = classifyResult;
    assignSingleMatchDefaults(classifyResult);
  } catch (error) {
    alert("应用关键词组失败: " + error);
  } finally {
    scanning.value = false;
    cleanupProgress();
  }
}

function selectWorkflow(mode: "keywords" | "classify") {
  if (workflowMode.value === mode) return;
  workflowMode.value = mode;
  result.value = null;
  preview.value = null;
  keywordAssignments.value = {};
  manualKeywordInputs.value = {};
}

async function loadSavedCreatorExclusions() {
  try {
    creatorExclusions.value = await loadCreatorExclusions();
  } catch (error) {
    alert("加载频道名称排除词失败: " + error);
  }
}

async function saveCreatorExclusionList(keywords: string[]) {
  savingCreatorExclusions.value = true;
  try {
    creatorExclusions.value = await saveCreatorExclusions(keywords);
    preview.value = null;
    result.value = null;
    keywordAssignments.value = {};
    manualKeywordInputs.value = {};
  } catch (error) {
    alert("保存频道名称排除词失败: " + error);
  } finally {
    savingCreatorExclusions.value = false;
  }
}

async function addCreatorExclusion() {
  const keyword = creatorExclusionInput.value.trim();
  if (!keyword) return;
  creatorExclusionInput.value = "";
  await saveCreatorExclusionList([...creatorExclusions.value, keyword]);
}

async function removeCreatorExclusion(keyword: string) {
  await saveCreatorExclusionList(
    creatorExclusions.value.filter((item) => item !== keyword),
  );
}

function toggleGroup(group: KeywordGroup, checked: boolean) {
  group.files.forEach((file) => {
    file.checked = checked;
  });
}

function groupCheckedCount(group: KeywordGroup): number {
  return group.files.filter(
    (file) =>
      file.checked &&
      (!file.requires_confirmation || keywordAssignments.value[file.path]),
  ).length;
}

function assignKeyword(filePath: string, keyword: string) {
  if (keyword) {
    keywordAssignments.value[filePath] = keyword;
  } else {
    delete keywordAssignments.value[filePath];
  }
}

function assignSingleMatchDefaults(classifyResult: MediaClassifyResult) {
  const files = [
    ...classifyResult.groups.flatMap((group) => group.files),
    ...classifyResult.unmatched_files,
  ];

  for (const file of files) {
    const snapshotReady =
      !classifyResult.verify_content_hash || Boolean(file.sha256);
    if (file.matched_keywords.length === 1 && snapshotReady) {
      assignKeyword(file.path, file.matched_keywords[0]);
      file.checked = true;
    }
  }
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
        if (!file.requires_confirmation) {
          assignments[file.path] = group.keyword;
        }
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

onMounted(() => {
  loadSavedCreatorExclusions();
  loadKeywordGroups();
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

function hasCoverEvidence(file: MediaFile): boolean {
  return file.evidence.some((item) => item.startsWith("封面与“"));
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

      <div class="workflow-switch" role="tablist" aria-label="归类工作模式">
        <button
          class="workflow-tab"
          :class="{ active: workflowMode === 'keywords' }"
          @click="selectWorkflow('keywords')"
        >
          1. 生成关键词
        </button>
        <button
          class="workflow-tab"
          :class="{ active: workflowMode === 'classify' }"
          @click="selectWorkflow('classify')"
        >
          2. 应用关键词组
        </button>
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
        <span class="filter-label">内容校验</span>
        <div class="verification-switch">
          <label :class="{ active: !verifyContentHash }">
            <input v-model="verifyContentHash" type="radio" :value="false" @change="invalidateVerificationResult" />
            快速：路径 + 大小
          </label>
          <label :class="{ active: verifyContentHash }">
            <input v-model="verifyContentHash" type="radio" :value="true" @change="invalidateVerificationResult" />
            严格：SHA-256
          </label>
        </div>
      </div>

      <div
        v-if="workflowMode === 'keywords'"
        class="filter-row exclusion-row"
      >
        <span class="filter-label">频道名称排除</span>
        <div class="exclusion-editor">
          <input
            v-model="creatorExclusionInput"
            class="exclusion-input"
            :disabled="savingCreatorExclusions"
            placeholder="输入频道名"
            @keyup.enter="addCreatorExclusion"
          />
          <button
            class="btn-exclusion-add"
            :disabled="savingCreatorExclusions || !creatorExclusionInput.trim()"
            title="添加频道名称排除词"
            @click="addCreatorExclusion"
          >
            添加
          </button>
          <span
            v-for="keyword in creatorExclusions"
            :key="keyword"
            class="exclusion-tag"
          >
            {{ keyword }}
            <button
              class="btn-exclusion-remove"
              :disabled="savingCreatorExclusions"
              :title="`移除 ${keyword}`"
              @click="removeCreatorExclusion(keyword)"
            >
              ×
            </button>
          </span>
        </div>
      </div>

      <div v-if="workflowMode === 'keywords'" class="filter-row">
        <span class="filter-label">生成来源</span>
        <span>子文件夹、作者/艺术家、专辑艺术家、专辑名、作曲家</span>
      </div>

      <div class="filter-row">
        <span class="filter-label">辅助证据</span>
        <span>内嵌封面完全一致时增强已有文本匹配，不会单独自动归类</span>
      </div>

      <div v-if="workflowMode === 'classify'" class="filter-row">
        <label class="filter-label" for="keyword-group-select">关键词组</label>
        <select
          id="keyword-group-select"
          v-model="selectedKeywordGroupId"
          class="keyword-select"
        >
          <option value="" disabled>选择已保存的关键词组</option>
          <option v-for="group in keywordGroups" :key="group.id" :value="group.id">
            {{ group.name }}（{{ group.keywords.length }}）
          </option>
        </select>
        <button
          class="btn-sm"
          :disabled="!selectedKeywordGroupId"
          @click="editSelectedKeywordGroup"
        >
          编辑
        </button>
        <button
          class="btn-sm"
          :disabled="!selectedKeywordGroupId"
          @click="deleteKeywordGroup"
        >
          删除
        </button>
      </div>

      <button
        v-if="workflowMode === 'keywords'"
        class="btn-scan"
        :disabled="
          sourcePaths.length === 0 ||
          scanning ||
          selectedMediaTypes.length === 0
        "
        @click="generateKeywords"
      >
        {{
          scanning
            ? "重新生成中…"
            : showKeywordEditor
              ? "重新生成关键词"
              : "生成关键词"
        }}
      </button>
      <button
        v-else
        class="btn-scan"
        :disabled="
          sourcePaths.length === 0 ||
          scanning ||
          selectedMediaTypes.length === 0 ||
          !selectedKeywordGroupId
        "
        @click="applyKeywordGroup"
      >
        {{ scanning ? "扫描中…" : "应用关键词组并归类" }}
      </button>

      <ProgressBar
        v-if="scanning || executing"
        :current="progress.current"
        :total="progress.total"
        :current-file="progress.currentFile"
        :phase="progress.phase"
      />
    </section>

    <section
      v-if="workflowMode === 'keywords' && showKeywordEditor"
      class="keyword-editor-section"
    >
      <div class="section-head">
        <div>
          <div class="label">关键词列表</div>
          <div class="panel-subtitle">可在保存前添加、编辑或删除关键词</div>
        </div>
        <button
          class="btn-preview"
          :disabled="savingKeywordGroup || !keywordGroupName.trim() || editableKeywords.length === 0"
          @click="saveKeywordGroup"
        >
          {{ savingKeywordGroup ? "保存中…" : "保存关键词组" }}
        </button>
      </div>
      <div class="keyword-group-name-row">
        <label for="keyword-group-name">名称</label>
        <input
          id="keyword-group-name"
          v-model="keywordGroupName"
          placeholder="例如：常用作者"
          @input="clearKeywordSaveMessage"
        />
      </div>
      <div class="keyword-add-row">
        <input
          v-model="keywordInput"
          placeholder="添加关键词"
          @keyup.enter="addKeyword"
        />
        <button class="btn-sm" :disabled="!keywordInput.trim()" @click="addKeyword">
          添加
        </button>
      </div>
      <div class="editable-keyword-list">
        <div v-for="(_, index) in editableKeywords" :key="index" class="editable-keyword-row">
          <input
            v-model="editableKeywords[index]"
            aria-label="关键词"
            @input="clearKeywordSaveMessage"
          />
          <button class="btn-exclusion-remove" title="删除关键词" @click="removeKeyword(index)">×</button>
        </div>
      </div>
      <div
        v-if="keywordSaveMessage"
        class="message-box keyword-save-message"
        :class="keywordSaveState === 'success' ? 'success-msg' : 'error-msg'"
        role="status"
      >
        {{ keywordSaveMessage }}
      </div>
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
          <span class="stat-label">待确认</span>
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
            :value="keywordAssignments[mf.path] || ''"
            @change="
              assignKeyword(mf.path, ($event.target as HTMLSelectElement).value)
            "
          >
            <option value="" disabled>请选择归属</option>
            <option v-for="kw in mf.keywords" :key="kw" :value="kw">
              {{ kw }}
            </option>
          </select>
        </div>
      </div>

      <!-- 未匹配文件提示 -->
        <div v-if="unmatchedFiles.length > 0" class="unmatched-section">
          <div class="panel-title">📭 待确认与待释放文件</div>
        <div
          v-for="file in unmatchedFiles"
          :key="file.path"
          class="unmatched-row"
        >
          <input type="checkbox" v-model="file.checked" />
          <span class="unmatched-name" :title="file.evidence.join('、')">
            {{ file.file_name }} · 置信度 {{ file.confidence }}%
            <span v-if="file.release_to_root" class="release-label">移到根目录</span>
          </span>
          <select
            v-if="file.matched_keywords.length > 0"
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
            <option v-if="file.release_to_root" value="">移到扫描根目录</option>
            <option v-else value="" disabled>
              请选择候选（{{ file.matched_keywords.length }}）
            </option>
            <option v-for="kw in file.matched_keywords" :key="kw" :value="kw">
              {{ kw }}
            </option>
          </select>
          <input
            v-else
            class="keyword-search-input"
            :class="{ invalid: hasInvalidManualKeyword(file.path) }"
            type="search"
            list="manual-keyword-options"
            :value="manualKeywordInputs[file.path] || ''"
            :disabled="!file.checked"
            :placeholder="
              file.release_to_root
                ? '无候选；留空移到根目录'
                : '无候选；搜索关键字'
            "
            :title="
              hasInvalidManualKeyword(file.path)
                ? '请输入并选择一个现有关键字'
                : '输入文字可过滤现有关键字'
            "
            @input="
              updateManualKeyword(
                file.path,
                ($event.target as HTMLInputElement).value,
              )
            "
          />
        </div>
        <datalist id="manual-keyword-options">
          <option v-for="kw in allKeywords" :key="kw" :value="kw" />
        </datalist>
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
          {{ remainingUnmatched }} 个文件因无可靠归属或候选分数接近，将保持原位不动。
        </div>

        <div v-if="releaseToRootCount > 0" class="message-box">
          {{ releaseToRootCount }} 个文件来自已取消的分类目录，将移到扫描根目录；目录清空后会自动删除。
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
              <span
                v-if="hasCoverEvidence(file)"
                class="cover-evidence"
                :title="file.evidence.join('、')"
              >
                🖼 封面一致
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

.workflow-switch {
  display: flex;
  gap: 6px;
  margin: 12px 0 4px;
}

.workflow-tab {
  height: 32px;
  padding: 0 12px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  background: var(--color-bg);
  color: var(--color-text-secondary);
  cursor: pointer;
  font-size: 12px;
}

.workflow-tab.active {
  border-color: var(--color-primary);
  background: var(--color-active);
  color: var(--color-primary);
}

.verification-switch {
  display: flex;
  min-height: 30px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  overflow: hidden;
}

.verification-switch label {
  display: flex;
  align-items: center;
  cursor: pointer;
  padding: 0 10px;
  color: var(--color-text-secondary);
  font-size: 12px;
}

.verification-switch label + label {
  border-left: 1px solid var(--color-border);
}

.verification-switch label.active {
  background: var(--color-active);
  color: var(--color-primary);
}

.verification-switch input {
  position: absolute;
  opacity: 0;
  pointer-events: none;
}

.keyword-editor-section {
  margin-top: 12px;
  padding: 14px;
  border: 1px solid var(--color-border);
  border-radius: 8px;
  background: var(--color-surface);
}

.keyword-group-name-row,
.keyword-add-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 12px;
}

.keyword-group-name-row label {
  flex-shrink: 0;
  font-size: 13px;
  font-weight: 600;
}

.keyword-group-name-row input,
.keyword-add-row input,
.editable-keyword-row input {
  min-width: 0;
  height: 30px;
  padding: 0 8px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  background: var(--color-bg);
  color: var(--color-text);
  font-size: 13px;
}

.keyword-group-name-row input {
  width: min(320px, 100%);
}

.keyword-add-row input {
  width: min(240px, 100%);
}

.editable-keyword-list {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
  gap: 8px;
  margin-top: 12px;
}

.editable-keyword-row {
  display: flex;
  align-items: center;
  gap: 4px;
}

.editable-keyword-row input {
  width: 100%;
}

.exclusion-row {
  align-items: flex-start;
}

.exclusion-editor {
  display: flex;
  flex: 1;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
}

.exclusion-input {
  width: 150px;
  height: 30px;
  padding: 0 8px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  background: var(--color-bg);
  color: var(--color-text);
  font-size: 12px;
}

.btn-exclusion-add,
.btn-exclusion-remove {
  border: 1px solid var(--color-border);
  background: var(--color-bg);
  color: var(--color-text-secondary);
  cursor: pointer;
}

.btn-exclusion-add {
  height: 30px;
  padding: 0 10px;
  border-radius: 4px;
  font-size: 12px;
}

.btn-exclusion-add:disabled,
.btn-exclusion-remove:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.exclusion-tag {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  min-height: 28px;
  padding: 0 5px 0 8px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  background: rgba(214, 158, 46, 0.08);
  color: var(--color-text);
  font-size: 12px;
}

.btn-exclusion-remove {
  width: 18px;
  height: 18px;
  padding: 0;
  border: 0;
  border-radius: 3px;
  font-size: 15px;
  line-height: 1;
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

.release-label {
  margin-left: 8px;
  color: var(--color-primary);
  font-size: 11px;
  font-weight: 600;
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

.keyword-search-input {
  width: 210px;
  height: 28px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  padding: 0 8px;
  font-size: 12px;
  background: var(--color-bg);
  color: var(--color-text);
}

.keyword-search-input.invalid {
  border-color: var(--color-danger);
}

.keyword-search-input:disabled {
  cursor: not-allowed;
  opacity: 0.55;
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

.cover-evidence {
  flex-shrink: 0;
  color: var(--color-success);
  font-size: 11px;
}

.success-msg {
  border-left: 3px solid var(--color-success);
}

.error-msg {
  border-left: 3px solid var(--color-danger);
  color: var(--color-danger);
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
