import { defineStore } from "pinia";
import { ref } from "vue";
import type { RuleSet } from "../types";
import { loadRuleSets, saveRuleSet, deleteRuleSet } from "../utils/tauriApi";

export const useRuleStore = defineStore("rules", () => {
  const ruleSets = ref<RuleSet[]>([]);
  const activeRuleSetId = ref<string | null>(null);
  const loading = ref(false);

  const activeRuleSet = () =>
    ruleSets.value.find((rs) => rs.id === activeRuleSetId.value) ?? null;

  async function load() {
    loading.value = true;
    try {
      ruleSets.value = await loadRuleSets();
    } finally {
      loading.value = false;
    }
  }

  async function save(ruleSet: RuleSet) {
    await saveRuleSet(ruleSet);
    await load();
  }

  async function remove(id: string) {
    await deleteRuleSet(id);
    if (activeRuleSetId.value === id) activeRuleSetId.value = null;
    await load();
  }

  return {
    ruleSets,
    activeRuleSetId,
    activeRuleSet,
    loading,
    load,
    save,
    remove,
  };
});
