import { invoke } from "@tauri-apps/api/core";

export type ClipKind = "text" | "rtf" | "html" | "image" | "file_url";

export interface ClipSummary {
  id: string;
  createdAt: string;
  kind: ClipKind;
  textPreview: string;
  payloadRef: string | null;
  isPinned: boolean;
  tags: string[];
}

export interface AppSettings {
  maxItems: number;
  maxPayloadBytes: number;
  trimWhitespaceForTextDedup: boolean;
  useSamplingHash: boolean;
  retentionDays: number;
}

export function searchClips(query: string, limit = 40, offset = 0): Promise<ClipSummary[]> {
  return invoke("search_clips", { query, limit, offset });
}

export function pasteClip(id: string): Promise<void> {
  return invoke("paste_clip", { id });
}

export function deleteClip(id: string): Promise<void> {
  return invoke("delete_clip", { id });
}

export function pinClip(id: string, pinned: boolean): Promise<void> {
  return invoke("pin_clip", { id, pinned });
}

export function hidePanel(): Promise<void> {
  return invoke("hide_panel");
}

export function getSettings(): Promise<AppSettings> {
  return invoke("get_settings");
}

export function saveSettings(settings: AppSettings): Promise<AppSettings> {
  return invoke("save_settings", { settings });
}

export function hasAccessibilityPermission(): Promise<boolean> {
  return invoke("has_accessibility_permission");
}

export function getClipThumbnail(id: string): Promise<string | null> {
  return invoke("get_clip_thumbnail", { id });
}

export interface FilePreview {
  fileType: string;
  extension: string;
  thumbnail: string | null;
  fileCount: number;
  fileName: string;
}

export function getFilePreview(id: string): Promise<FilePreview | null> {
  return invoke("get_file_preview", { id });
}

export function openAccessibilitySettings(): Promise<void> {
  return invoke("open_accessibility_settings");
}

// ── Export / Import ──────────────────────────────────────────────

export function exportToJson(
  ids?: string[] | null,
  kind?: string | null,
  dateFrom?: string | null,
  dateTo?: string | null
): Promise<string> {
  return invoke("export_to_json", {
    ids: ids ?? null,
    kind: kind ?? null,
    dateFrom: dateFrom ?? null,
    dateTo: dateTo ?? null,
  });
}

export function exportToCsv(
  ids?: string[] | null,
  kind?: string | null,
  dateFrom?: string | null,
  dateTo?: string | null
): Promise<string> {
  return invoke("export_to_csv", {
    ids: ids ?? null,
    kind: kind ?? null,
    dateFrom: dateFrom ?? null,
    dateTo: dateTo ?? null,
  });
}

export interface ImportResult {
  added: number;
  skipped: number;
  failed: number;
  versionWarning?: string;
}

export function importFromJson(json: string, mode: string): Promise<ImportResult> {
  return invoke("import_from_json", { json, mode });
}

export function importFromCsv(csv: string, mode: string): Promise<ImportResult> {
  return invoke("import_from_csv", { csv, mode });
}

// ── Tags ─────────────────────────────────────────────────────────

export interface Tag {
  id: number;
  name: string;
  color: string | null;
  createdAt: string;
}

export function createTag(name: string, color?: string | null): Promise<Tag> {
  return invoke("create_tag", { name, color: color ?? null });
}

export function listTags(): Promise<Tag[]> {
  return invoke("list_tags");
}

export function deleteTag(id: number): Promise<void> {
  return invoke("delete_tag", { id });
}

export function addTagToClip(clipId: string, tagId: number): Promise<void> {
  return invoke("add_tag_to_clip", { clipId, tagId });
}

export function removeTagFromClip(clipId: string, tagId: number): Promise<void> {
  return invoke("remove_tag_from_clip", { clipId, tagId });
}

export function getClipTags(clipId: string): Promise<Tag[]> {
  return invoke("get_clip_tags", { clipId });
}

// ── Rules ────────────────────────────────────────────────────────

export interface Rule {
  id: number;
  name: string;
  pattern: string;
  patternType: string;
  action: string;
  actionValue: string | null;
  enabled: boolean;
  priority: number;
  createdAt: string;
}

export function createRule(
  name: string, pattern: string, patternType: string,
  action: string, actionValue?: string | null
): Promise<Rule> {
  return invoke("create_rule", { name, pattern, patternType, action, actionValue: actionValue ?? null });
}

export function listRules(): Promise<Rule[]> {
  return invoke("list_rules");
}

export function updateRule(
  id: number, name: string, pattern: string, patternType: string,
  action: string, actionValue: string | null, enabled: boolean, priority: number
): Promise<Rule> {
  return invoke("update_rule", { id, name, pattern, patternType, action, actionValue, enabled, priority });
}

export function deleteRule(id: number): Promise<void> {
  return invoke("delete_rule", { id });
}

// ── Data Management ──────────────────────────────────────────────

export function deleteByDateRange(from: string, to: string): Promise<number> {
  return invoke("delete_by_date_range", { from, to });
}

export function deleteSelected(ids: string[]): Promise<number> {
  return invoke("delete_selected", { ids });
}

export function deleteByType(kind: string): Promise<number> {
  return invoke("delete_by_type", { kind });
}

export function autoPrune(retentionDays: number): Promise<number> {
  return invoke("auto_prune", { retentionDays });
}

export function countByType(kind: string): Promise<number> {
  return invoke("count_by_type", { kind });
}

export function countByDateRange(from: string, to: string): Promise<number> {
  return invoke("count_by_date_range", { from, to });
}

export function countPrunable(retentionDays: number): Promise<number> {
  return invoke("count_prunable", { retentionDays });
}

export interface TypeBreakdown {
  kind: string;
  count: number;
  bytes: number;
}

export interface AgeBreakdown {
  range: string;
  count: number;
}

export interface DiskUsage {
  totalItems: number;
  totalBytes: number;
  byType: TypeBreakdown[];
  byAge: AgeBreakdown[];
}

export function getDiskUsage(): Promise<DiskUsage> {
  return invoke("get_disk_usage");
}

export function updateTag(id: number, name: string, color?: string | null): Promise<Tag> {
  return invoke("update_tag", { id, name, color: color ?? null });
}

export function applyRulesToClip(clipId: string): Promise<string[]> {
  return invoke("apply_rules_to_clip", { clipId });
}

export function batchApplyRules(): Promise<number> {
  return invoke("batch_apply_rules");
}
