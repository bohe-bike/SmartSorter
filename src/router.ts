import { createRouter, createWebHistory } from "vue-router";

const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: "/",
      name: "workspace",
      component: () => import("./views/WorkspaceView.vue"),
    },
    {
      path: "/duplicate",
      name: "duplicate",
      component: () => import("./views/DuplicateView.vue"),
    },
    {
      path: "/history",
      name: "history",
      component: () => import("./views/HistoryView.vue"),
    },
    {
      path: "/settings",
      name: "settings",
      component: () => import("./views/SettingsView.vue"),
    },
  ],
});

export default router;
