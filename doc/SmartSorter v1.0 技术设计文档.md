# 🏗️ SmartSorter v1.0 技术设计文档

**文档版本：** v1.0
**创建日期：** 2026-03-23
**关联文档：** SmartSorter v1.0 产品需求文档.md

---

## 一、 核心数据结构设计（前后端契约）

> 以下 JSON Schema 是前端 Vue 3 与后端 Rust（通过 Tauri Command）之间通信的核心契约。
> 前端通过 `invoke()` 将 JSON 传入 Rust，Rust 通过 `serde` 反序列化为对应 struct。

---

### 1.1 规则系统 (Rule System)

#### 1.1.1 完整规则集 `RuleSet`

一个"预设方案"就是一个 `RuleSet`，内部包含多条有序规则。

```jsonc
// RuleSet — 规则预设方案
{
  "id": "rs_a1b2c3d4", // 唯一ID（UUID v4 短格式）
  "name": "清理桌面截图", // 方案显示名
  "description": "把桌面上的截图按日期归档到 D:\\归档", // 可选描述
  "created_at": "2026-03-23T10:30:00Z",
  "updated_at": "2026-03-23T14:20:00Z",
  "rules": [
    // 有序规则数组，按顺序执行
    {
      /* Rule 对象，见下文 */
    },
  ],
}
```

#### 1.1.2 单条规则 `Rule`

每条规则由 **条件组 (Condition Group)** + **动作列表 (Actions)** 组成，即 `IF ... THEN ...` 语义。

```jsonc
// Rule — 单条整理规则
{
  "id": "rule_x1y2z3",
  "enabled": true, // 可临时禁用
  "name": "截图归档", // 规则显示名
  "condition_group": {
    // 条件组（顶层 AND/OR）
    "logic": "AND", // "AND" | "OR"
    "conditions": [
      {
        "field": "extension", // 条件字段
        "operator": "equals", // 操作符
        "value": "png", // 比对值
      },
      {
        "field": "filename",
        "operator": "contains",
        "value": "截图",
      },
    ],
    "sub_groups": [], // 可嵌套子条件组，实现复杂逻辑
  },
  "actions": [
    // 按顺序执行的动作列表
    {
      "type": "rename",
      "params": {
        "mode": "replace", // "replace" | "prefix" | "suffix" | "sequence"
        "find": "截图",
        "replace": "screenshot",
      },
    },
    {
      "type": "move",
      "params": {
        "dest_pattern": "D:\\归档\\{extension}\\{created_year}\\{created_month}\\",
        "conflict_strategy": "auto_rename", // "skip" | "overwrite" | "auto_rename"
      },
    },
  ],
}
```

#### 1.1.3 条件字段与操作符枚举

| `field` 字段  | 说明                   | 适用 `operator`                                                           |
| ------------- | ---------------------- | ------------------------------------------------------------------------- |
| `filename`    | 文件名（不含扩展名）   | `contains`, `not_contains`, `starts_with`, `ends_with`, `equals`, `regex` |
| `extension`   | 扩展名（如 `png`）     | `equals`, `not_equals`, `in`, `not_in`                                    |
| `full_name`   | 完整文件名（含扩展名） | 同 `filename`                                                             |
| `size_bytes`  | 文件大小（字节）       | `gt`, `gte`, `lt`, `lte`, `between`                                       |
| `created_at`  | 创建时间               | `before`, `after`, `between`, `within_days`                               |
| `modified_at` | 修改时间               | 同 `created_at`                                                           |
| `parent_dir`  | 所在父目录名           | `contains`, `equals`, `starts_with`                                       |

#### 1.1.4 动作类型与参数枚举

| `type` 动作 | `params` 参数说明                                                                                                    |
| ----------- | -------------------------------------------------------------------------------------------------------------------- |
| `rename`    | `mode`: `replace`(查找替换), `prefix`(加前缀), `suffix`(加后缀), `sequence`(智能编号)                                |
|             | `replace` 模式: `{ find, replace }`                                                                                  |
|             | `prefix` 模式: `{ text: "2023_" }`                                                                                   |
|             | `suffix` 模式: `{ text: "_备份" }`                                                                                   |
|             | `sequence` 模式: `{ template: "{original}_{seq}", start: 1, padding: 3, sort_by: "modified_at", sort_order: "asc" }` |
| `move`      | `dest_pattern`: 目标路径模板, `conflict_strategy`: 冲突策略                                                          |
| `copy`      | 同 `move`                                                                                                            |
| `delete`    | `confirm_required`: `true`（强制二次确认）                                                                           |

