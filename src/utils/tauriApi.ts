import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  PreviewRequest,
  PreviewResult,
  RuleSet,
  DuplicateResult,
  MediaClassifyResult,
  ClassifyExecuteRequest,
  ClassifyPreviewResult,
  ExecutionLog,
  ProgressEvent,
} from "../types";

// ========== 规则方案 ==========

export async function loadRuleSets(): Promise<RuleSet[]> {
  return invoke<RuleSet[]>("load_rule_sets");
}

export async function saveRuleSet(ruleSet: RuleSet): Promise<void> {
  return invoke("save_rule_set", { ruleSet });
}

export async function deleteRuleSet(id: string): Promise<void> {
  return invoke("delete_rule_set", { id });
}

// ========== 分析预览 ==========

export async function analyzePreview(
  request: PreviewRequest,
): Promise<PreviewResult> {
  return invoke<PreviewResult>("analyze_preview", { request });
}

// ========== 物理执行 ==========

export async function executeTask(
  taskId: string,
  checkedIds: string[],
): Promise<string> {
  return invoke<string>("execute_task", { taskId, checkedIds });
}

// ========== 撤销 ==========

export async function undoTask(logId: string): Promise<void> {
  return invoke("undo_task", { logId });
}

// ========== 重复文件 ==========

export async function scanDuplicates(
  paths: string[],
  recursive: boolean,
): Promise<DuplicateResult> {
  return invoke<DuplicateResult>("scan_duplicates", { paths, recursive });
}

export async function deleteDuplicates(
  pathsToDelete: string[],
): Promise<string> {
  return invoke<string>("delete_duplicates", { pathsToDelete });
}

// ========== 媒体归类 ==========

export async function scanMediaAuthors(
  paths: string[],
  recursive: boolean,
  mediaTypes: string[],
  keywordSources: string[],
): Promise<MediaClassifyResult> {
  return invoke<MediaClassifyResult>("scan_media_authors", {
    paths,
    recursive,
    mediaTypes,
    keywordSources,
  });
}

export async function previewMediaClassify(
  request: ClassifyExecuteRequest,
): Promise<ClassifyPreviewResult> {
  return invoke<ClassifyPreviewResult>("preview_media_classify", { request });
}

export async function executeMediaClassify(taskId: string): Promise<string> {
  return invoke<string>("execute_media_classify", { taskId });
}

// ========== 历史日志 ==========

export async function loadHistory(): Promise<ExecutionLog[]> {
  return invoke<ExecutionLog[]>("load_history");
}

// ========== 选择目录（系统对话框）==========

export async function pickFolder(): Promise<string | null> {
  return invoke<string | null>("pick_folder");
}

// ========== 进度事件监听 ==========

export async function listenProgress(
  callback: (event: ProgressEvent) => void,
): Promise<UnlistenFn> {
  return listen<ProgressEvent>("progress", (e) => callback(e.payload));
}
