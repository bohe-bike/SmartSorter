<script setup lang="ts">
import type { Rule, Condition, Action } from "../../types";
import ConditionRow from "./ConditionRow.vue";
import ActionRow from "./ActionRow.vue";

const props = defineProps<{ rule: Rule }>();
const emit = defineEmits<{
  update: [rule: Rule];
  remove: [];
}>();

function toggleEnabled() {
  emit("update", { ...props.rule, enabled: !props.rule.enabled });
}

function updateName(name: string) {
  emit("update", { ...props.rule, name });
}

function updateLogic(logic: "AND" | "OR") {
  emit("update", {
    ...props.rule,
    condition_group: { ...props.rule.condition_group, logic },
  });
}

function addCondition() {
  const newCond: Condition = {
    field: "extension",
    operator: "equals",
    value: "",
  };
  emit("update", {
    ...props.rule,
    condition_group: {
      ...props.rule.condition_group,
      conditions: [...props.rule.condition_group.conditions, newCond],
    },
  });
}

function updateCondition(index: number, cond: Condition) {
  const conditions = [...props.rule.condition_group.conditions];
  conditions[index] = cond;
  emit("update", {
    ...props.rule,
    condition_group: { ...props.rule.condition_group, conditions },
  });
}

function removeCondition(index: number) {
  const conditions = props.rule.condition_group.conditions.filter(
    (_, i) => i !== index,
  );
  emit("update", {
    ...props.rule,
    condition_group: { ...props.rule.condition_group, conditions },
  });
}

function addAction() {
  const newAction: Action = {
    type: "move",
    params: { dest_pattern: "", conflict_strategy: "skip" },
  };
  emit("update", {
    ...props.rule,
    actions: [...props.rule.actions, newAction],
  });
}

function updateAction(index: number, action: Action) {
  const actions = [...props.rule.actions];
  actions[index] = action;
  emit("update", { ...props.rule, actions });
}

function removeAction(index: number) {
  const actions = props.rule.actions.filter((_, i) => i !== index);
  emit("update", { ...props.rule, actions });
}
</script>

<template>
  <div class="rule-card" :class="{ disabled: !rule.enabled }">
    <div class="rule-header">
      <label class="toggle">
        <input
          type="checkbox"
          :checked="rule.enabled"
          @change="toggleEnabled"
        />
      </label>
      <input
        class="rule-name"
        :value="rule.name"
        @input="updateName(($event.target as HTMLInputElement).value)"
        placeholder="规则名称"
      />
      <button
        class="btn-icon btn-delete"
        @click="emit('remove')"
        title="删除规则"
      >
        🗑
      </button>
    </div>

    <!-- IF 条件区 -->
    <div class="section">
      <div class="section-label">
        <span>IF</span>
        <select
          class="logic-select"
          :value="rule.condition_group.logic"
          @change="
            updateLogic(
              ($event.target as HTMLSelectElement).value as 'AND' | 'OR',
            )
          "
        >
          <option value="AND">全部满足（AND）</option>
          <option value="OR">任一满足（OR）</option>
        </select>
      </div>
      <div class="condition-list">
        <ConditionRow
          v-for="(cond, i) in rule.condition_group.conditions"
          :key="i"
          :condition="cond"
          @update="updateCondition(i, $event)"
          @remove="removeCondition(i)"
        />
        <button class="btn-add" @click="addCondition">+ 添加条件</button>
      </div>
    </div>

    <!-- THEN 动作区 -->
    <div class="section">
      <div class="section-label"><span>THEN</span></div>
      <div class="action-list">
        <ActionRow
          v-for="(act, i) in rule.actions"
          :key="i"
          :action="act"
          @update="updateAction(i, $event)"
          @remove="removeAction(i)"
        />
        <button class="btn-add" @click="addAction">+ 添加动作</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.rule-card {
  border: 1px solid var(--color-border);
  border-radius: 8px;
  padding: 12px;
  background: var(--color-surface);
}

.rule-card.disabled {
  opacity: 0.5;
}

.rule-header {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 12px;
}

.toggle input {
  width: 16px;
  height: 16px;
  cursor: pointer;
}

.rule-name {
  flex: 1;
  border: none;
  border-bottom: 1px solid var(--color-border);
  background: transparent;
  font-size: 14px;
  font-weight: 600;
  color: var(--color-text);
  padding: 4px 0;
  outline: none;
}

.rule-name:focus {
  border-color: var(--color-primary);
}

.section {
  margin-bottom: 12px;
}

.section-label {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
}

.section-label span {
  font-size: 12px;
  font-weight: 700;
  color: var(--color-primary);
  text-transform: uppercase;
  letter-spacing: 1px;
}

.logic-select {
  height: 26px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  padding: 0 6px;
  font-size: 12px;
  background: var(--color-bg);
  color: var(--color-text);
}

.condition-list,
.action-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.btn-add {
  border: 1px dashed var(--color-border);
  border-radius: 4px;
  padding: 6px;
  background: transparent;
  color: var(--color-text-secondary);
  cursor: pointer;
  font-size: 13px;
  text-align: center;
}

.btn-add:hover {
  border-color: var(--color-primary);
  color: var(--color-primary);
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
}

.btn-delete:hover {
  color: var(--color-danger);
}
</style>