#### 1.1.5 魔法变量（路径模板可用）

| 变量               | 说明                        | 示例值        |
| ------------------ | --------------------------- | ------------- |
| `{filename}`       | 原文件名（不含扩展名）      | `截图001`     |
| `{extension}`      | 扩展名                      | `png`         |
| `{full_name}`      | 完整文件名                  | `截图001.png` |
| `{created_year}`   | 创建年份                    | `2026`        |
| `{created_month}`  | 创建月份（补零）            | `03`          |
| `{created_day}`    | 创建日期（补零）            | `23`          |
| `{modified_year}`  | 修改年份                    | `2026`        |
| `{modified_month}` | 修改月份（补零）            | `03`          |
| `{parent_dir}`     | 直属父目录名                | `Desktop`     |
| `{size_mb}`        | 文件大小（MB，保留1位小数） | `2.3`         |
| `{seq}`            | 序号（仅在 sequence 模式）  | `001`         |

---

### 1.2 预览树 (Preview Tree)

#### 1.2.1 预览请求 `PreviewRequest`

前端发起分析请求时传入的参数：

```jsonc
// 前端 -> Rust: invoke("analyze_preview", payload)
{
  "source_paths": [
    // 待整理的源目录/文件列表
    "C:\\Users\\xxx\\Desktop",
    "Z:\\NAS共享\\电影",
  ],
  "rule_set_id": "rs_a1b2c3d4", // 使用的规则方案 ID
  "recursive": true, // 是否递归子目录
  "max_depth": null, // 递归深度限制，null 为不限
}
```

#### 1.2.2 预览结果 `PreviewResult`

Rust 内存计算后返回的完整 Diff 结果：

```jsonc
// Rust -> 前端: PreviewResult
{
  "task_id": "task_20260323_143000_abcd", // 任务唯一ID
  "generated_at": "2026-03-23T14:30:05Z",
  "summary": {
    "total_scanned": 12580, // 扫描文件总数
    "matched": 342, // 命中规则的文件数
    "to_rename": 210, // 将被重命名
    "to_move": 280, // 将被移动
    "to_copy": 0, // 将被复制
    "to_delete": 12, // 将被删除
    "conflicts": 5, // 检测到的冲突数
    "errors": 1, // 预分析阶段的错误数
  },
  "items": [
    // 逐文件预览项
    {
      /* PreviewItem 对象，见下文 */
    },
  ],
  "errors": [
    // 全局错误列表
    {
      "path": "Z:\\NAS共享\\电影\\损坏文件.avi",
      "error": "PERMISSION_DENIED",
      "message": "无法读取文件元数据：权限不足",
    },
  ],
}
```

#### 1.2.3 单文件预览项 `PreviewItem`

```jsonc
// PreviewItem — 每个文件的预览 Diff
{
  "id": "pi_001", // 预览项ID（用于前端勾选交互）
  "checked": true, // 是否参与本次执行（用户可取消勾选）
  "source": {
    // 原始状态
    "path": "C:\\Users\\xxx\\Desktop\\截图001.png",
    "name": "截图001.png",
    "size_bytes": 2415700,
    "created_at": "2026-03-20T09:15:00Z",
    "modified_at": "2026-03-20T09:15:00Z",
  },
  "target": {
    // 预计新状态（可能有多个变化叠加）
    "path": "D:\\归档\\png\\2026\\03\\screenshot001.png",
    "name": "screenshot001.png",
  },
  "changes": [
    // 命中的规则及产生的变更明细
    {
      "rule_id": "rule_x1y2z3",
      "rule_name": "截图归档",
      "action_type": "rename",
      "description": "\"截图001\" → \"screenshot001\"",
    },
    {
      "rule_id": "rule_x1y2z3",
      "rule_name": "截图归档",
      "action_type": "move",
      "description": "移动至 D:\\归档\\png\\2026\\03\\",
    },
  ],
  "conflict": null, // 若有冲突，包含冲突详情
}
```

