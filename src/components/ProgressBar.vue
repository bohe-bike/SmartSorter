<script setup lang="ts">
defineProps<{
  current: number;
  total: number;
  currentFile: string;
  phase: string;
}>();

const phaseLabels: Record<string, string> = {
  scanning: "扫描文件中…",
  extracting: "提取作者中…",
  hashing: "计算哈希中…",
  matching: "匹配规则中…",
  executing: "执行操作中…",
};
</script>

<template>
  <div class="progress-bar-wrapper">
    <div class="progress-info">
      <span class="phase-label">{{ phaseLabels[phase] || phase }}</span>
      <span class="progress-count" v-if="total > 0"
        >{{ current }} / {{ total }}</span
      >
    </div>
    <div class="progress-track">
      <div
        class="progress-fill"
        :style="{
          width:
            total > 0 ? `${Math.min((current / total) * 100, 100)}%` : '0%',
        }"
      />
    </div>
    <div class="progress-file" v-if="currentFile" :title="currentFile">
      {{ currentFile.length > 60 ? "…" + currentFile.slice(-58) : currentFile }}
    </div>
  </div>
</template>

<style scoped>
.progress-bar-wrapper {
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: 8px;
  padding: 12px 14px;
  margin: 8px 0;
}

.progress-info {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 6px;
}

.phase-label {
  font-size: 13px;
  font-weight: 500;
  color: var(--color-primary);
}

.progress-count {
  font-size: 12px;
  color: var(--color-text-secondary);
  font-variant-numeric: tabular-nums;
}

.progress-track {
  height: 6px;
  background: var(--color-bg);
  border-radius: 3px;
  overflow: hidden;
}

.progress-fill {
  height: 100%;
  background: var(--color-primary);
  border-radius: 3px;
  transition: width 0.15s ease;
}

.progress-file {
  margin-top: 4px;
  font-size: 11px;
  color: var(--color-text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  font-family: monospace;
}
</style>
