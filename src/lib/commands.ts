import { invoke } from "@tauri-apps/api/core";

export type ClipKind = "text" | "rtf" | "html" | "image" | "file_url";

export interface ClipSummary {
  id: string;
  createdAt: string;
  kind: ClipKind;
  textPreview: string;
  payloadRef: string | null;
  isPinned: boolean;
}

export interface AppSettings {
  maxItems: number;
  maxPayloadBytes: number;
  trimWhitespaceForTextDedup: boolean;
}

export function searchClips(query: string, limit = 40): Promise<ClipSummary[]> {
  return invoke("search_clips", { query, limit });
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

export function openAccessibilitySettings(): Promise<void> {
  return invoke("open_accessibility_settings");
}
