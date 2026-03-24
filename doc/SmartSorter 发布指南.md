# SmartSorter 发布指南

## 概述

SmartSorter 提供两种发布方式，均通过 `scripts/release.ps1` 脚本驱动：

| 方式             | 适用场景           | 编译位置       | Release 创建      |
| ---------------- | ------------------ | -------------- | ----------------- |
| **本地编译发布** | 快速验证、离线环境 | 本地机器       | 手动上传到 GitHub |
| **云端自动发布** | 正式发布（推荐）   | GitHub Actions | 自动创建并上传    |

---

## 前置条件

- **Node.js** ≥ 22 + **pnpm**
- **Rust** + **cargo** + **cargo-tauri**（`cargo install tauri-cli --version "^2"`）
- **Git** 已初始化且配置了远程仓库
- MSVC Build Tools（Windows 编译 Tauri 需要）

---

## 方式一：本地编译发布

本地完成编译，生成可执行文件后打 Tag。

### 步骤

```powershell
# 1. 编译 + 更新版本号 + commit + 打 tag（一条命令）
.\scripts\release.ps1 -Version "1.1.0"

# 2. 推送代码和 tag 到 GitHub
git push SmartSorter main --tags
```

### 自定义选项

```powershell
# 自定义提交说明
.\scripts\release.ps1 -Version "1.1.0" -Message "feat: 新增批量操作"

# 编译 + 打 tag + 自动推送（全自动）
.\scripts\release.ps1 -Version "1.1.0" -Push
```

### 产物位置

编译完成后，可执行文件在以下位置：

| 文件            | 路径                                       | 说明           |
| --------------- | ------------------------------------------ | -------------- |
| SmartSorter.exe | `src-tauri\target\release\SmartSorter.exe` | 独立可执行文件 |
| \*.msi          | `src-tauri\target\release\bundle\msi\`     | MSI 安装包     |
| \*.exe          | `src-tauri\target\release\bundle\nsis\`    | NSIS 安装程序  |

### 手动上传到 GitHub Release

1. 打开 GitHub 仓库 → Releases → 找到对应 Tag
2. 点击 **Edit** → 拖拽上传上述文件 → **Publish**

---

## 方式二：云端自动发布（推荐）

只需推送 Tag，GitHub Actions 自动完成编译 + 创建 Release + 上传产物。

### 步骤

```powershell
# 跳过本地编译，只更新版本号 + commit + 打 tag + 推送
.\scripts\release.ps1 -Version "1.1.0" -SkipBuild -Push
```

一条命令即可，后续全部由云端自动完成。

### 工作流程

```
本地 release.ps1             GitHub Actions
     │                           │
     ├─ 更新版本号                │
     ├─ git commit               │
     ├─ git tag v1.1.0           │
     ├─ git push --tags ────────►│
     │                           ├─ 检出代码
     │                           ├─ 安装 Node.js + pnpm
     │                           ├─ 安装 Rust
     │                           ├─ pnpm build（前端）
     │                           ├─ cargo tauri build（Tauri）
     │                           ├─ 创建 GitHub Release
     │                           └─ 上传 .exe / .msi 到 Release
```

### 查看发布结果

1. 推送后打开 GitHub 仓库 → **Actions** 标签页，查看构建进度
2. 构建完成后，进入 **Releases** 页面即可看到自动创建的 Release 及附件

---

## release.ps1 参数一览

| 参数         | 必填 | 默认值            | 说明                                    |
| ------------ | ---- | ----------------- | --------------------------------------- |
| `-Version`   | 是   | —                 | 版本号，格式 `X.Y.Z`                    |
| `-Message`   | 否   | `release: vX.Y.Z` | Commit 和 Tag 的说明文字                |
| `-Push`      | 否   | `$false`          | 自动执行 `git push` + `git push --tags` |
| `-SkipBuild` | 否   | `$false`          | 跳过本地编译（搭配云端发布使用）        |

### 常用组合

```powershell
# 完整本地发布（编译 + tag）
.\scripts\release.ps1 -Version "1.1.0"

# 云端发布（推荐，最省事）
.\scripts\release.ps1 -Version "1.1.0" -SkipBuild -Push

# 本地编译 + 自动推送触发云端 Release
.\scripts\release.ps1 -Version "1.1.0" -Push
```

---

## 版本号规范

采用 [语义化版本](https://semver.org/lang/zh-CN/) `主版本.次版本.修订号`：

- **主版本**：不兼容的重大变更
- **次版本**：向下兼容的新功能
- **修订号**：向下兼容的问题修复

发布脚本会自动同步三个文件中的版本号：

- `package.json`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`

---

## 更新日志

每次发布前，请在 `CHANGELOG.md` 中记录本次变更内容，脚本会自动将其纳入 commit。
