<script setup lang="ts">
import { computed } from "vue";
import type {
  Action,
  RenameAction,
  MoveAction,
  CopyAction,
  ConflictStrategy,
} from "../../types";
import { pickFolder } from "../../utils/tauriApi";

const props = defineProps<{ action: Action }>();
const emit = defineEmits<{
  update: [action: Action];
  remove: [];
}>();

const actionTypes = [
  { value: "rename", label: "重命名" },
  { value: "move", label: "移动到" },
  { value: "copy", label: "复制到" },
  { value: "delete", label: "删除" },
];

const renameModes = [
  { value: "replace", label: "查找替换" },
  { value: "prefix", label: "添加前缀" },
  { value: "suffix", label: "添加后缀" },
  { value: "sequence", label: "序列号" },
];

const conflictStrategies: { value: ConflictStrategy; label: string }[] = [
  { value: "skip", label: "跳过" },
  { value: "overwrite", label: "覆盖" },
  { value: "auto_rename", label: "自动重命名" },
];

const actionType = computed(() => props.action.type);

function changeType(type: string) {
  const defaults: Record<string, Action> = {
    rename: {
      type: "rename",
      params: { mode: "replace", find: "", replace: "" },
    },
    move: {
      type: "move",
      params: { dest_pattern: "", conflict_strategy: "skip" },
    },
    copy: {
      type: "copy",
      params: { dest_pattern: "", conflict_strategy: "skip" },
    },
    delete: { type: "delete", params: { confirm_required: true } },
  };
  emit("update", defaults[type] || defaults.rename);
}

function updateRenameMode(mode: string) {
  const base = { mode: mode as RenameAction["params"]["mode"] };
  const defaults: Record<string, object> = {
    replace: { find: "", replace: "" },
    prefix: { text: "" },
    suffix: { text: "" },
    sequence: { template: "{filename}_{seq}", padding: 3 },
  };
  emit("update", {
    type: "rename",
    params: { ...base, ...defaults[mode] },
  } as RenameAction);
}

function updateRenameParam(key: string, val: string) {
  const a = props.action as RenameAction;
  emit("update", {
    type: "rename",
    params: { ...a.params, [key]: val },
  } as RenameAction);
}

function updateRoutePattern(val: string) {
  const a = props.action as MoveAction | CopyAction;
  emit("update", {
    ...a,
    params: { ...a.params, dest_pattern: val },
  } as Action);
}

// 拆分 dest_pattern 为基路径和子目录
const routeBase = computed(() => {
  const p = (props.action as MoveAction).params.dest_pattern || "";
  const sep =
    p.lastIndexOf("\\") >= 0 ? p.lastIndexOf("\\") : p.lastIndexOf("/");
  return sep >= 0 ? p.substring(0, sep) : "";
});

const routeSub = computed(() => {
  const p = (props.action as MoveAction).params.dest_pattern || "";
  const sep =
    p.lastIndexOf("\\") >= 0 ? p.lastIndexOf("\\") : p.lastIndexOf("/");
  return sep >= 0 ? p.substring(sep + 1) : p;
});

function buildAndEmitRoute(base: string, sub: string) {
  const b = base.replace(/[\\/]+$/, "");
  const s = sub.replace(/^[\\/]+/, "");
  const combined = s ? `${b}\\${s}` : b;
  updateRoutePattern(combined);
}

async function pickBaseFolder() {
  const path = await pickFolder();
  if (path) {
    buildAndEmitRoute(path, routeSub.value);
  }
}

function updateConflict(val: ConflictStrategy) {
  const a = props.action as MoveAction | CopyAction;
  emit("update", {
    ...a,
    params: { ...a.params, conflict_strategy: val },
  } as Action);
}
</script>

