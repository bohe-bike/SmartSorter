import { defineStore } from "pinia";
import { ref } from "vue";
import type { ExecutionLog } from "../types";
import { loadHistory, undoTask } from "../utils/tauriApi";

export const useHistoryStore = defineStore("history", () => {
  const logs = ref<ExecutionLog[]>([]);
  const loading = ref(false);

  async function load() {
    loading.value = true;
    try {
      logs.value = await loadHistory();
    } finally {
      loading.value = false;
    }
  }

  async function undo(logId: string) {
    await undoTask(logId);
    await load();
  }

  return { logs, loading, load, undo };
});
