<script setup lang="ts">
import { onMounted } from "vue";
import { useHistoryStore } from "../stores/historyStore";

const historyStore = useHistoryStore();

onMounted(() => {
  historyStore.load();
});

function formatDate(iso: string): string {
  try {
    return new Date(iso).toLocaleString("zh-CN");
  } catch {
    return iso;
  }
}

function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

async function handleUndo(logId: string) {
  await historyStore.undo(logId);
}
</script>

<template>
  <div class="history-view">
    <div class="header">
      <h2>操作历史</h2>
      <button
        class="btn-refresh"
        @click="historyStore.load"
        :disabled="historyStore.loading"
      >
        🔄 刷新
      </button>
    </div>

    <div v-if="historyStore.loading" class="placeholder">加载中…</div>

    <div v-else-if="historyStore.undoError" class="error-banner">
      ✖ 撤销失败: {{ historyStore.undoError }}
      <button class="btn-dismiss" @click="historyStore.undoError = null">
        ×
      </button>
    </div>

    <div v-else-if="historyStore.logs.length === 0" class="placeholder">
      暂无操作记录
    </div>

    <div v-else class="log-list">
      <div v-for="log in historyStore.logs" :key="log.log_id" class="log-card">
        <div class="log-header">
          <div class="log-meta">
            <span class="log-time">{{ formatDate(log.executed_at) }}</span>
            <span class="log-duration"
              >⏱ {{ formatDuration(log.duration_ms) }}</span
            >
            <span v-if="log.rule_set_name" class="log-ruleset"
              >📋 {{ log.rule_set_name }}</span
            >
          </div>
          <div class="log-actions">
            <span class="undo-status" :class="log.undo_status">
              {{
                log.undo_status === "available"
                  ? "可撤销"
                  : log.undo_status === "partial"
                    ? "部分撤销"
                    : "已过期"
              }}
            </span>
            <button
              v-if="log.undo_status === 'available'"
              class="btn-undo"
              @click="handleUndo(log.log_id)"
            >
              ↩ 撤销
            </button>
          </div>
        </div>

        <div class="log-summary">
          <span class="stat">
            <span class="stat-num">{{ log.summary.total }}</span> 总计
          </span>
          <span class="stat success">
            <span class="stat-num">{{ log.summary.succeeded }}</span> 成功
          </span>
          <span v-if="log.summary.failed > 0" class="stat danger">
            <span class="stat-num">{{ log.summary.failed }}</span> 失败
          </span>
          <span v-if="log.summary.skipped > 0" class="stat">
            <span class="stat-num">{{ log.summary.skipped }}</span> 跳过
          </span>
        </div>

        <details class="log-details">
          <summary>查看详情 ({{ log.operations.length }} 项操作)</summary>
          <div class="op-list">
            <div
              v-for="op in log.operations"
              :key="op.op_id"
              class="op-row"
              :class="op.status"
            >
              <span class="op-action">{{ op.action }}</span>
              <span class="op-source">{{ op.source_path }}</span>
              <span class="op-arrow">→</span>
              <span class="op-target">{{ op.target_path }}</span>
              <span v-if="op.error_message" class="op-error">{{
                op.error_message
              }}</span>
            </div>
          </div>
        </details>
      </div>
    </div>
  </div>
</template>

<style scoped>
.history-view {
  padding: 16px;
  height: 100%;
  overflow-y: auto;
}

.header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 16px;
}

.header h2 {
  font-size: 18px;
  font-weight: 600;
}

.btn-refresh {
  height: 32px;
  padding: 0 14px;
  border: 1px solid var(--color-border);
  border-radius: 6px;
  background: var(--color-surface);
  color: var(--color-text);
  cursor: pointer;
  font-size: 13px;
}

.btn-refresh:hover:not(:disabled) {
  background: var(--color-hover);
}

.placeholder {
  color: var(--color-text-secondary);
  text-align: center;
  padding: 64px;
}

.error-banner {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 14px;
  background: rgba(229, 62, 62, 0.08);
  border: 1px solid var(--color-danger);
  border-radius: 6px;
  color: var(--color-danger);
  font-size: 13px;
  margin-bottom: 4px;
}

.btn-dismiss {
  margin-left: auto;
  background: none;
  border: none;
  cursor: pointer;
  color: var(--color-danger);
  font-size: 16px;
  line-height: 1;
}

.log-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.log-card {
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: 8px;
  padding: 14px;
}

.log-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 10px;
}

.log-meta {
  display: flex;
  gap: 12px;
  align-items: center;
  font-size: 13px;
}

.log-time {
  font-weight: 500;
}

.log-duration {
  color: var(--color-text-secondary);
}

.log-ruleset {
  color: var(--color-primary);
}

.log-actions {
  display: flex;
  gap: 8px;
  align-items: center;
}

.undo-status {
  font-size: 12px;
  padding: 2px 8px;
  border-radius: 4px;
}

.undo-status.available {
  background: rgba(56, 161, 105, 0.1);
  color: var(--color-success);
}

.undo-status.partial {
  background: rgba(214, 158, 46, 0.1);
  color: var(--color-warning);
}

.undo-status.expired {
  background: var(--color-hover);
  color: var(--color-text-secondary);
}

.btn-undo {
  height: 28px;
  padding: 0 12px;
  border: 1px solid var(--color-primary);
  border-radius: 4px;
  background: transparent;
  color: var(--color-primary);
  cursor: pointer;
  font-size: 12px;
}

.btn-undo:hover {
  background: var(--color-active);
}

.log-summary {
  display: flex;
  gap: 16px;
  margin-bottom: 8px;
}

.stat {
  font-size: 13px;
  color: var(--color-text-secondary);
}

.stat .stat-num {
  font-weight: 700;
  color: var(--color-text);
}

.stat.success .stat-num {
  color: var(--color-success);
}

.stat.danger .stat-num {
  color: var(--color-danger);
}

.log-details summary {
  font-size: 12px;
  color: var(--color-text-secondary);
  cursor: pointer;
  padding: 4px 0;
}

.op-list {
  margin-top: 6px;
  border-top: 1px solid var(--color-border);
  max-height: 200px;
  overflow-y: auto;
}

.op-row {
  display: flex;
  gap: 8px;
  align-items: center;
  padding: 4px 0;
  font-size: 12px;
  border-bottom: 1px solid var(--color-border);
}

.op-row.failed {
  opacity: 0.6;
}

.op-action {
  flex-shrink: 0;
  width: 50px;
  font-weight: 500;
  color: var(--color-primary);
}

.op-source,
.op-target {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.op-source {
  color: var(--color-danger);
}

.op-arrow {
  flex-shrink: 0;
  color: var(--color-text-secondary);
}

.op-target {
  color: var(--color-success);
}

.op-error {
  flex-shrink: 0;
  color: var(--color-danger);
  font-size: 11px;
}
</style>
