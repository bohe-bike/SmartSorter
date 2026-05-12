import { defineStore } from "pinia";
import { ref } from "vue";
import type { PreviewResult, PreviewRequest } from "../types";
import { analyzePreview, executeTask } from "../utils/tauriApi";

export const usePreviewStore = defineStore("preview", () => {
  const result = ref<PreviewResult | null>(null);
  const analyzing = ref(false);
  const executing = ref(false);
  const progress = ref({ current: 0, total: 0, currentFile: "" });

  async function analyze(request: PreviewRequest) {
    analyzing.value = true;
    try {
      result.value = await analyzePreview(request);
    } finally {
      analyzing.value = false;
    }
  }

  const executeResult = ref<{ success: boolean; message: string } | null>(null);

  async function execute(taskId: string) {
    if (!result.value) return;
    executing.value = true;
    executeResult.value = null;
    try {
      const checkedIds = result.value.items
        .filter((item) => item.checked)
        .map((item) => item.id);
      const msg = await executeTask(taskId, checkedIds);
      executeResult.value = { success: true, message: msg || "执行完成" };
      result.value = null; // 执行成功后清除预览，防止重复执行旧任务
    } catch (e: any) {
      executeResult.value = { success: false, message: String(e) };
    } finally {
      executing.value = false;
    }
  }

  function toggleItem(itemId: string) {
    if (!result.value) return;
    const item = result.value.items.find((i) => i.id === itemId);
    if (item) item.checked = !item.checked;
  }

  function clear() {
    result.value = null;
  }

  return {
    result,
    analyzing,
    executing,
    executeResult,
    progress,
    analyze,
    execute,
    toggleItem,
    clear,
  };
});
