<script setup lang="ts">
import Sidebar from "./components/Sidebar.vue";
</script>

<template>
  <div class="app-layout">
    <Sidebar />
    <main class="main-content">
      <router-view />
    </main>
  </div>
</template>

<style>
:root {
  /* 设计语言 - 亮色主题 */
  --color-bg: #f5f5f7;
  --color-surface: #ffffff;
  --color-sidebar: #fafafa;
  --color-border: #e5e5e5;
  --color-primary: #4f6ef7;
  --color-primary-hover: #3b5de7;
  --color-danger: #e53e3e;
  --color-success: #38a169;
  --color-warning: #d69e2e;
  --color-text: #1a1a1a;
  --color-text-secondary: #888;
  --color-hover: rgba(0, 0, 0, 0.04);
  --color-active: rgba(79, 110, 247, 0.08);

  font-family:
    -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue",
    Arial, "Noto Sans SC", sans-serif;
  font-size: 14px;
  line-height: 1.5;
  color: var(--color-text);
  background-color: var(--color-bg);
}

@media (prefers-color-scheme: dark) {
  :root {
    --color-bg: #1a1a1e;
    --color-surface: #2a2a2e;
    --color-sidebar: #222226;
    --color-border: #3a3a3e;
    --color-text: #e5e5e5;
    --color-text-secondary: #888;
    --color-hover: rgba(255, 255, 255, 0.06);
    --color-active: rgba(79, 110, 247, 0.15);
  }
}

* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

html,
body,
#app {
  height: 100%;
  overflow: hidden;
}
</style>

<style scoped>
.app-layout {
  display: flex;
  height: 100%;
}

.main-content {
  flex: 1;
  overflow: auto;
  background: var(--color-bg);
}
</style>