#### 1.2.4 冲突详情 `Conflict`

```jsonc
// 当目标路径已存在同名文件时
{
  "type": "name_collision", // "name_collision" | "path_too_long" | "invalid_chars"
  "existing_file": {
    "path": "D:\\归档\\png\\2026\\03\\screenshot001.png",
    "size_bytes": 1024000,
    "modified_at": "2026-02-10T08:00:00Z",
  },
  "resolution": "auto_rename", // 按规则中的冲突策略自动填充
  "resolved_path": "D:\\归档\\png\\2026\\03\\screenshot001_(1).png",
}
```

---

### 1.3 重复文件检测 (Duplicate Finder)

#### 1.3.1 去重扫描结果 `DuplicateResult`

```jsonc
{
  "task_id": "dup_20260323_150000",
  "scanned_count": 8500,
  "total_groups": 120, // 发现 120 组重复
  "total_wasted_bytes": 5368709120, // 可释放空间（字节）
  "groups": [
    {
      "group_id": "dg_001",
      "hash": "sha256:a1b2c3d4e5f6...",
      "file_size": 4521984,
      "files": [
        {
          "path": "D:\\照片\\IMG_001.jpg",
          "created_at": "2025-06-15T08:00:00Z",
          "modified_at": "2025-06-15T08:00:00Z",
          "keep": true, // 前端默认标记保留（最早/最新）
        },
        {
          "path": "D:\\照片\\副本\\IMG_001(1).jpg",
          "created_at": "2025-12-01T10:00:00Z",
          "modified_at": "2025-12-01T10:00:00Z",
          "keep": false, // 标记为待删除候选
        },
      ],
    },
  ],
}
```

---

### 1.4 操作日志与撤销 (Log & Undo)

#### 1.4.1 执行日志 `ExecutionLog`

```jsonc
{
  "log_id": "log_20260323_160000",
  "task_id": "task_20260323_143000_abcd",
  "rule_set_name": "清理桌面截图",
  "executed_at": "2026-03-23T16:00:00Z",
  "duration_ms": 3420, // 总耗时
  "summary": {
    "total": 337,
    "succeeded": 335,
    "failed": 2,
    "skipped": 5, // 用户取消勾选的
  },
  "operations": [
    // 逐条操作映射（撤销核心数据）
    {
      "op_id": "op_001",
      "action": "move", // "rename" | "move" | "copy" | "delete"
      "source_path": "C:\\Users\\xxx\\Desktop\\截图001.png",
      "target_path": "D:\\归档\\png\\2026\\03\\screenshot001.png",
      "status": "success", // "success" | "failed"
      "error_message": null,
      "reversible": true, // 是否可撤销
    },
  ],
  "undo_status": "available", // "available" | "partial" | "expired"
}
```

---

### 1.5 Rust 后端 Struct 对应（Serde 映射参考）

