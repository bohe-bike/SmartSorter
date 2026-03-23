<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { useRuleStore } from "../stores/ruleStore";
import { usePreviewStore } from "../stores/previewStore";
import RuleCard from "../components/RuleBuilder/RuleCard.vue";
import MagicVarRef from "../components/RuleBuilder/MagicVarRef.vue";
import PreviewTable from "../components/PreviewTree/PreviewTable.vue";
import ProgressBar from "../components/ProgressBar.vue";
import type { Rule, RuleSet, ProgressEvent } from "../types";
import { pickFolder, listenProgress } from "../utils/tauriApi";

const ruleStore = useRuleStore();
const previewStore = usePreviewStore();

const sourcePaths = ref<string[]>([]);
const recursive = ref(true);

// 当前编辑中的规则方案（本地状态，保存时才持久化）
const editingRuleSet = ref<RuleSet | null>(null);

// 进度状态
const progress = ref({ current: 0, total: 0, currentFile: "", phase: "" });
const showProgress = ref(false);
let unlistenProgress: (() => void) | null = null;

onMounted(() => {
  ruleStore.load();
});

const canAnalyze = computed(
  () =>
    sourcePaths.value.length > 0 &&
    editingRuleSet.value !== null &&
    editingRuleSet.value.rules.length > 0,
);

async function addFolder() {
  const path = await pickFolder();
  if (path && !sourcePaths.value.includes(path)) {
    sourcePaths.value.push(path);
  }
}

function removePath(index: number) {
  sourcePaths.value.splice(index, 1);
}

// ===== 规则方案管理 =====

function createRuleSet() {
  const now = new Date().toISOString();
  editingRuleSet.value = {
    id: crypto.randomUUID(),
    name: "新规则方案",
    description: "",
    created_at: now,
    updated_at: now,
    rules: [],
  };
}

function selectRuleSet(rs: RuleSet) {
  editingRuleSet.value = JSON.parse(JSON.stringify(rs));
}

function addRule() {
  if (!editingRuleSet.value) return;
  const rule: Rule = {
    id: crypto.randomUUID(),
    enabled: true,
    name: `规则 ${editingRuleSet.value.rules.length + 1}`,
    condition_group: { logic: "AND", conditions: [], sub_groups: [] },
    actions: [],
  };
  editingRuleSet.value.rules.push(rule);
}

function updateRule(index: number, rule: Rule) {
  if (!editingRuleSet.value) return;
  editingRuleSet.value.rules[index] = rule;
}

function removeRule(index: number) {
  if (!editingRuleSet.value) return;
  editingRuleSet.value.rules.splice(index, 1);
}

async function saveRuleSet() {
  if (!editingRuleSet.value) return;
  editingRuleSet.value.updated_at = new Date().toISOString();
  await ruleStore.save(editingRuleSet.value);
  // 保存后从 store 同步最新数据，确保下拉框名称正确
  const saved = ruleStore.ruleSets.find(
    (rs) => rs.id === editingRuleSet.value!.id,
  );
  if (saved) {
    editingRuleSet.value = JSON.parse(JSON.stringify(saved));
  }
}

// ===== 进度监听 =====

async function startProgress() {
  progress.value = { current: 0, total: 0, currentFile: "", phase: "" };
  showProgress.value = true;
  unlistenProgress = await listenProgress((e: ProgressEvent) => {
    progress.value = {
      current: e.current,
      total: e.total,
      currentFile: e.current_file,
      phase: e.phase,
    };
  });
}

function stopProgress() {
  showProgress.value = false;
  if (unlistenProgress) {
    unlistenProgress();
    unlistenProgress = null;
  }
}

onUnmounted(() => {
  stopProgress();
});

// ===== 预览 =====

async function runPreview() {
  if (!editingRuleSet.value) return;
  await saveRuleSet();
  await startProgress();
  try {
    await previewStore.analyze({
      source_paths: sourcePaths.value,
      rule_set_id: editingRuleSet.value.id,
      recursive: recursive.value,
      max_depth: null,
    });
  } finally {
    stopProgress();
  }
}

// ===== Toast 通知 =====
const toast = ref<{
  visible: boolean;
  success: boolean;
  summary: string;
  details: string[];
}>({ visible: false, success: true, summary: "", details: [] });
let toastTimer: ReturnType<typeof setTimeout> | null = null;

