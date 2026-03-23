<script setup lang="ts">
import { useRoute } from "vue-router";

const route = useRoute();

const navItems = [
  { path: "/", icon: "📂", label: "整理" },
  { path: "/duplicate", icon: "🔍", label: "去重" },
  { path: "/history", icon: "📋", label: "历史" },
  { path: "/settings", icon: "⚙️", label: "设置" },
];
</script>

<template>
  <nav class="sidebar">
    <div class="logo">SS</div>
    <router-link
      v-for="item in navItems"
      :key="item.path"
      :to="item.path"
      class="nav-item"
      :class="{ active: route.path === item.path }"
    >
      <span class="nav-icon">{{ item.icon }}</span>
      <span class="nav-label">{{ item.label }}</span>
    </router-link>
  </nav>
</template>

<style scoped>
.sidebar {
  width: 64px;
  background: var(--color-sidebar);
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: 12px 0;
  gap: 4px;
  border-right: 1px solid var(--color-border);
}

.logo {
  width: 36px;
  height: 36px;
  border-radius: 8px;
  background: var(--color-primary);
  color: #fff;
  display: flex;
  align-items: center;
  justify-content: center;
  font-weight: 700;
  font-size: 13px;
  margin-bottom: 16px;
}

.nav-item {
  width: 48px;
  height: 48px;
  border-radius: 8px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  text-decoration: none;
  color: var(--color-text-secondary);
  transition:
    background 0.15s,
    color 0.15s;
  gap: 2px;
}

.nav-item:hover {
  background: var(--color-hover);
}

.nav-item.active {
  background: var(--color-active);
  color: var(--color-primary);
}

.nav-icon {
  font-size: 18px;
  line-height: 1;
}

.nav-label {
  font-size: 10px;
  line-height: 1;
}
</style>