```rust
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

// ========== 规则系统 ==========

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleSet {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub enabled: bool,
    pub name: String,
    pub condition_group: ConditionGroup,
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionGroup {
    pub logic: Logic,
    pub conditions: Vec<Condition>,
    pub sub_groups: Vec<ConditionGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Logic {
    And,
    Or,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Condition {
    pub field: ConditionField,
    pub operator: Operator,
    pub value: serde_json::Value, // 灵活承接 string / number / array
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionField {
    Filename,
    Extension,
    FullName,
    SizeBytes,
    CreatedAt,
    ModifiedAt,
    ParentDir,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operator {
    Equals, NotEquals,
    Contains, NotContains,
    StartsWith, EndsWith,
    Regex,
    In, NotIn,
    Gt, Gte, Lt, Lte,
    Between,
    Before, After,
    WithinDays,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "params")]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Rename(RenameParams),
    Move(RouteParams),
    Copy(RouteParams),
    Delete(DeleteParams),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameParams {
    pub mode: RenameMode,
    #[serde(flatten)]
    pub detail: serde_json::Value, // 根据 mode 动态解析
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenameMode {
    Replace,
    Prefix,
    Suffix,
    Sequence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteParams {
    pub dest_pattern: String,
    pub conflict_strategy: ConflictStrategy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictStrategy {
    Skip,
    Overwrite,
    AutoRename,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteParams {
    pub confirm_required: bool,
}

// ========== 预览系统 ==========

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewRequest {
    pub source_paths: Vec<String>,
    pub rule_set_id: String,
    pub recursive: bool,
    pub max_depth: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewResult {
    pub task_id: String,
    pub generated_at: DateTime<Utc>,
    pub summary: PreviewSummary,
    pub items: Vec<PreviewItem>,
    pub errors: Vec<PreviewError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewSummary {
    pub total_scanned: u64,
    pub matched: u64,
    pub to_rename: u64,
    pub to_move: u64,
    pub to_copy: u64,
    pub to_delete: u64,
    pub conflicts: u64,
    pub errors: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewItem {
    pub id: String,
    pub checked: bool,
    pub source: FileSnapshot,
    pub target: FileTarget,
    pub changes: Vec<ChangeDetail>,
    pub conflict: Option<Conflict>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSnapshot {
    pub path: String,
    pub name: String,
    pub size_bytes: u64,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTarget {
    pub path: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeDetail {
    pub rule_id: String,
    pub rule_name: String,
    pub action_type: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    pub conflict_type: String,
    pub existing_file: Option<FileSnapshot>,
    pub resolution: ConflictStrategy,
    pub resolved_path: String,
}
```

---

## 二、 UI 线框图构思

### 2.1 整体布局架构

采用经典的 **侧边导航 + 主工作区** 布局，参考 VS Code / Raycast 的极简设计语言。

```
┌─────────────────────────────────────────────────────────────────┐
│  SmartSorter                              ─  □  ×  │ ← 顶部标题栏 (Tauri 自定义)
├──────────┬──────────────────────────────────────────────────────┤
│          │                                                      │
│  📂 整理  │         ← 主工作区（根据左侧导航切换内容）           │
│          │                                                      │
│  🔍 去重  │                                                      │
│          │                                                      │
│  📋 历史  │                                                      │
│          │                                                      │
│  ⚙️ 设置  │                                                      │
│          │                                                      │
│          │                                                      │
│          │                                                      │
└──────────┴──────────────────────────────────────────────────────┘
   60px                        剩余宽度
```

### 2.2 核心页面：整理工作台 (Main Workspace)

整理工作台采用 **上下三分** 布局：

