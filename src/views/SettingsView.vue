<script setup lang="ts">
import { ref, watch } from "vue";

const theme = ref<"system" | "light" | "dark">(
  (localStorage.getItem("ss-theme") as any) || "system",
);
const defaultConflict = ref(localStorage.getItem("ss-conflict") || "skip");
const confirmDelete = ref(
  localStorage.getItem("ss-confirm-delete") !== "false",
);

watch(theme, (v) => {
  localStorage.setItem("ss-theme", v);
  applyTheme(v);
});

watch(defaultConflict, (v) => {
  localStorage.setItem("ss-conflict", v);
});

watch(confirmDelete, (v) => {
  localStorage.setItem("ss-confirm-delete", String(v));
});

function applyTheme(t: string) {
  const root = document.documentElement;
  root.removeAttribute("data-theme");
  if (t === "light" || t === "dark") {
    root.setAttribute("data-theme", t);
  }
}

// 初始化
applyTheme(theme.value);
</script>

<template>
  <div class="settings-view">
    <h2>设置</h2>

    <section class="settings-group">
      <h3>外观</h3>
      <div class="setting-row">
        <label>主题模式</label>
        <select v-model="theme">
          <option value="system">跟随系统</option>
          <option value="light">浅色</option>
          <option value="dark">深色</option>
        </select>
      </div>
    </section>

    <section class="settings-group">
      <h3>文件操作</h3>
      <div class="setting-row">
        <label>默认冲突策略</label>
        <select v-model="defaultConflict">
          <option value="skip">跳过</option>
          <option value="overwrite">覆盖</option>
          <option value="auto_rename">自动重命名</option>
        </select>
      </div>
      <div class="setting-row">
        <label>删除前确认</label>
        <label class="switch">
          <input type="checkbox" v-model="confirmDelete" />
          <span class="slider"></span>
        </label>
      </div>
    </section>

    <section class="settings-group">
      <h3>关于</h3>
      <div class="about-info">
        <p><strong>SmartSorter</strong> v1.3.0</p>
        <p>桌面端文件智能整理工具</p>
        <p class="tech">Tauri 2 + Vue 3 + Rust</p>
      </div>
    </section>
  </div>
</template>

<style scoped>
.settings-view {
  padding: 16px;
  max-width: 600px;
}

.settings-view h2 {
  font-size: 18px;
  font-weight: 600;
  margin-bottom: 20px;
}

.settings-group {
  background: var(--color-surface);
  border-radius: 8px;
  padding: 16px;
  margin-bottom: 12px;
}

.settings-group h3 {
  font-size: 13px;
  font-weight: 600;
  color: var(--color-text-secondary);
  text-transform: uppercase;
  letter-spacing: 1px;
  margin-bottom: 12px;
}

.setting-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 0;
  border-bottom: 1px solid var(--color-border);
}

.setting-row:last-child {
  border-bottom: none;
}

.setting-row label {
  font-size: 14px;
}

.setting-row select {
  height: 32px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  padding: 0 10px;
  font-size: 13px;
  background: var(--color-bg);
  color: var(--color-text);
  min-width: 140px;
}

/* Toggle switch */
.switch {
  position: relative;
  display: inline-block;
  width: 42px;
  height: 24px;
}

.switch input {
  opacity: 0;
  width: 0;
  height: 0;
}

.slider {
  position: absolute;
  cursor: pointer;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background-color: var(--color-border);
  border-radius: 24px;
  transition: 0.2s;
}

.slider::before {
  content: "";
  position: absolute;
  height: 18px;
  width: 18px;
  left: 3px;
  bottom: 3px;
  background-color: white;
  border-radius: 50%;
  transition: 0.2s;
}

.switch input:checked + .slider {
  background-color: var(--color-primary);
}

.switch input:checked + .slider::before {
  transform: translateX(18px);
}

.about-info {
  font-size: 13px;
  line-height: 1.8;
}

.about-info .tech {
  color: var(--color-text-secondary);
  font-size: 12px;
}
</style>
