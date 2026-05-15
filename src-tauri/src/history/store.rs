use std::cell::Cell;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use base64::Engine;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::macos_bridge::{ClipboardItem, ClipboardPayload};
use crate::search::score_clip;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClipKind {
    Text,
    Rtf,
    Html,
    Image,
    FileUrl,
}

impl ClipKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ClipKind::Text => "text",
            ClipKind::Rtf => "rtf",
            ClipKind::Html => "html",
            ClipKind::Image => "image",
            ClipKind::FileUrl => "file_url",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "rtf" => ClipKind::Rtf,
            "html" => ClipKind::Html,
            "image" => ClipKind::Image,
            "file_url" => ClipKind::FileUrl,
            _ => ClipKind::Text,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipSummary {
    pub id: String,
    pub created_at: String,
    pub kind: ClipKind,
    pub text_preview: String,
    pub payload_ref: Option<String>,
    pub is_pinned: bool,
}

#[derive(Debug, Clone)]
pub struct Clip {
    pub id: String,
    pub kind: ClipKind,
    pub text_preview: String,
    pub payloads: Vec<ClipboardPayload>,
}

/// Lightweight file preview info returned to the frontend for file_url clips.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePreview {
    /// "image", "document", "archive", "code", "video", "audio", or "other"
    pub file_type: String,
    /// Lowercase file extension without the dot, e.g. "png", "pdf"
    pub extension: String,
    /// Base64 data-URL thumbnail for image files; null for non-image files
    pub thumbnail: Option<String>,
    /// Number of files in this clip
    pub file_count: usize,
    /// Display name of the first file (filename only)
    pub file_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub max_items: usize,
    pub max_payload_bytes: usize,
    pub trim_whitespace_for_text_dedup: bool,
    pub use_sampling_hash: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            max_items: 1000,
            max_payload_bytes: 25 * 1024 * 1024,
            trim_whitespace_for_text_dedup: true,
            use_sampling_hash: false,
        }
    }
}

pub struct HistoryStore {
    conn: Connection,
    blobs_dir: PathBuf,
    cached_settings: Mutex<Option<AppSettings>>,
    insert_count: Cell<usize>,
}

const PRUNE_INTERVAL: usize = 10;
const SAMPLING_HASH_THRESHOLD: usize = 1024 * 1024;
const SAMPLING_HASH_HEAD_BYTES: usize = 64 * 1024;
const SAMPLING_HASH_TAIL_BYTES: usize = 64 * 1024;