<template>
  <div class="action-row">
    <div class="action-header">
      <select
        class="type-select"
        :value="actionType"
        @change="changeType(($event.target as HTMLSelectElement).value)"
      >
        <option v-for="t in actionTypes" :key="t.value" :value="t.value">
          {{ t.label }}
        </option>
      </select>
      <button class="btn-icon" @click="emit('remove')" title="删除动作">
        ✕
      </button>
    </div>

    <!-- Rename -->
    <div v-if="actionType === 'rename'" class="action-params">
      <select
        class="mode-select"
        :value="(action as RenameAction).params.mode"
        @change="updateRenameMode(($event.target as HTMLSelectElement).value)"
      >
        <option v-for="m in renameModes" :key="m.value" :value="m.value">
          {{ m.label }}
        </option>
      </select>

      <template v-if="(action as RenameAction).params.mode === 'replace'">
        <input
          class="param-input"
          placeholder="查找"
          :value="(action as RenameAction).params.find"
          @input="
            updateRenameParam('find', ($event.target as HTMLInputElement).value)
          "
        />
        <input
          class="param-input"
          placeholder="替换为"
          :value="(action as RenameAction).params.replace"
          @input="
            updateRenameParam(
              'replace',
              ($event.target as HTMLInputElement).value,
            )
          "
        />
      </template>

      <template v-else-if="(action as RenameAction).params.mode === 'prefix'">
        <input
          class="param-input"
          placeholder="前缀文本"
          :value="(action as RenameAction).params.text"
          @input="
            updateRenameParam('text', ($event.target as HTMLInputElement).value)
          "
        />
      </template>

      <template v-else-if="(action as RenameAction).params.mode === 'suffix'">
        <input
          class="param-input"
          placeholder="后缀文本"
          :value="(action as RenameAction).params.text"
          @input="
            updateRenameParam('text', ($event.target as HTMLInputElement).value)
          "
        />
      </template>

      <template v-else-if="(action as RenameAction).params.mode === 'sequence'">
        <input
          class="param-input"
          placeholder="模板 如 {filename}_{seq}"
          :value="(action as RenameAction).params.template"
          @input="
            updateRenameParam(
              'template',
              ($event.target as HTMLInputElement).value,
            )
          "
        />
      </template>
    </div>

    <!-- Move / Copy -->
    <div
      v-else-if="actionType === 'move' || actionType === 'copy'"
      class="action-params route-params"
    >
      <div class="route-row">
        <label class="route-label">目标基路径</label>
        <div class="route-base-group">
          <input
            class="param-input flex-1"
            placeholder="点击右侧按钮选择目录，或直接输入路径"
            :value="routeBase"
            @input="
              buildAndEmitRoute(
                ($event.target as HTMLInputElement).value,
                routeSub,
              )
            "
          />
          <button class="btn-pick" @click="pickBaseFolder" title="选择文件夹">
            📂
          </button>
        </div>
      </div>
      <div class="route-row">
        <label class="route-label">子目录名称</label>
        <input
          class="param-input flex-1"
          placeholder="输入子目录名，如：周杰伦、{created_year}"
          :value="routeSub"
          @input="
            buildAndEmitRoute(
              routeBase,
              ($event.target as HTMLInputElement).value,
            )
          "
        />
      </div>
      <div class="route-preview" v-if="routeBase">
        📁 {{ routeBase }}\{{ routeSub || "..." }}
      </div>
      <select
        class="conflict-select"
        :value="(action as MoveAction).params.conflict_strategy"
        @change="
          updateConflict(
            ($event.target as HTMLSelectElement).value as ConflictStrategy,
          )
        "
      >
        <option v-for="s in conflictStrategies" :key="s.value" :value="s.value">
          {{ s.label }}
        </option>
      </select>
    </div>

    <!-- Delete -->
    <div v-else-if="actionType === 'delete'" class="action-params">
      <span class="delete-warn">⚠ 匹配的文件将被删除</span>
    </div>
  </div>
</template>

<style scoped>
.action-row {
  border: 1px solid var(--color-border);
  border-radius: 6px;
  padding: 8px;
  background: var(--color-bg);
}

.action-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 8px;
}

.type-select,
.mode-select,
.conflict-select {
  height: 30px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  padding: 0 8px;
  font-size: 13px;
  background: var(--color-surface);
  color: var(--color-text);
}

.action-params {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
}

.param-input {
  height: 30px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  padding: 0 8px;
  font-size: 13px;
  background: var(--color-surface);
  color: var(--color-text);
  min-width: 120px;
}

.flex-1 {
  flex: 1;
}

.route-input-group {
  display: flex;
  align-items: center;
  gap: 4px;
  flex: 1;
  min-width: 0;
}

.route-hint {
  font-size: 12px;
  color: var(--color-text-secondary);
  white-space: nowrap;
  flex-shrink: 0;
}

.route-tip {
  width: 100%;
  font-size: 11px;
  color: var(--color-text-secondary);
  opacity: 0.7;
}

.route-params {
  flex-direction: column;
  align-items: stretch;
}

.route-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.route-label {
  font-size: 12px;
  color: var(--color-text-secondary);
  white-space: nowrap;
  width: 72px;
  flex-shrink: 0;
}

.route-base-group {
  display: flex;
  gap: 4px;
  flex: 1;
  min-width: 0;
}

.btn-pick {
  height: 30px;
  width: 34px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  background: var(--color-surface);
  cursor: pointer;
  font-size: 14px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.btn-pick:hover {
  border-color: var(--color-primary);
  background: var(--color-active);
}

.route-preview {
  font-size: 11px;
  color: var(--color-text-secondary);
  background: var(--color-bg);
  border-radius: 4px;
  padding: 4px 8px;
  font-family: monospace;
  word-break: break-all;
}

.btn-icon {
  width: 28px;
  height: 28px;
  border: none;
  border-radius: 4px;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  color: var(--color-text-secondary);
}

.btn-icon:hover {
  background: var(--color-hover);
  color: var(--color-danger);
}

.delete-warn {
  color: var(--color-danger);
  font-size: 13px;
}
</style>