function showToast(success: boolean, message: string) {
  if (toastTimer) {
    clearTimeout(toastTimer);
    toastTimer = null;
  }
  // 解析消息：第一行为摘要，后续为详情
  const lines = message.split("\n").filter((l) => l.trim());
  const summary = lines[0] || message;
  // 跳过"失败详情:"标题行，保留具体条目
  const detailStart = lines.findIndex((l) => l.includes("失败详情"));
  const details = detailStart >= 0 ? lines.slice(detailStart + 1) : [];
  toast.value = { visible: true, success, summary, details };
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

async function runExecute() {
  if (!previewStore.result) return;
  toast.value.visible = false;
  await startProgress();
  try {
    await previewStore.execute(previewStore.result.task_id);
  } finally {
    stopProgress();
  }
  if (previewStore.executeResult) {
    showToast(
      previewStore.executeResult.success,
      previewStore.executeResult.message,
    );
  }
}
</script>

<template>
  <div class="workspace">
    <!-- 左侧：数据源 + 规则配置 -->
    <div class="left-panel">
      <!-- ① 数据源区域 -->
      <section class="source-panel">
        <div class="panel-header">
          <h3>数据源</h3>
          <label class="recursive-toggle">
            <input type="checkbox" v-model="recursive" /> 递归子目录
          </label>
        </div>
        <div class="drop-zone">
          <div v-for="(p, i) in sourcePaths" :key="p" class="source-item">
            <span class="source-path">📂 {{ p }}</span>
            <button class="btn-remove" @click="removePath(i)">✕</button>
          </div>
          <button class="btn-add-folder" @click="addFolder">
            + 选择文件夹
          </button>
        </div>
      </section>

      <!-- ② 规则配置区域 -->
      <section class="rule-panel">
        <div class="panel-header">
          <h3>规则配置</h3>
          <div class="rule-actions">
            <select
              class="preset-select"
              :value="editingRuleSet?.id ?? ''"
              @change="
                ($event.target as HTMLSelectElement).value === '__new__'
                  ? createRuleSet()
                  : selectRuleSet(
                      ruleStore.ruleSets.find(
                        (r) =>
                          r.id === ($event.target as HTMLSelectElement).value,
                      )!,
                    )
              "
            >
              <option value="" disabled>选择方案…</option>
              <option
                v-for="rs in ruleStore.ruleSets"
                :key="rs.id"
                :value="rs.id"
              >
                {{ rs.name }}
              </option>
              <option value="__new__">＋ 新建方案</option>
            </select>
            <button v-if="editingRuleSet" class="btn-save" @click="saveRuleSet">
              💾 保存
            </button>
          </div>
        </div>

        <template v-if="editingRuleSet">
          <input
            class="ruleset-name"
            v-model="editingRuleSet.name"
            placeholder="方案名称"
          />
          <MagicVarRef />
          <div class="rule-list">
            <RuleCard
              v-for="(rule, i) in editingRuleSet.rules"
              :key="rule.id"
              :rule="rule"
              @update="updateRule(i, $event)"
              @remove="removeRule(i)"
            />
            <button class="btn-add-rule" @click="addRule">+ 添加规则</button>
          </div>
        </template>
        <p v-else class="placeholder">选择或新建一个规则方案开始配置</p>
      </section>
    </div>

    <!-- 右侧：预览 & 执行 -->
    <section class="preview-panel">
      <div class="panel-header">
        <h3>预览结果</h3>
        <div class="preview-actions">
          <button
            class="btn-primary"
            :disabled="!canAnalyze || previewStore.analyzing"
            @click="runPreview"
          >
            {{ previewStore.analyzing ? "分析中…" : "🔍 分析预览" }}
          </button>
          <button
            v-if="previewStore.result && previewStore.result.items.length > 0"
            class="btn-execute"
            :disabled="previewStore.executing"
            @click="runExecute"
          >
            {{ previewStore.executing ? "执行中…" : "▶ 执行" }}
          </button>
        </div>
      </div>

      <ProgressBar
        v-if="showProgress"
        :current="progress.current"
        :total="progress.total"
        :current-file="progress.currentFile"
        :phase="progress.phase"
      />

      <PreviewTable
        v-if="previewStore.result"
        :result="previewStore.result"
        @toggle="previewStore.toggleItem($event)"
      />
      <p v-else-if="!showProgress" class="placeholder">
        配置规则后点击「分析预览」查看 Diff
      </p>
    </section>

    <!-- Toast 通知 -->
    <Transition name="toast">
      <div
        v-if="toast.visible"
        class="toast"
        :class="toast.success ? 'toast-success' : 'toast-error'"
      >
        <div class="toast-header">
          <span class="toast-icon">{{ toast.success ? "✅" : "❌" }}</span>
          <span class="toast-summary">{{ toast.summary }}</span>
          <button class="toast-close" @click="dismissToast">✕</button>
        </div>
        <div v-if="toast.details.length > 0" class="toast-details">
          <div
            v-for="(line, i) in toast.details"
            :key="i"
            class="toast-detail-line"
          >
            {{ line }}
          </div>
        </div>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.workspace {
  display: flex;
  gap: 12px;
  height: 100%;
  padding: 16px;
  overflow: hidden;
}

.left-panel {
  width: 420px;
  min-width: 360px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 12px;
  overflow-y: auto;
}

.panel-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 10px;
}