```
┌─────────────────────────────────────────────────────────────────┐
│ ① 数据源区域                                                    │
│ ┌───────────────────────────────────────────────────────────┐   │
│ │  📂 C:\Users\xxx\Desktop          [✕]                     │   │
│ │  📂 Z:\NAS共享\电影               [✕]                     │   │
│ │                                                           │   │
│ │     + 拖拽文件夹到此处，或 [点击选择]                       │   │
│ └───────────────────────────────────────────────────────────┘   │
├─────────────────────────────────────────────────────────────────┤
│ ② 规则配置区域                                                   │
│ ┌───────────────────────────────────────────────────────────┐   │
│ │  方案: [清理桌面截图 ▾]          [+ 新建规则] [💾 保存方案]  │   │
│ │ ┌─────────────────────────────────────────────────────┐   │   │
│ │ │ 规则 1: 截图归档                              [☑][✕] │   │   │
│ │ │ ┌─ IF ─────────────────────────────────────────┐    │   │   │
│ │ │ │ [扩展名 ▾] [等于 ▾] [png        ]  [+ 条件]  │    │   │   │
│ │ │ │    AND                                        │    │   │   │
│ │ │ │ [文件名 ▾] [包含 ▾] [截图       ]            │    │   │   │
│ │ │ └──────────────────────────────────────────────┘    │   │   │
│ │ │ ┌─ THEN ──────────────────────────────────────┐    │   │   │
│ │ │ │ ① [重命名 ▾]  查找[截图] 替换为[screenshot]  │    │   │   │
│ │ │ │ ② [移动至 ▾]  路径: D:\归档\{extension}\...  │    │   │   │
│ │ │ │                冲突: [自动重命名 ▾]           │    │   │   │
│ │ │ │                          [+ 添加动作]         │    │   │   │
│ │ │ └──────────────────────────────────────────────┘    │   │   │
│ │ └─────────────────────────────────────────────────────┘   │   │
│ └───────────────────────────────────────────────────────────┘   │
│                                                                  │
│              [🔍 分析预览]                                       │
├─────────────────────────────────────────────────────────────────┤
│ ③ 预览结果区域 (Diff View)                                       │
│ ┌───────────────────────────────────────────────────────────┐   │
│ │ 📊 扫描 12,580 文件 | 命中 342 | 重命名 210 | 移动 280    │   │
│ │━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━│   │
│ │ ☑ │ 原文件                        │ 新状态               │   │
│ │───┼───────────────────────────────┼──────────────────────│   │
│ │ ☑ │ 🔴 Desktop\截图001.png        │ 🟢 归档\...\screen…  │   │
│ │ ☑ │ 🔴 Desktop\截图002.png        │ 🟢 归档\...\screen…  │   │
│ │ ☐ │ 🔴 Desktop\截图003.png        │ 🟢 归档\...\screen…  │   │ ← 用户手动取消
│ │ ☑ │ 🔴 Desktop\截图004.png        │ 🟡 归档\...\screen…  │   │ ← 黄色=有冲突
│ │ ...                                                       │   │
│ └───────────────────────────────────────────────────────────┘   │
│                                                                  │
│              [🚀 开始整理]              进度: ████████░░ 80%     │
└─────────────────────────────────────────────────────────────────┘
```

### 2.3 规则配置器 — 交互细则

**条件块 (IF)：**

- 每行一个条件，由三个下拉框组成：`[字段] [操作符] [输入值]`
- 条件之间有 `AND` / `OR` 切换 Toggle
- 点击 `[+ 条件]` 追加新行
- 支持嵌套子条件组（缩进显示，外层用卡片包裹）

**动作块 (THEN)：**

- 每个动作一行，带序号标识执行顺序
- 第一个下拉框选择动作类型（重命名/移动/复制/删除）
- 动作类型切换后，右侧参数区域动态切换对应表单
- 路径输入框支持输入魔法变量 `{xxx}`，带自动补全下拉

**规则卡片：**

- 每条规则是一个可折叠的 Card 组件
- 右上角有 `☑ 启用/禁用` 和 `✕ 删除` 按钮
- 多条规则之间可拖拽排序

### 2.4 预览树 — 交互细则

**列表模式（默认）：**

- 虚拟列表（Virtual Scroll），支持 10k+ 条目不卡顿
- 每行：`[☑勾选][原文件路径(红)][→][新状态(绿)][变更标签]`
- 行内展开（Accordion）可看到该文件命中的具体规则和变更明细
- 冲突行用黄色标记，点击可展开冲突详情及解决方案

**筛选工具栏：**

- 按变更类型筛选：`全部` | `重命名` | `移动` | `复制` | `删除` | `有冲突`
- 搜索框：实时过滤文件名

**统计横条：**

- 固定在预览区顶部，实时显示汇总数据
- 数字使用色彩编码：绿色=命中，红色=错误，黄色=冲突

### 2.5 去重页面

```
┌─────────────────────────────────────────────────────────────────┐
│  选择扫描目录: [D:\照片              ] [📂] [🔍 开始扫描]       │
├─────────────────────────────────────────────────────────────────┤
│  📊 扫描 8,500 文件 | 发现 120 组重复 | 可释放 5.0 GB           │
│━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━│
│  快捷操作: [保留最新] [保留最旧] [展开全部]                      │
│                                                                  │
│  ▼ 重复组 #1 (4.3 MB × 2 个文件)                    SHA256: a1… │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │  🟢 保留  D:\照片\IMG_001.jpg         2025-06-15  4.3MB │   │
│  │  🔴 删除  D:\照片\副本\IMG_001(1).jpg 2025-12-01  4.3MB │   │
│  └──────────────────────────────────────────────────────────┘   │
│                                                                  │
│  ▶ 重复组 #2 (12.1 MB × 3 个文件)                               │
│  ▶ 重复组 #3 (890 KB × 2 个文件)                                │
│  …                                                               │
│                                                                  │
│              [🗑️ 清理选中的重复文件]                              │
└─────────────────────────────────────────────────────────────────┘
```

