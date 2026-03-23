<script setup lang="ts">
const magicVars = [
  { var: "{filename}", desc: "不含扩展名的文件名" },
  { var: "{extension}", desc: "扩展名（无点号）" },
  { var: "{full_name}", desc: "完整文件名" },
  { var: "{parent_dir}", desc: "父目录名" },
  { var: "{created_year}", desc: "创建年份" },
  { var: "{created_month}", desc: "创建月份" },
  { var: "{created_day}", desc: "创建日期" },
  { var: "{modified_year}", desc: "修改年份" },
  { var: "{modified_month}", desc: "修改月份" },
  { var: "{size_mb}", desc: "文件大小(MB)" },
  { var: "{seq}", desc: "序列号" },
];
</script>

<template>
  <div class="magic-var-ref">
    <div class="ref-title">可用变量</div>
    <div class="var-list">
      <div v-for="v in magicVars" :key="v.var" class="var-item">
        <code class="var-name">{{ v.var }}</code>
        <span class="var-desc">{{ v.desc }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.magic-var-ref {
  border: 1px solid var(--color-border);
  border-radius: 6px;
  padding: 10px;
  background: var(--color-bg);
}

.ref-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--color-text-secondary);
  margin-bottom: 6px;
}

.var-list {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 12px;
}

.var-item {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
}

.var-name {
  background: var(--color-active);
  color: var(--color-primary);
  padding: 1px 5px;
  border-radius: 3px;
  font-size: 11px;
}

.var-desc {
  color: var(--color-text-secondary);
}
</style>
