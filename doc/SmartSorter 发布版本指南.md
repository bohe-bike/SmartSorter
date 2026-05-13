# SmartSorter 发布版本指南

---

## 一、本地编译

在本地机器上编译出可执行文件，适用于快速验证或离线环境。

### 1. 环境要求

| 工具             | 版本       | 安装命令                                   |
| ---------------- | ---------- | ------------------------------------------ |
| Node.js          | ≥ 22       | https://nodejs.org                         |
| pnpm             | 最新       | `npm install -g pnpm`                      |
| Rust             | 最新稳定版 | https://rustup.rs                          |
| Tauri CLI        | ≥ 2.x      | `cargo install tauri-cli --version "^2"`   |
| MSVC Build Tools | 最新       | Visual Studio Installer 安装"C++ 桌面开发" |

### 2. 编译步骤

```powershell
cd D:\MyProjects\SmartSorter

# 第一步：安装前端依赖（首次或依赖变更后执行）
pnpm install

# 第二步：编译前端
pnpm build

# 第三步：编译 Tauri 生产版本
cd src-tauri
cargo tauri build
```

### 3. 产物位置

编译成功后，文件在以下目录：

```
src-tauri/target/release/
├── smart-sorter.exe              ← 独立可执行文件（可直接运行）
└── bundle/
    ├── msi/*.msi                 ← MSI 安装包
    ├── msi/*.msi.sig             ← MSI 更新签名（自动更新必需）
    ├── nsis/*.exe                ← NSIS 安装程序
    └── nsis/*.exe.sig            ← NSIS 更新签名（自动更新必需）
```

| 产物               | 说明                | 适合场景           |
| ------------------ | ------------------- | ------------------ |
| `smart-sorter.exe` | 免安装，双击即用    | 绿色版 / U盘携带   |
| `.msi`             | 标准 Windows 安装包 | 正式安装，支持卸载 |
| `.exe (NSIS)`      | 带向导的安装程序    | 分发给最终用户     |
| `.sig`             | 安装包签名          | Tauri 自动更新校验 |
| `latest.json`      | 更新清单            | 上传到 GitHub Release 根附件 |

### 4. 一键发版（脚本方式）

发布脚本会自动完成：版本号同步、git commit、打 tag。默认不做本地编译（推荐交给 GitHub Actions）。

本地仅打版本（不推送、不本地编译）：

```powershell
.\scripts\release.ps1 -Version "1.2.0"
```

如需发布前在本地先自检构建：

```powershell
.\scripts\release.ps1 -Version "1.2.0" -LocalBuild
```

完成后手动推送：

```powershell
git push
git push origin --delete v1.2.0  # 如果远程已存在同名 tag
git push origin refs/tags/v1.2.0
```

---

## 二、线上自动编译（GitHub Actions）

推送 Tag 后，GitHub 自动在云端编译并创建 Release，附带可下载的安装包、签名文件和 `latest.json` 更新清单。

### 1. 工作原理

```
本地                             GitHub Actions（云端）
 │                                    │
 ├─ 更新版本号                         │
 ├─ git commit                        │
 ├─ git tag v1.2.0                    │
 ├─ 如同名 tag 已存在则删除后重建       │
 ├─ git push origin refs/tags/v1.2.0 ─►│
 │                                    ├─ 检出代码
 │                                    ├─ 安装 Node.js + pnpm + Rust
 │                                    ├─ cargo install tauri-cli
 │                                    ├─ pnpm build（编译前端）
 │                                    ├─ cargo tauri build（编译后端）
 │                                    ├─ 生成 latest.json
 │                                    ├─ 创建 GitHub Release 页面
 │                                    └─ 上传 .exe / .msi / .sig / latest.json 到 Release
 │                                    │
 └─ 在 GitHub Releases 页面下载 ◄────┘
```

### 2. 操作步骤

只需一条命令：

```powershell
.\scripts\release.ps1 -Version "1.2.0" -Push
```

