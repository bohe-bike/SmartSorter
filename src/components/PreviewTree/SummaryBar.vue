<script setup lang="ts">
import type { PreviewSummary } from "../../types";

defineProps<{ summary: PreviewSummary }>();
</script>

<template>
  <div class="summary-bar">
    <div class="stat">
      <span class="stat-value">{{ summary.total_scanned }}</span>
      <span class="stat-label">扫描</span>
    </div>
    <div class="stat">
      <span class="stat-value highlight">{{ summary.matched }}</span>
      <span class="stat-label">匹配</span>
    </div>
    <div class="stat" v-if="summary.to_rename > 0">
      <span class="stat-value">{{ summary.to_rename }}</span>
      <span class="stat-label">重命名</span>
    </div>
    <div class="stat" v-if="summary.to_move > 0">
      <span class="stat-value">{{ summary.to_move }}</span>
      <span class="stat-label">移动</span>
    </div>
    <div class="stat" v-if="summary.to_copy > 0">
      <span class="stat-value">{{ summary.to_copy }}</span>
      <span class="stat-label">复制</span>
    </div>
    <div class="stat" v-if="summary.to_delete > 0">
      <span class="stat-value danger">{{ summary.to_delete }}</span>
      <span class="stat-label">删除</span>
    </div>
    <div class="stat" v-if="summary.conflicts > 0">
      <span class="stat-value warning">{{ summary.conflicts }}</span>
      <span class="stat-label">冲突</span>
    </div>
    <div class="stat" v-if="summary.errors > 0">
      <span class="stat-value danger">{{ summary.errors }}</span>
      <span class="stat-label">错误</span>
    </div>
  </div>
</template>

<style scoped>
.summary-bar {
  display: flex;
  gap: 16px;
  padding: 10px 12px;
  background: var(--color-bg);
  border-radius: 6px;
  border: 1px solid var(--color-border);
}

.stat {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
}

.stat-value {
  font-size: 18px;
  font-weight: 700;
  color: var(--color-text);
}

.stat-value.highlight {
  color: var(--color-primary);
}

.stat-value.danger {
  color: var(--color-danger);
}

.stat-value.warning {
  color: var(--color-warning);
}

.stat-label {
  font-size: 11px;
  color: var(--color-text-secondary);
}
</style>
