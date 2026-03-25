<script setup lang="ts">
import { useRoute } from "vue-router";
import appLogo from "../assets/logo.svg";

const route = useRoute();

const navItems = [
  { path: "/", icon: "📂", label: "整理" },
  { path: "/duplicate", icon: "🔍", label: "去重" },
  { path: "/media-classify", icon: "🎵", label: "归类" },
  { path: "/history", icon: "📋", label: "历史" },
  { path: "/settings", icon: "⚙️", label: "设置" },
];
</script>

<template>
  <nav class="sidebar">
    <img class="logo" :src="appLogo" alt="SmartSorter" />
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
  object-fit: contain;
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
