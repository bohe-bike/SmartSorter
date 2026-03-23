<script setup lang="ts">
import { computed } from "vue";
import type { Condition, ConditionField, Operator } from "../../types";

const props = defineProps<{ condition: Condition }>();
const emit = defineEmits<{
  update: [condition: Condition];
  remove: [];
}>();

const fieldOptions: { value: ConditionField; label: string }[] = [
  { value: "filename", label: "文件名" },
  { value: "extension", label: "扩展名" },
  { value: "full_name", label: "完整文件名" },
  { value: "size_bytes", label: "文件大小(B)" },
  { value: "created_at", label: "创建时间" },
  { value: "modified_at", label: "修改时间" },
  { value: "parent_dir", label: "父目录" },
];

const stringOps: { value: Operator; label: string }[] = [
  { value: "equals", label: "等于" },
  { value: "not_equals", label: "不等于" },
  { value: "contains", label: "包含" },
  { value: "not_contains", label: "不包含" },
  { value: "starts_with", label: "开头是" },
  { value: "ends_with", label: "结尾是" },
  { value: "regex", label: "正则匹配" },
  { value: "in", label: "属于" },
  { value: "not_in", label: "不属于" },
];

const numberOps: { value: Operator; label: string }[] = [
  { value: "gt", label: "大于" },
  { value: "gte", label: "大于等于" },
  { value: "lt", label: "小于" },
  { value: "lte", label: "小于等于" },
  { value: "between", label: "介于" },
  { value: "equals", label: "等于" },
];

const dateOps: { value: Operator; label: string }[] = [
  { value: "before", label: "早于" },
  { value: "after", label: "晚于" },
  { value: "within_days", label: "最近N天" },
];

const isNumericField = (f: ConditionField) => f === "size_bytes";
const isDateField = (f: ConditionField) =>
  f === "created_at" || f === "modified_at";

const operatorOptions = computed(() => {
  if (isNumericField(props.condition.field)) return numberOps;
  if (isDateField(props.condition.field)) return dateOps;
  return stringOps;
});

function updateField(field: ConditionField) {
  const ops = isNumericField(field)
    ? numberOps
    : isDateField(field)
      ? dateOps
      : stringOps;
  emit("update", {
    ...props.condition,
    field,
    operator: ops[0].value,
    value: "",
  });
}

function updateOperator(operator: Operator) {
  emit("update", { ...props.condition, operator });
}

function updateValue(val: string) {
  let parsed: string | number = val;
  if (isNumericField(props.condition.field) && val !== "") {
    parsed = Number(val);
  }
  emit("update", { ...props.condition, value: parsed });
}
</script>

<template>
  <div class="condition-row">
    <select
      class="field-select"
      :value="condition.field"
      @change="
        updateField(
          ($event.target as HTMLSelectElement).value as ConditionField,
        )
      "
    >
      <option v-for="f in fieldOptions" :key="f.value" :value="f.value">
        {{ f.label }}
      </option>
    </select>

    <select
      class="op-select"
      :value="condition.operator"
      @change="
        updateOperator(($event.target as HTMLSelectElement).value as Operator)
      "
    >
      <option v-for="op in operatorOptions" :key="op.value" :value="op.value">
        {{ op.label }}
      </option>
    </select>

    <input
      class="value-input"
      :type="isNumericField(condition.field) ? 'number' : 'text'"
      :value="condition.value"
      :placeholder="condition.operator === 'regex' ? '正则表达式' : '值'"
      @input="updateValue(($event.target as HTMLInputElement).value)"
    />

    <button
      class="btn-icon btn-remove"
      @click="emit('remove')"
      title="删除条件"
    >
      ✕
    </button>
  </div>
</template>

<style scoped>
.condition-row {
  display: flex;
  gap: 8px;
  align-items: center;
}

.field-select,
.op-select,
.value-input {
  height: 32px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  padding: 0 8px;
  font-size: 13px;
  background: var(--color-bg);
  color: var(--color-text);
}

.field-select {
  width: 120px;
}

.op-select {
  width: 110px;
}

.value-input {
  flex: 1;
  min-width: 100px;
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
  font-size: 14px;
}

.btn-icon:hover {
  background: var(--color-hover);
  color: var(--color-danger);
}
</style>
