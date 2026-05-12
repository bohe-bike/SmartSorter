import { defineStore } from "pinia";
import { ref } from "vue";
import type { ExecutionLog } from "../types";
import { loadHistory, undoTask } from "../utils/tauriApi";

export const useHistoryStore = defineStore("history", () => {
  const logs = ref<ExecutionLog[]>([]);
  const loading = ref(false);
  const undoError = ref<string | null>(null);

  async function load() {
    loading.value = true;
    try {
      logs.value = await loadHistory();
    } finally {
      loading.value = false;
    }
  }

  async function undo(logId: string) {
    undoError.value = null;
    try {
      await undoTask(logId);
      await load();
    } catch (e: any) {
      undoError.value = String(e);
    }
  }

  return { logs, loading, undoError, load, undo };
});