### 2.6 历史与撤销页面

```
┌─────────────────────────────────────────────────────────────────┐
│  操作历史                                          [清除历史 ▾] │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  📋 2026-03-23 16:00  "清理桌面截图"                             │
│     成功 335 | 失败 2 | 跳过 5 | 耗时 3.4s                      │
│     [↩️ 撤销] [📄 查看详情]                                     │
│                                                                  │
│  📋 2026-03-22 10:30  "归档NAS电影"                              │
│     成功 1,205 | 失败 0 | 跳过 12 | 耗时 45.2s                  │
│     [⚠️ 部分可撤销] [📄 查看详情]                                │
│                                                                  │
│  📋 2026-03-20 09:00  "去重 - D:\照片"                           │
│     删除 48 个文件 | 释放 2.1 GB                                 │
│     [❌ 不可撤销（已删除）] [📄 查看详情]                         │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## 三、 技术栈搭建指南

### 3.1 环境准备

| 工具             | 版本要求                    | 安装方式                           |
| ---------------- | --------------------------- | ---------------------------------- |
| **Rust**         | >= 1.75 (stable)            | `winget install Rustlang.Rustup`   |
| **Node.js**      | >= 20 LTS                   | `winget install OpenJS.NodeJS.LTS` |
| **pnpm**         | >= 9.x                      | `npm install -g pnpm`              |
| **VS Code** 插件 | rust-analyzer, Volar, Tauri | VS Code 扩展面板安装               |
| **系统依赖**     | WebView2 (Win10/11 自带)    | 通常无需额外安装                   |

### 3.2 项目初始化

```powershell
# 1. 安装 Tauri CLI
cargo install create-tauri-app
cargo install tauri-cli --version "^2"

# 2. 创建项目（交互式选择）
cd D:\MyProjects\SmartSorter
cargo create-tauri-app . --template vue-ts