.panel-header h3 {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
}

.source-panel,
.rule-panel,
.preview-panel {
  background: var(--color-surface);
  border-radius: 8px;
  padding: 14px;
}

.rule-panel {
  flex: 1;
  overflow-y: auto;
}

.preview-panel {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.recursive-toggle {
  font-size: 12px;
  color: var(--color-text-secondary);
  display: flex;
  align-items: center;
  gap: 4px;
  cursor: pointer;
}

.drop-zone {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.source-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 10px;
  background: var(--color-bg);
  border-radius: 4px;
}

.source-path {
  font-size: 13px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.btn-remove {
  background: none;
  border: none;
  cursor: pointer;
  color: var(--color-text-secondary);
  font-size: 14px;
}

.btn-add-folder {
  border: 2px dashed var(--color-border);
  border-radius: 6px;
  padding: 10px;
  background: transparent;
  color: var(--color-text-secondary);
  cursor: pointer;
  font-size: 13px;
}

.btn-add-folder:hover {
  border-color: var(--color-primary);
  color: var(--color-primary);
}

/* 规则面板 */
.rule-actions {
  display: flex;
  gap: 8px;
  align-items: center;
}

.preset-select {
  height: 30px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  padding: 0 8px;
  font-size: 13px;
  background: var(--color-bg);
  color: var(--color-text);
  min-width: 140px;
}

.btn-save {
  height: 30px;
  padding: 0 12px;
  border: 1px solid var(--color-primary);
  border-radius: 4px;
  background: transparent;
  color: var(--color-primary);
  cursor: pointer;
  font-size: 13px;
}

.btn-save:hover {
  background: var(--color-active);
}

.ruleset-name {
  width: 100%;
  border: none;
  border-bottom: 1px solid var(--color-border);
  background: transparent;
  font-size: 16px;
  font-weight: 600;
  color: var(--color-text);
  padding: 4px 0;
  margin-bottom: 10px;
  outline: none;
}

.ruleset-name:focus {
  border-color: var(--color-primary);
}

.rule-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin-top: 10px;
}

.btn-add-rule {
  border: 1px dashed var(--color-border);
  border-radius: 6px;
  padding: 10px;
  background: transparent;
  color: var(--color-text-secondary);
  cursor: pointer;
  font-size: 13px;
  text-align: center;
}

.btn-add-rule:hover {
  border-color: var(--color-primary);
  color: var(--color-primary);
}

/* 预览面板 */
.preview-actions {
  display: flex;
  gap: 8px;
}

.btn-primary {
  height: 32px;
  padding: 0 16px;
  border: none;
  border-radius: 6px;
  background: var(--color-primary);
  color: #fff;
  cursor: pointer;
  font-size: 13px;
  font-weight: 500;
}

.btn-primary:hover:not(:disabled) {
  background: var(--color-primary-hover);
}

.btn-primary:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.btn-execute {
  height: 32px;
  padding: 0 16px;
  border: none;
  border-radius: 6px;
  background: var(--color-success);
  color: #fff;
  cursor: pointer;
  font-size: 13px;
  font-weight: 500;
}

.btn-execute:hover:not(:disabled) {
  opacity: 0.9;
}

.btn-execute:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.placeholder {
  color: var(--color-text-secondary);
  text-align: center;
  padding: 24px;
  font-size: 13px;
}

/* Toast 通知 */
.toast {
  position: fixed;
  bottom: 20px;
  right: 20px;
  min-width: 320px;
  max-width: 520px;
  border-radius: 8px;
  padding: 12px 16px;
  box-shadow: 0 4px 20px rgba(0, 0, 0, 0.15);
  z-index: 9999;
  font-size: 13px;
}
.toast-success {
  background: var(--color-surface);
  border-left: 4px solid var(--color-success);
}
.toast-error {
  background: var(--color-surface);
  border-left: 4px solid var(--color-danger);
}
.toast-header {
  display: flex;
  align-items: center;
  gap: 8px;
}
.toast-icon {
  font-size: 16px;
  flex-shrink: 0;
}
.toast-summary {
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
.toast-details {
  margin-top: 8px;
  padding-top: 8px;
  border-top: 1px solid var(--color-border);
  max-height: 200px;
  overflow-y: auto;
}
.toast-detail-line {
  font-size: 12px;
  color: var(--color-text-secondary);
  padding: 2px 0;
  white-space: pre-wrap;
  font-family: monospace;
}

/* Toast 动画 */
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
