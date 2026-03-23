<script setup lang="ts">
import type { PreviewItem } from "../../types";

defineProps<{ item: PreviewItem }>();
defineEmits<{ toggle: [id: string] }>();

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1048576) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1048576).toFixed(1)} MB`;
}

function actionIcon(type: string): string {
  const icons: Record<string, string> = {
    rename: "✏️",
    move: "📦",
    copy: "📋",
    delete: "🗑️",
  };
  return icons[type] || "📄";
}
</script>

<template>
  <div
    class="preview-row"
    :class="{ unchecked: !item.checked, conflict: !!item.conflict }"
  >
    <label class="check-col">
      <input
        type="checkbox"
        :checked="item.checked"
        @change="$emit('toggle', item.id)"
      />
    </label>

    <div class="info-col">
      <div class="file-path">
        <span class="source-name">{{ item.source.name }}</span>
        <span class="source-dir">{{ item.source.path }}</span>
      </div>
      <div class="arrow">→</div>
      <div class="target-path">
        <span class="target-name">{{ item.target.name }}</span>
        <span class="target-dir">{{ item.target.path }}</span>
      </div>
    </div>

    <div class="change-col">
      <span
        v-for="c in item.changes"
        :key="c.rule_id"
        class="change-badge"
        :title="c.description"
      >
        {{ actionIcon(c.action_type) }} {{ c.description }}
      </span>
    </div>

    <div class="size-col">{{ formatSize(item.source.size_bytes) }}</div>

    <div
      v-if="item.conflict"
      class="conflict-badge"
      :title="item.conflict.resolved_path"
    >
      ⚠ {{ item.conflict.conflict_type }}
    </div>
  </div>
</template>

<style scoped>
.preview-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  border-bottom: 1px solid var(--color-border);
  font-size: 13px;
}

.preview-row:hover {
  background: var(--color-hover);
}

.preview-row.unchecked {
  opacity: 0.4;
}

.preview-row.conflict {
  background: rgba(214, 158, 46, 0.06);
}

.check-col {
  flex-shrink: 0;
}

.check-col input {
  width: 15px;
  height: 15px;
  cursor: pointer;
}

.info-col {
  flex: 1;
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  overflow: hidden;
}

.file-path,
.target-path {
  display: flex;
  flex-direction: column;
  min-width: 0;
}

.source-name {
  font-weight: 500;
  color: var(--color-danger);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.target-name {
  font-weight: 500;
  color: var(--color-success);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.source-dir,
.target-dir {
  font-size: 11px;
  color: var(--color-text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.arrow {
  flex-shrink: 0;
  color: var(--color-text-secondary);
  font-weight: 700;
}

.change-col {
  display: flex;
  gap: 4px;
  flex-shrink: 0;
}

.change-badge {
  font-size: 11px;
  padding: 2px 6px;
  background: var(--color-active);
  border-radius: 3px;
  white-space: nowrap;
}

.size-col {
  flex-shrink: 0;
  width: 70px;
  text-align: right;
  color: var(--color-text-secondary);
  font-size: 12px;
}

.conflict-badge {
  flex-shrink: 0;
  font-size: 11px;
  color: var(--color-warning);
  white-space: nowrap;
}
</style>