impl HistoryStore {
    pub fn new(app_data_dir: PathBuf) -> rusqlite::Result<Self> {
        fs::create_dir_all(&app_data_dir).map_err(to_sql_error)?;
        let blobs_dir = app_data_dir.join("blobs");
        fs::create_dir_all(&blobs_dir).map_err(to_sql_error)?;

        let conn = Connection::open(app_data_dir.join("history.sqlite3"))?;
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS clips (
                id TEXT PRIMARY KEY,
                created_at TEXT NOT NULL,
                kind TEXT NOT NULL,
                text_preview TEXT NOT NULL,
                payload_ref TEXT,
                pasteboard_hash TEXT NOT NULL UNIQUE,
                source_app TEXT,
                is_pinned INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS payloads (
                clip_id TEXT NOT NULL,
                uti TEXT NOT NULL,
                storage TEXT NOT NULL,
                value TEXT NOT NULL,
                position INTEGER NOT NULL,
                PRIMARY KEY (clip_id, uti),
                FOREIGN KEY (clip_id) REFERENCES clips(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_clips_created_at ON clips(created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_clips_pinned_created ON clips(is_pinned DESC, created_at DESC);
            ",
        )?;

        Ok(Self {
            conn,
            blobs_dir,
            cached_settings: Mutex::new(None),
            insert_count: Cell::new(0),
        })
    }

    pub fn insert_clip(&self, item: ClipboardItem) -> rusqlite::Result<()> {
        if item.payloads.is_empty() {
            return Ok(());
        }

        let settings = self.get_settings()?;
        let total_payload_bytes = item.payloads.iter().map(|payload| payload.data.len()).sum::<usize>();
        if total_payload_bytes > settings.max_payload_bytes {
            return Ok(());
        }

        let hash = self.hash_item(&item, &settings);
        let id = Uuid::new_v4().to_string();
        let created_at = OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| OffsetDateTime::now_utc().unix_timestamp().to_string());

        let payload_ref = item
            .payloads
            .iter()
            .find(|payload| payload.is_blob_candidate())
            .map(|payload| payload.uti.clone());

        // Wrap all DB writes in an explicit transaction.
        // Blob files are written after COMMIT so they use the correct clip_id.
        let tx_result = (|| -> rusqlite::Result<String> {
            self.conn.execute_batch("BEGIN IMMEDIATE")?;

            self.conn.execute(
                "
                INSERT INTO clips (id, created_at, kind, text_preview, payload_ref, pasteboard_hash)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                ON CONFLICT(pasteboard_hash) DO UPDATE SET created_at = excluded.created_at
                ",
                params![
                    id,
                    created_at,
                    item.kind.as_str(),
                    item.text_preview,
                    payload_ref,
                    hash
                ],
            )?;

            let clip_id: String = self
                .conn
                .query_row("SELECT id FROM clips WHERE pasteboard_hash = ?1", params![hash], |row| row.get(0))?;

            self.conn
                .execute("DELETE FROM payloads WHERE clip_id = ?1", params![&clip_id])?;

            for (position, payload) in item.payloads.iter().enumerate() {
                let (storage, value) = self.payload_storage_info(&clip_id, payload);
                self.conn.execute(
                    "
                    INSERT INTO payloads (clip_id, uti, storage, value, position)
                    VALUES (?1, ?2, ?3, ?4, ?5)
                    ",
                    params![&clip_id, payload.uti, storage, value, position as i64],
                )?;
            }

            self.conn.execute_batch("COMMIT")?;
            Ok(clip_id)
        })();

        let clip_id = match tx_result {
            Ok(clip_id) => clip_id,
            Err(e) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                return Err(e);
            }
        };

        // Write blob files after commit — file I/O is best-effort after DB is consistent
        for payload in &item.payloads {
            self.write_blob_file(&clip_id, payload);
        }

        // Prune runs after commit as a separate operation
        let count = self.insert_count.get() + 1;
        self.insert_count.set(count);
        if count >= PRUNE_INTERVAL {
            self.insert_count.set(0);
            self.prune(settings.max_items)?;
        }

        Ok(())
    }

    /// Returns (storage, value) metadata without writing blob files to disk.
    fn payload_storage_info(&self, clip_id: &str, payload: &ClipboardPayload) -> (String, String) {
        if payload.is_blob_candidate() || payload.data.len() > 256 * 1024 {
            let file_name = format!("{}-{}.bin", clip_id, sanitize_uti(&payload.uti));
            ("blob".to_string(), file_name)
        } else {
            (
                "inline".to_string(),
                base64::engine::general_purpose::STANDARD.encode(&payload.data),
            )
        }
    }

    /// Write a single payload's blob file. Errors are logged, not propagated.
    fn write_blob_file(&self, clip_id: &str, payload: &ClipboardPayload) {
        if !payload.is_blob_candidate() && payload.data.len() <= 256 * 1024 {
            return;
        }
        let file_name = format!("{}-{}.bin", clip_id, sanitize_uti(&payload.uti));
        if let Err(e) = fs::write(self.blobs_dir.join(&file_name), &payload.data) {
            tracing::error!("failed to write blob file {file_name}: {e}");
        }
    }

    pub fn search(&self, query: &str, limit: usize, offset: usize) -> rusqlite::Result<Vec<ClipSummary>> {
        let query = query.trim();

        if query.is_empty() {
            let mut stmt = self.conn.prepare(
                "
                SELECT id, created_at, kind, text_preview, payload_ref, is_pinned
                FROM clips
                ORDER BY is_pinned DESC, created_at DESC
                LIMIT ?1 OFFSET ?2
                ",
            )?;

            return stmt
                .query_map(params![limit as i64, offset as i64], |row| {
                    Ok(ClipSummary {
                        id: row.get(0)?,
                        created_at: row.get(1)?,
                        kind: ClipKind::from_str(row.get::<_, String>(2)?.as_str()),
                        text_preview: row.get(3)?,
                        payload_ref: row.get(4)?,
                        is_pinned: row.get::<_, i64>(5)? == 1,
                    })
                })?
                .collect();
        }

        let mut stmt = self.conn.prepare(
            "
            SELECT id, created_at, kind, text_preview, payload_ref, is_pinned
            FROM clips
            ORDER BY is_pinned DESC, created_at DESC
            ",
        )?;

        let mut clips = stmt
            .query_map([], |row| {
                Ok(ClipSummary {
                    id: row.get(0)?,
                    created_at: row.get(1)?,
                    kind: ClipKind::from_str(row.get::<_, String>(2)?.as_str()),
                    text_preview: row.get(3)?,
                    payload_ref: row.get(4)?,
                    is_pinned: row.get::<_, i64>(5)? == 1,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        clips = clips
            .into_iter()
            .filter_map(|clip| score_clip(query, &clip.text_preview).map(|score| (clip, score)))
            .collect::<Vec<_>>()
            .sorted_by_score()
            .into_iter()
            .map(|(clip, _score)| clip)
            .collect();

        // Apply offset + limit in memory for fuzzy search results
        let total = clips.len();
        let start = offset.min(total);
        let end = (start + limit).min(total);
        Ok(clips[start..end].to_vec())
    }

    pub fn get_clip(&self, id: &str) -> rusqlite::Result<Clip> {
        let (kind, text_preview): (ClipKind, String) = self.conn.query_row(
            "SELECT kind, text_preview FROM clips WHERE id = ?1",
            params![id],
            |row| {
                let kind = ClipKind::from_str(row.get::<_, String>(0)?.as_str());
                Ok((kind, row.get(1)?))
            },
        )?;

        let mut stmt = self.conn.prepare(
            "
            SELECT uti, storage, value
            FROM payloads
            WHERE clip_id = ?1
            ORDER BY position ASC
            ",
        )?;

        let payloads = stmt
            .query_map(params![id], |row| {
                let uti: String = row.get(0)?;
                let storage: String = row.get(1)?;
                let value: String = row.get(2)?;

                if storage == "blob" {
                    Ok(ClipboardPayload {
                        uti,
                        data: fs::read(self.blobs_dir.join(value)).map_err(to_sql_error)?,
                    })
                } else {
                    Ok(ClipboardPayload {
                        uti,
                        data: base64::engine::general_purpose::STANDARD
                            .decode(value)
                            .map_err(|error| to_sql_error(std::io::Error::new(std::io::ErrorKind::InvalidData, error)))?,
                    })
                }
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(Clip {
            id: id.to_string(),
            kind,
            text_preview,
            payloads,
        })
    }

    pub fn delete_clip(&self, id: &str) -> rusqlite::Result<()> {
        self.delete_blob_files(id)?;
        self.conn.execute("DELETE FROM clips WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn get_clip_thumbnail(&self, id: &str) -> rusqlite::Result<Option<String>> {
        let row = self
            .conn
            .query_row(
                "SELECT p.storage, p.value, p.uti
                 FROM payloads p
                 WHERE p.clip_id = ?1
                 ORDER BY p.position ASC
                 LIMIT 1",
                params![id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
            )
            .optional()?;

        let Some((storage, value, uti)) = row else {
            return Ok(None);
        };

        let data = if storage == "blob" {
            fs::read(self.blobs_dir.join(&value)).map_err(to_sql_error)?
        } else {
            base64::engine::general_purpose::STANDARD
                .decode(&value)
                .map_err(|error| to_sql_error(std::io::Error::new(std::io::ErrorKind::InvalidData, error)))?
        };

        let mime = uti_to_mime(&uti);
        Ok(Some(format!(
            "data:{};base64,{}",
            mime,
            base64::engine::general_purpose::STANDARD.encode(&data)
        )))
    }

    pub fn get_file_preview(&self, id: &str) -> rusqlite::Result<Option<FilePreview>> {
        let clip = self.get_clip(id)?;
        if clip.kind != ClipKind::FileUrl {
            return Ok(None);
        }

        let file_url_payload = clip.payloads.first();
        let Some(payload) = file_url_payload else {
            return Ok(None);
        };

        let text = String::from_utf8_lossy(&payload.data);
        let paths: Vec<&str> = text.lines().filter(|line| !line.is_empty()).collect();
        let file_count = paths.len();

        let Some(first_path) = paths.first() else {
            return Ok(None);
        };

        let file_name = std::path::Path::new(first_path)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| first_path.to_string());

        let extension = std::path::Path::new(first_path)
            .extension()
            .map(|ext| ext.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        let file_type = classify_file_type(&extension);

        let thumbnail = if file_type == "image" {
            self.read_image_thumbnail(first_path)
        } else {
            None
        };

        Ok(Some(FilePreview {
            file_type,
            extension,
            thumbnail,
            file_count,
            file_name,
        }))
    }

    pub fn pin_clip(&self, id: &str, pinned: bool) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE clips SET is_pinned = ?2 WHERE id = ?1",
            params![id, if pinned { 1 } else { 0 }],
        )?;
        Ok(())
    }

    pub fn get_settings(&self) -> rusqlite::Result<AppSettings> {
        if let Ok(guard) = self.cached_settings.lock() {
            if let Some(ref settings) = *guard {
                return Ok(settings.clone());
            }
        }

        let value = self
            .conn
            .query_row("SELECT value FROM settings WHERE key = 'app'", [], |row| row.get::<_, String>(0))
            .optional()?;

        let settings: AppSettings = value
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();

        if let Ok(mut guard) = self.cached_settings.lock() {
            *guard = Some(settings.clone());
        }

        Ok(settings)
    }

    pub fn save_settings(&self, settings: AppSettings) -> rusqlite::Result<AppSettings> {
        let normalized = AppSettings {
            max_items: settings.max_items.clamp(50, 10_000),
            max_payload_bytes: settings.max_payload_bytes.clamp(1024 * 1024, 500 * 1024 * 1024),
            trim_whitespace_for_text_dedup: settings.trim_whitespace_for_text_dedup,
            use_sampling_hash: settings.use_sampling_hash,
        };
        let value = serde_json::to_string(&normalized)
            .map_err(|error| to_sql_error(std::io::Error::new(std::io::ErrorKind::InvalidData, error)))?;
        self.conn.execute(
            "
            INSERT INTO settings (key, value)
            VALUES ('app', ?1)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value
            ",
            params![value],
        )?;
        self.prune(normalized.max_items)?;
        self.insert_count.set(0);

        if let Ok(mut guard) = self.cached_settings.lock() {
            *guard = Some(normalized.clone());
        }

        // Periodically truncate WAL to bound its size on disk
        if let Err(e) = self.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)") {
            tracing::warn!("WAL checkpoint failed: {e}");
        }

        Ok(normalized)
    }

    fn hash_item(&self, item: &ClipboardItem, settings: &AppSettings) -> String {
        let mut hasher = Sha256::new();
        hasher.update(item.kind.as_str().as_bytes());

        for payload in &item.payloads {
            hasher.update(payload.uti.as_bytes());
            if settings.trim_whitespace_for_text_dedup && item.kind == ClipKind::Text {
                if let Ok(text) = std::str::from_utf8(&payload.data) {
                    hasher.update(text.trim().as_bytes());
                    continue;
                }
            }
            if settings.use_sampling_hash && payload.data.len() > SAMPLING_HASH_THRESHOLD {
                self.sampling_hash_update(&mut hasher, &payload.data);
            } else {
                hasher.update(&payload.data);
            }
        }

        hex::encode(hasher.finalize())
    }

    fn sampling_hash_update(&self, hasher: &mut Sha256, data: &[u8]) {
        let len = data.len();
        hasher.update(&len.to_le_bytes());
        let head = &data[..SAMPLING_HASH_HEAD_BYTES.min(len)];
        hasher.update(head);
        if len > SAMPLING_HASH_HEAD_BYTES + SAMPLING_HASH_TAIL_BYTES {
            let tail = &data[len - SAMPLING_HASH_TAIL_BYTES..];
            hasher.update(tail);
        }
    }

    fn prune(&self, max_items: usize) -> rusqlite::Result<()> {
        let mut stmt = self.conn.prepare(
            "SELECT p.value FROM payloads p
             WHERE p.storage = 'blob'
             AND p.clip_id IN (
                 SELECT id FROM clips WHERE is_pinned = 0
                 ORDER BY created_at DESC, rowid DESC
                 LIMIT -1 OFFSET ?1
             )",
        )?;
        let filenames: Vec<String> = stmt
            .query_map(params![max_items as i64], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        for filename in &filenames {
            if is_safe_filename(filename) {
                let _ = fs::remove_file(self.blobs_dir.join(filename));
            }
        }

        self.conn.execute(
            "DELETE FROM clips WHERE is_pinned = 0 AND id IN (
                SELECT id FROM clips WHERE is_pinned = 0
                ORDER BY created_at DESC, rowid DESC
                LIMIT -1 OFFSET ?1
            )",
            params![max_items as i64],
        )?;

        Ok(())
    }

    fn delete_blob_files(&self, id: &str) -> rusqlite::Result<()> {
        let mut stmt = self.conn.prepare(
            "SELECT value FROM payloads WHERE clip_id = ?1 AND storage = 'blob'",
        )?;
        let filenames: Vec<String> = stmt
            .query_map(params![id], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        for filename in filenames {
            if is_safe_filename(&filename) {
                let _ = fs::remove_file(self.blobs_dir.join(&filename));
            }
        }
        Ok(())
    }
}

trait SortByScore {
    fn sorted_by_score(self) -> Vec<(ClipSummary, i64)>;
}

impl SortByScore for Vec<(ClipSummary, i64)> {
    fn sorted_by_score(mut self) -> Vec<(ClipSummary, i64)> {
        self.sort_by(|(left_clip, left_score), (right_clip, right_score)| {
            right_clip
                .is_pinned
                .cmp(&left_clip.is_pinned)
                .then_with(|| right_score.cmp(left_score))
                .then_with(|| right_clip.created_at.cmp(&left_clip.created_at))
        });
        self
    }
}

fn uti_to_mime(uti: &str) -> &'static str {
    if uti.contains("png") {
        "image/png"
    } else if uti.contains("jpeg") || uti.contains("jpg") {
        "image/jpeg"
    } else if uti.contains("gif") {
        "image/gif"
    } else if uti.contains("tiff") {
        "image/tiff"
    } else if uti.contains("bmp") {
        "image/bmp"
    } else if uti.contains("webp") {
        "image/webp"
    } else if uti.contains("heic") || uti.contains("heif") {
        "image/heic"
    } else {
        "image/png"
    }
}

fn is_safe_filename(name: &str) -> bool {
    !name.contains("..") && !name.contains('/') && !name.contains('\\')
}

fn sanitize_uti(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect()
}

/// Classify a file by its extension into a broad type category for UI display.
fn classify_file_type(ext: &str) -> String {
    match ext {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tiff" | "tif" | "svg"
        | "heic" | "heif" | "ico" | "avif" => "image".to_string(),
        "pdf" => "document".to_string(),
        "doc" | "docx" | "pages" => "document".to_string(),
        "xls" | "xlsx" | "numbers" | "csv" => "document".to_string(),
        "ppt" | "pptx" | "key" => "document".to_string(),
        "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" | "dmg" | "iso" => "archive".to_string(),
        "mp4" | "mov" | "avi" | "mkv" | "webm" | "m4v" => "video".to_string(),
        "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" => "audio".to_string(),
        "rs" | "ts" | "js" | "jsx" | "tsx" | "py" | "rb" | "go" | "java" | "c"
        | "cpp" | "h" | "hpp" | "swift" | "kt" | "sh" | "bash" | "zsh" | "json"
        | "yaml" | "yml" | "toml" | "xml" | "html" | "css" | "scss" | "sql"
        | "md" | "r" | "lua" | "php" | "pl" | "dart" => "code".to_string(),
        _ => "other".to_string(),
    }
}

/// Read the first 256KB of an image file and return a base64 data-URL.
/// Returns None if the file cannot be read or the extension isn't a known image format.
fn read_image_thumbnail(path_str: &str) -> Option<String> {
    let ext = std::path::Path::new(path_str)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "tiff" | "tif" => "image/tiff",
        "svg" => "image/svg+xml",
        "heic" | "heif" => "image/heic",
        "ico" => "image/x-icon",
        "avif" => "image/avif",
        _ => return None,
    };

    // Read up to 512KB to keep memory bounded
    let data = fs::read(path_str).ok()?;
    let data = if data.len() > 512 * 1024 {
        &data[..512 * 1024]
    } else {
        &data
    };

    Some(format!(
        "data:{};base64,{}",
        mime,
        base64::engine::general_purpose::STANDARD.encode(data)
    ))
}

fn to_sql_error(error: std::io::Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macos_bridge::ClipboardPayload;

    fn temp_store() -> (HistoryStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!("paste-test-{}", Uuid::new_v4()));
        let store = HistoryStore::new(dir.clone()).expect("failed to create temp store");
        (store, dir)
    }

    fn cleanup(dir: &PathBuf) {
        let _ = fs::remove_dir_all(dir);
    }

    fn text_item(text: &str) -> ClipboardItem {
        ClipboardItem {
            kind: ClipKind::Text,
            text_preview: text.to_string(),
            payloads: vec![ClipboardPayload {
                uti: "public.utf8-plain-text".to_string(),
                data: text.as_bytes().to_vec(),
            }],
        }
    }

    fn text_item_with_data(text: &str, data: &str) -> ClipboardItem {
        ClipboardItem {
            kind: ClipKind::Text,
            text_preview: text.to_string(),
            payloads: vec![ClipboardPayload {
                uti: "public.utf8-plain-text".to_string(),
                data: data.as_bytes().to_vec(),
            }],
        }
    }

    #[test]
    fn insert_and_search_text() {
        let (store, dir) = temp_store();
        store.insert_clip(text_item("hello world")).unwrap();
        store.insert_clip(text_item("foo bar")).unwrap();

        let results = store.search("", 10, 0).unwrap();
        assert_eq!(results.len(), 2);

        let results = store.search("hello", 10, 0).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].text_preview, "hello world");

        cleanup(&dir);
    }

    #[test]
    fn duplicate_hash_upserts() {
        let (store, dir) = temp_store();
        store.insert_clip(text_item("same content")).unwrap();
        store.insert_clip(text_item("same content")).unwrap();

        let results = store.search("", 10, 0).unwrap();
        assert_eq!(results.len(), 1);

        cleanup(&dir);
    }

    #[test]
    fn delete_clip() {
        let (store, dir) = temp_store();
        store.insert_clip(text_item("to delete")).unwrap();

        let results = store.search("", 10, 0).unwrap();
        assert_eq!(results.len(), 1);
        let id = results[0].id.clone();

        store.delete_clip(&id).unwrap();
        let results = store.search("", 10, 0).unwrap();
        assert_eq!(results.len(), 0);

        cleanup(&dir);
    }

    #[test]
    fn pin_clip() {
        let (store, dir) = temp_store();
        store.insert_clip(text_item("pinned item")).unwrap();

        let results = store.search("", 10, 0).unwrap();
        let id = results[0].id.clone();
        assert!(!results[0].is_pinned);

        store.pin_clip(&id, true).unwrap();
        let results = store.search("", 10, 0).unwrap();
        assert!(results[0].is_pinned);

        cleanup(&dir);
    }

    #[test]
    fn prune_removes_old_items() {
        let (store, dir) = temp_store();

        // max_items minimum is 50 (enforced by save_settings clamp)
        store
            .save_settings(AppSettings {
                max_items: 50,
                ..Default::default()
            })
            .unwrap();

        // Insert 60 items
        for i in 0..60 {
            store
                .insert_clip(text_item_with_data("item", &format!("data-{i}")))
                .unwrap();
        }

        let results = store.search("", 100, 0).unwrap();
        assert_eq!(results.len(), 50, "expected 50 items, got {}", results.len());

        cleanup(&dir);
    }

    #[test]
    fn sampling_hash_produces_different_hash_than_exact() {
        let (store, dir) = temp_store();
        // Create a large payload (>1MB) with unique content in the middle
        let mut data = vec![0u8; 2 * 1024 * 1024];
        data[1024 * 1024] = 42; // unique byte in the middle (sampled-out area)

        let item = ClipboardItem {
            kind: ClipKind::Image,
            text_preview: "[Image]".to_string(),
            payloads: vec![ClipboardPayload {
                uti: "image/png".to_string(),
                data,
            }],
        };

        // Exact hash
        let exact_hash = {
            let mut hasher = Sha256::new();
            hasher.update(item.kind.as_str().as_bytes());
            for p in &item.payloads {
                hasher.update(p.uti.as_bytes());
                hasher.update(&p.data);
            }
            hex::encode(hasher.finalize())
        };

        // Sampling hash via store
        store
            .save_settings(AppSettings {
                use_sampling_hash: true,
                ..Default::default()
            })
            .unwrap();

        let sampling_hash = store.hash_item(&item, &store.get_settings().unwrap());

        // The hashes should differ because middle content is unique
        assert_ne!(exact_hash, sampling_hash);

        cleanup(&dir);
    }

    #[test]
    fn sampling_hash_same_content_same_hash() {
        let (store, dir) = temp_store();
        store
            .save_settings(AppSettings {
                use_sampling_hash: true,
                ..Default::default()
            })
            .unwrap();

        let data = vec![0xABu8; 2 * 1024 * 1024];
        let item1 = ClipboardItem {
            kind: ClipKind::Image,
            text_preview: "[Image]".to_string(),
            payloads: vec![ClipboardPayload {
                uti: "image/png".to_string(),
                data: data.clone(),
            }],
        };
        let item2 = ClipboardItem {
            kind: ClipKind::Image,
            text_preview: "[Image]".to_string(),
            payloads: vec![ClipboardPayload {
                uti: "image/png".to_string(),
                data,
            }],
        };

        let settings = store.get_settings().unwrap();
        assert_eq!(
            store.hash_item(&item1, &settings),
            store.hash_item(&item2, &settings)
        );

        cleanup(&dir);
    }

    #[test]
    fn settings_roundtrip() {
        let (store, dir) = temp_store();
        let settings = AppSettings {
            max_items: 500,
            max_payload_bytes: 10 * 1024 * 1024,
            trim_whitespace_for_text_dedup: false,
            use_sampling_hash: true,
        };
        store.save_settings(settings).unwrap();
        let loaded = store.get_settings().unwrap();
        assert_eq!(loaded.max_items, 500);
        assert_eq!(loaded.max_payload_bytes, 10 * 1024 * 1024);
        assert!(!loaded.trim_whitespace_for_text_dedup);
        assert!(loaded.use_sampling_hash);

        cleanup(&dir);
    }
}
