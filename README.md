# 📁 SmartSorter

**桌面端文件智能整理工具** — 安全、高效地完成海量文件的重命名、去重与自动归类。

基于 Tauri 2 + Vue 3 + Rust 构建，面向本地硬盘及 NAS 用户，提供现代化极简 UI 与「整理前强制预览」机制。

## ✨ 核心功能

- **可视化规则配置** — 零代码拖拽/下拉组合 IF-THEN 整理逻辑，支持预设方案一键加载
- **批量文件重命名** — 查找替换、前后缀管理、智能序号编排
- **智能归类路由** — 魔法变量动态目录（`{extension}/{created_year}/`），自动创建层级
- **重复文件检测** — 文件大小初筛 + SHA-256 哈希复核，分组可视化清理
- **整理前预览** — 内存虚拟执行，红绿 Diff 对比，支持逐文件勾选干预
- **日志与一键撤销** — 操作映射快照，一键还原，失败原因可追溯

## 🏗️ 技术栈

| 层   | 技术                      | 职责                                             |
| ---- | ------------------------- | ------------------------------------------------ |
| 前端 | Vue 3 + TypeScript + Vite | UI 渲染、状态管理 (Pinia)、路由                  |
| 后端 | Rust + Tauri 2            | 文件扫描 (walkdir)、规则引擎、安全 I/O、哈希计算 |
| 通信 | Tauri IPC                 | invoke 请求/响应 + Event 流式进度推送            |

## 📂 项目结构

```
SmartSorter/
├── src/                          # Vue 3 前端
│   ├── components/               # UI 组件（Sidebar 等）
│   ├── views/                    # 页面视图（整理/去重/历史/设置）
│   ├── stores/                   # Pinia 状态管理
│   ├── types/                    # TypeScript 类型定义
│   ├── utils/                    # Tauri API 封装
│   ├── App.vue                   # 根布局
│   ├── main.ts                   # 入口
│   └── router.ts                 # 路由配置
├── src-tauri/                    # Rust 后端
│   ├── src/
│   │   ├── commands/             # Tauri Command（API 层）
│   │   ├── models/               # 数据模型（规则/预览/日志）
│   │   ├── engine/               # 核心引擎（扫描/匹配/执行/哈希/撤销）
│   │   ├── storage/              # 本地持久化
│   │   ├── lib.rs                # 模块注册 & Tauri 启动
│   │   └── main.rs               # Windows 入口
│   ├── Cargo.toml
│   └── tauri.conf.json
├── doc/                          # 产品需求 & 技术设计文档
├── index.html
├── package.json
├── vite.config.ts
└── tsconfig.json
```

## 🚀 快速开始

### 环境要求

- **Rust** >= 1.85 stable（[安装](https://www.rust-lang.org/learn/get-started)）
- **Node.js** >= 20 LTS
- **pnpm** >= 9.x（`npm install -g pnpm`）
- **WebView2**（Windows 10/11 自带）

### 安装 & 运行

```bash
# 安装前端依赖
pnpm install

# 开发模式（前端热重载 + Rust 自动重编译）
pnpm tauri dev

# 构建生产包（.msi 安装包）
pnpm tauri build
```

### 其他命令

```bash
# 仅运行前端（脱离 Tauri 调试 UI）
pnpm dev

# Rust 单元测试
cd src-tauri && cargo test

# TypeScript 类型检查
pnpm vue-tsc --noEmit
```

## 🛡️ 安全设计

- **NAS 安全移动**：跨目录操作采用「复制 → SHA-256 校验 → 删除源文件」策略，防止网络抖动导致数据丢失
- **强制预览**：任何写操作执行前必须通过 Diff 预览确认
- **操作可逆**：每次执行生成映射快照，支持一键撤销

## 📄 许可证

MIT
