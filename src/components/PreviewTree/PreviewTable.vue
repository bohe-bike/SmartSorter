<script setup lang="ts">
import type { PreviewResult } from "../../types";
import SummaryBar from "./SummaryBar.vue";
import PreviewRow from "./PreviewRow.vue";

defineProps<{ result: PreviewResult }>();
defineEmits<{ toggle: [id: string] }>();
</script>

<template>
  <div class="preview-table">
    <SummaryBar :summary="result.summary" />

    <div class="table-header">
      <span class="col-check"></span>
      <span class="col-info">文件变更</span>
      <span class="col-action">操作</span>
      <span class="col-size">大小</span>
    </div>

    <div class="table-body">
      <PreviewRow
        v-for="item in result.items"
        :key="item.id"
        :item="item"
        @toggle="$emit('toggle', $event)"
      />
      <div v-if="result.items.length === 0" class="empty">没有匹配的文件</div>
    </div>

    <div v-if="result.errors.length > 0" class="error-list">
      <div class="error-title">⚠ 错误 ({{ result.errors.length }})</div>
      <div v-for="(err, i) in result.errors" :key="i" class="error-item">
        <span class="error-path">{{ err.path }}</span>
        <span class="error-msg">{{ err.message }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.preview-table {
  display: flex;
  flex-direction: column;
  gap: 8px;
  height: 100%;
}

.table-header {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 10px;
  font-size: 12px;
  font-weight: 600;
  color: var(--color-text-secondary);
  border-bottom: 2px solid var(--color-border);
}

.col-check {
  width: 15px;
}
.col-info {
  flex: 1;
}
.col-action {
  width: 120px;
}
.col-size {
  width: 70px;
  text-align: right;
}

.table-body {
  flex: 1;
  overflow-y: auto;
}

.empty {
  text-align: center;
  padding: 32px;
  color: var(--color-text-secondary);
}

.error-list {
  border-top: 1px solid var(--color-border);
  padding-top: 8px;
  max-height: 120px;
  overflow-y: auto;
}

.error-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--color-danger);
  margin-bottom: 4px;
}

.error-item {
  display: flex;
  gap: 8px;
  font-size: 12px;
  padding: 2px 0;
}

.error-path {
  color: var(--color-text);
  font-weight: 500;
}

.error-msg {
  color: var(--color-danger);
}
</style>