| 参数               | 作用               |
| ------------------ | ------------------ |
| `-Version "1.2.0"` | 设置版本号         |
| `-Push`            | 自动推送代码和 Tag |

### 3. 查看进度和结果

1. 打开 GitHub 仓库 → **Actions** 标签页 → 查看构建进度
2. 首次构建约 10-15 分钟（安装依赖+编译），后续约 5 分钟（有缓存）
3. 构建完成后进入 **Releases** 页面，即可看到自动创建的 Release 及附件下载
4. Release 附件必须包含 `latest.json`；否则客户端检查更新会请求失败：
   `https://github.com/bohe-bike/SmartSorter/releases/latest/download/latest.json`

### 4. 工作流配置文件

位于 `.github/workflows/release.yml`，触发条件为推送 `v*` 格式的 Tag。

---

## 三、release.ps1 完整参数

```powershell
.\scripts\release.ps1 -Version <版本号> [-Message <说明>] [-Push] [-LocalBuild]
```

| 参数          | 必填 | 默认值            | 说明                                          |
| ------------- | ---- | ----------------- | --------------------------------------------- |
| `-Version`    | 是   | —                 | 版本号 `X.Y.Z`，如 `1.2.0`                    |
| `-Message`    | 否   | `release: vX.Y.Z` | commit 和 tag 的说明文字                      |
| `-Push`       | 否   | 不推送            | 自动 `git push` + `git push --tags`           |
| `-LocalBuild` | 否   | 不执行本地编译    | 先本地执行 `pnpm build` + `cargo tauri build` |

脚本会自动同步以下三个文件中的版本号：

- `package.json`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`

---

## 四、常用发布场景

### 场景 A：本地编译 + 手动上传

```powershell
# 编译 + 打 tag
.\scripts\release.ps1 -Version "1.2.0" -LocalBuild

# 推送到 GitHub
git push
git push origin --delete v1.2.0  # 如果远程已存在同名 tag
git push origin refs/tags/v1.2.0

# 然后去 GitHub Releases 页面手动上传 exe/msi
```

### 场景 B：云端自动编译发布（推荐）

```powershell
# 一条命令搞定
.\scripts\release.ps1 -Version "1.2.0" -Push

# 等待 GitHub Actions 自动编译，完成后 Release 页面自动出现下载链接
```

### 场景 C：本地编译验证 + 云端发布

```powershell
# 先本地编译确认没问题
.\scripts\release.ps1 -Version "1.2.0" -LocalBuild

# 确认产物正常后推送，云端也会再编译一份上传到 Release
git push
git push origin --delete v1.2.0  # 如果远程已存在同名 tag
git push origin refs/tags/v1.2.0
```

---

## 五、版本号规范

采用 [语义化版本](https://semver.org/lang/zh-CN/) `主版本.次版本.修订号`：

- **主版本**（1.x.x → 2.0.0）：不兼容的重大变更
- **次版本**（1.1.x → 1.2.0）：新增功能，向下兼容
- **修订号**（1.1.0 → 1.1.1）：Bug 修复

---

## 六、注意事项

1. 发布前请更新 `CHANGELOG.md`，脚本会自动将其纳入 commit
2. 自动更新必须在 GitHub 仓库 Secrets 中配置 `TAURI_SIGNING_PRIVATE_KEY`，如私钥有密码还需配置 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
3. `TAURI_SIGNING_PRIVATE_KEY` 必须与 `src-tauri/tauri.conf.json` 中的 updater `pubkey` 对应
4. 首次云端构建较慢（10-15 分钟），后续有 Rust 缓存会快很多
5. 确保 Git 远程仓库已配置：`git remote -v` 查看
6. 如遇网络问题推送失败，配置 Git 代理：
   ```powershell
   git config --global http.proxy http://127.0.0.1:7890
   git config --global https.proxy http://127.0.0.1:7890
   ```
7. 数据文件存储位置：`C:\Users\<用户名>\AppData\Roaming\com.smartsorter.app\`