# 交互选项选择：
#   Frontend: Vue
#   Language: TypeScript
#   Package Manager: pnpm
```

### 3.3 预期项目结构

```
SmartSorter/
├── doc/                             # 文档目录（已有）
│   ├── SmartSorter v1.0 产品需求文档.md
│   └── SmartSorter v1.0 技术设计文档.md
│
├── src/                             # Vue 3 前端源码
│   ├── assets/                      # 静态资源
│   ├── components/                  # 通用组件
│   │   ├── RuleBuilder/             # 规则配置器组件族
│   │   │   ├── RuleCard.vue         #   单条规则卡片
│   │   │   ├── ConditionRow.vue     #   条件行组件
│   │   │   ├── ActionRow.vue        #   动作行组件
│   │   │   └── MagicVarInput.vue    #   魔法变量输入框
│   │   ├── PreviewTree/             # 预览树组件族
│   │   │   ├── PreviewTable.vue     #   主列表（Virtual Scroll）
│   │   │   ├── PreviewRow.vue       #   单行 Diff 展示
│   │   │   ├── SummaryBar.vue       #   统计横条
│   │   │   └── ConflictDetail.vue   #   冲突详情弹窗
│   │   ├── DuplicateFinder/         # 去重组件
│   │   └── HistoryLog/              # 历史日志组件
│   ├── views/                       # 页面视图
│   │   ├── WorkspaceView.vue        #   整理工作台
│   │   ├── DuplicateView.vue        #   去重页面
│   │   ├── HistoryView.vue          #   历史记录
│   │   └── SettingsView.vue         #   设置页面
│   ├── stores/                      # Pinia 状态管理
│   │   ├── ruleStore.ts             #   规则方案状态
│   │   ├── previewStore.ts          #   预览结果状态
│   │   └── historyStore.ts          #   历史日志状态
│   ├── types/                       # TypeScript 类型定义
│   │   └── index.ts                 #   与 Rust 对齐的类型
│   ├── utils/                       # 工具函数
│   │   └── tauriApi.ts              #   Tauri invoke 封装
│   ├── App.vue
│   ├── main.ts
│   └── router.ts                    # Vue Router
│
├── src-tauri/                       # Rust 后端源码
│   ├── Cargo.toml                   # Rust 依赖配置
│   ├── tauri.conf.json              # Tauri 配置
│   ├── src/
│   │   ├── main.rs                  # 入口
│   │   ├── lib.rs                   # 模块注册
│   │   ├── commands/                # Tauri Command 层（API 网关）
│   │   │   ├── mod.rs
│   │   │   ├── rule_commands.rs     #   规则 CRUD
│   │   │   ├── preview_commands.rs  #   分析预览
│   │   │   ├── execute_commands.rs  #   物理执行
│   │   │   ├── duplicate_commands.rs#   去重扫描
│   │   │   └── history_commands.rs  #   日志与撤销
│   │   ├── models/                  # 数据模型（struct 定义）
│   │   │   ├── mod.rs
│   │   │   ├── rule.rs              #   Rule / RuleSet / Condition / Action
│   │   │   ├── preview.rs           #   PreviewResult / PreviewItem
│   │   │   ├── duplicate.rs         #   DuplicateResult / DuplicateGroup
│   │   │   └── log.rs               #   ExecutionLog / Operation
│   │   ├── engine/                  # 核心业务引擎
│   │   │   ├── mod.rs
│   │   │   ├── scanner.rs           #   文件扫描（walkdir）
│   │   │   ├── matcher.rs           #   条件匹配引擎
│   │   │   ├── transformer.rs       #   重命名 / 路径计算
│   │   │   ├── executor.rs          #   物理 I/O 执行器
│   │   │   ├── hasher.rs            #   文件哈希计算
│   │   │   └── undo.rs              #   撤销引擎
│   │   └── storage/                 # 本地持久化
│   │       ├── mod.rs
│   │       ├── rule_store.rs        #   规则方案存储（JSON 文件）
│   │       └── log_store.rs         #   日志存储
│   └── icons/                       # 应用图标
│
├── index.html
├── package.json
├── pnpm-lock.yaml
├── tsconfig.json
├── vite.config.ts
└── README.md
```

### 3.4 核心依赖

#### Rust (Cargo.toml)

```toml
[dependencies]
tauri = { version = "2", features = ["dialog", "fs", "path", "shell"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1", features = ["v4"] }
walkdir = "2"                     # 高速目录遍历
sha2 = "0.10"                     # SHA-256 哈希
md-5 = "0.10"                     # MD5（可选快速初筛）
rayon = "1.10"                    # 并行迭代器
regex = "1"                       # 正则匹配
glob = "0.3"                      # Glob 模式
tokio = { version = "1", features = ["full"] }  # 异步运行时
log = "0.4"
env_logger = "0.11"
thiserror = "2"                   # 错误处理
```

#### 前端 (package.json)

```json
{
  "dependencies": {
    "vue": "^3.5",
    "vue-router": "^4.5",
    "pinia": "^2.3",
    "@tauri-apps/api": "^2",
    "@tauri-apps/plugin-dialog": "^2",
    "@tauri-apps/plugin-fs": "^2"
  },
  "devDependencies": {
    "@vitejs/plugin-vue": "^5",
    "typescript": "^5.7",
    "vite": "^6",
    "@tauri-apps/cli": "^2",
    "sass": "^1.83",
    "@vueuse/core": "^12"
  }
}
```

### 3.5 关键技术方案

#### A. 前后端通信模式

```typescript
// 前端: src/utils/tauriApi.ts
import { invoke } from "@tauri-apps/api/core";

// 分析预览 — 请求/响应模式
export async function analyzePreview(
  request: PreviewRequest,
): Promise<PreviewResult> {
  return invoke<PreviewResult>("analyze_preview", { request });
}

// 物理执行 — 流式进度推送（Tauri Event）
import { listen } from "@tauri-apps/api/event";

export function executeWithProgress(
  taskId: string,
  onProgress: (progress: ProgressEvent) => void,
) {
  // 监听后端推送的进度事件
  const unlisten = listen<ProgressEvent>("execute-progress", (event) => {
    onProgress(event.payload);
  });

  // 触发执行
  invoke("execute_task", { taskId });

  return unlisten;
}
```

```rust
// 后端: src-tauri/src/commands/execute_commands.rs
use tauri::{command, AppHandle, Emitter};

#[command]
pub async fn execute_task(app: AppHandle, task_id: String) -> Result<(), String> {
    // ... 执行逻辑
    for (i, operation) in operations.iter().enumerate() {
        // 执行单个文件操作
        // ...

        // 向前端推送进度
        app.emit("execute-progress", ProgressPayload {
            task_id: task_id.clone(),
            current: i + 1,
            total: operations.len(),
            current_file: operation.source_path.clone(),
            status: "processing".into(),
        }).unwrap();
    }
    Ok(())
}
```

#### B. NAS 安全移动策略

```rust
// engine/executor.rs — 安全移动（复制→校验→删除）
pub async fn safe_move(src: &Path, dest: &Path) -> Result<(), FileOpError> {
    // 1. 复制到目标位置
    tokio::fs::copy(src, dest).await?;

    // 2. 校验：对比源文件和目标文件的哈希
    let src_hash = compute_hash(src).await?;
    let dest_hash = compute_hash(dest).await?;
    if src_hash != dest_hash {
        // 校验失败，删除不完整的目标文件
        tokio::fs::remove_file(dest).await.ok();
        return Err(FileOpError::HashMismatch);
    }

    // 3. 校验通过，删除源文件
    tokio::fs::remove_file(src).await?;
    Ok(())
}
```

#### C. 预览虚拟列表（万级数据不卡顿）

```vue
<!-- components/PreviewTree/PreviewTable.vue -->
<script setup lang="ts">
import { useVirtualList } from "@vueuse/core";

const props = defineProps<{ items: PreviewItem[] }>();

const { list, containerProps, wrapperProps } = useVirtualList(
  computed(() => props.items),
  { itemHeight: 48 }, // 固定行高 48px
);
</script>

<template>
  <div v-bind="containerProps" class="preview-container">
    <div v-bind="wrapperProps">
      <PreviewRow v-for="{ data, index } in list" :key="data.id" :item="data" />
    </div>
  </div>
</template>
```

### 3.6 开发与调试命令

```powershell
# 开发模式（前端热重载 + Rust 自动重编译）
pnpm tauri dev

# 构建生产包（.msi 安装包）
pnpm tauri build

# 仅运行前端（脱离 Tauri 调试 UI）
pnpm dev

# Rust 单元测试
cd src-tauri && cargo test

# 类型检查
pnpm vue-tsc --noEmit
```

---

## 四、 架构全景图

> 见下方 Mermaid 图表渲染。

```
┌────────────────────────────────────────────────────────────┐
│                     SmartSorter 架构                        │
│                                                             │
│  ┌─────────────── 前端 (Vue 3 + Vite) ───────────────┐    │
│  │                                                     │    │
│  │  Views ─── Components ─── Stores (Pinia)            │    │
│  │    │            │              │                     │    │
│  │    └────────────┴──────────────┘                     │    │
│  │                    │                                 │    │
│  │          invoke() / listen()                         │    │
│  └──────────────────┬──────────────────────────────────┘    │
│                     │ Tauri IPC Bridge                       │
│  ┌──────────────────┴──────────────────────────────────┐    │
│  │                                                      │    │
│  │  ┌──────────┐  ┌──────────┐  ┌────────────────────┐ │    │
│  │  │ Commands │──│  Engine  │──│    Storage          │ │    │
│  │  │ (API层)  │  │(核心引擎)│  │ (JSON 文件持久化)   │ │    │
│  │  └──────────┘  └──────────┘  └────────────────────┘ │    │
│  │                     │                                │    │
│  │        ┌────────────┼────────────┐                   │    │
│  │    Scanner    Matcher    Executor                    │    │
│  │   (walkdir)   (条件)    (safe I/O)                   │    │
│  │                                                      │    │
│  └───────────────── 后端 (Rust) ───────────────────────┘    │
│                                                             │
└────────────────────────────────────────────────────────────┘
```
