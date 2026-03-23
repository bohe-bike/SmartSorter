// SmartSorter 核心类型定义 — 与 Rust 后端 Serde 结构对齐

// ========== 规则系统 ==========

export interface RuleSet {
  id: string;
  name: string;
  description?: string;
  created_at: string;
  updated_at: string;
  rules: Rule[];
}

export interface Rule {
  id: string;
  enabled: boolean;
  name: string;
  condition_group: ConditionGroup;
  actions: Action[];
}

export interface ConditionGroup {
  logic: "AND" | "OR";
  conditions: Condition[];
  sub_groups: ConditionGroup[];
}

export interface Condition {
  field: ConditionField;
  operator: Operator;
  value: string | number | string[] | number[];
}

export type ConditionField =
  | "filename"
  | "extension"
  | "full_name"
  | "size_bytes"
  | "created_at"
  | "modified_at"
  | "parent_dir";

export type Operator =
  | "equals"
  | "not_equals"
  | "contains"
  | "not_contains"
  | "starts_with"
  | "ends_with"
  | "regex"
  | "in"
  | "not_in"
  | "gt"
  | "gte"
  | "lt"
  | "lte"
  | "between"
  | "before"
  | "after"
  | "within_days";

export type Action = RenameAction | MoveAction | CopyAction | DeleteAction;

export interface RenameAction {
  type: "rename";
  params: RenameParams;
}

export interface MoveAction {
  type: "move";
  params: RouteParams;
}

export interface CopyAction {
  type: "copy";
  params: RouteParams;
}

export interface DeleteAction {
  type: "delete";
  params: { confirm_required: boolean };
}

export interface RenameParams {
  mode: "replace" | "prefix" | "suffix" | "sequence";
  find?: string;
  replace?: string;
  text?: string;
  template?: string;
  start?: number;
  padding?: number;
  sort_by?: string;
  sort_order?: "asc" | "desc";
}

export interface RouteParams {
  dest_pattern: string;
  conflict_strategy: ConflictStrategy;
}

export type ConflictStrategy = "skip" | "overwrite" | "auto_rename";

// ========== 预览系统 ==========

export interface PreviewRequest {
  source_paths: string[];
  rule_set_id: string;
  recursive: boolean;
  max_depth: number | null;
}

export interface PreviewResult {
  task_id: string;
  generated_at: string;
  summary: PreviewSummary;
  items: PreviewItem[];
  errors: PreviewError[];
}

export interface PreviewSummary {
  total_scanned: number;
  matched: number;
  to_rename: number;
  to_move: number;
  to_copy: number;
  to_delete: number;
  conflicts: number;
  errors: number;
}

export interface PreviewItem {
  id: string;
  checked: boolean;
  source: FileSnapshot;
  target: FileTarget;
  changes: ChangeDetail[];
  conflict: Conflict | null;
}

export interface FileSnapshot {
  path: string;
  name: string;
  size_bytes: number;
  created_at: string;
  modified_at: string;
}

export interface FileTarget {
  path: string;
  name: string;
}

export interface ChangeDetail {
  rule_id: string;
  rule_name: string;
  action_type: string;
  description: string;
}

export interface Conflict {
  conflict_type: string;
  existing_file: FileSnapshot | null;
  resolution: ConflictStrategy;
  resolved_path: string;
}

export interface PreviewError {
  path: string;
  error: string;
  message: string;
}

// ========== 重复文件检测 ==========

export interface DuplicateResult {
  task_id: string;
  scanned_count: number;
  total_groups: number;
  total_wasted_bytes: number;
  groups: DuplicateGroup[];
}

export interface DuplicateGroup {
  group_id: string;
  hash: string;
  file_size: number;
  files: DuplicateFile[];
}

export interface DuplicateFile {
  path: string;
  created_at: string;
  modified_at: string;
  keep: boolean;
}

// ========== 日志与撤销 ==========

export interface ExecutionLog {
  log_id: string;
  task_id: string;
  rule_set_name: string;
  executed_at: string;
  duration_ms: number;
  summary: ExecutionSummary;
  operations: Operation[];
  undo_status: "available" | "partial" | "expired";
}

export interface ExecutionSummary {
  total: number;
  succeeded: number;
  failed: number;
  skipped: number;
}

export interface Operation {
  op_id: string;
  action: string;
  source_path: string;
  target_path: string;
  status: "success" | "failed";
  error_message: string | null;
  reversible: boolean;
}

// ========== 进度事件 ==========

export interface ProgressEvent {
  task_id: string;
  current: number;
  total: number;
  current_file: string;
  phase: string;
}
