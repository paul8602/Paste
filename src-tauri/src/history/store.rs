use std::cell::Cell;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use base64::Engine;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use super::migrations;
use crate::macos_bridge::{ClipboardItem, ClipboardPayload};
use crate::search::{score_clip, parse_search_query, SearchFilters};

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
    #[serde(default)]
    pub tags: Vec<String>,
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

/// JSON export manifest containing version metadata and all exported clips.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportManifest {
    pub version: String,
    pub exported_at: String,
    pub items: Vec<ExportClip>,
}

/// A single clip entry in the export manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportClip {
    pub id: String,
    pub created_at: String,
    pub kind: ClipKind,
    pub text_preview: String,
    pub payloads: Vec<ExportPayload>,
    pub is_pinned: bool,
    pub tags: Vec<String>,
}

/// A single payload entry in the export manifest (always base64-encoded).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportPayload {
    pub uti: String,
    pub data: String,
}

/// Result returned after an import operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub added: usize,
    pub skipped: usize,
    pub failed: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_warning: Option<String>,
}

/// Disk usage breakdown for the clipboard store.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskUsage {
    pub total_items: usize,
    pub total_bytes: u64,
    pub by_type: Vec<TypeBreakdown>,
    pub by_age: Vec<AgeBreakdown>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeBreakdown {
    pub kind: String,
    pub count: usize,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgeBreakdown {
    pub range: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrityReport {
    pub ok: bool,
    pub message: String,
    pub orphaned_blobs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub id: i64,
    pub name: String,
    pub color: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: i64,
    pub name: String,
    pub pattern: String,
    pub pattern_type: String,
    pub action: String,
    pub action_value: Option<String>,
    pub enabled: bool,
    pub priority: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub max_items: usize,
    pub max_payload_bytes: usize,
    pub trim_whitespace_for_text_dedup: bool,
    pub use_sampling_hash: bool,
    #[serde(default = "default_retention_days")]
    pub retention_days: usize,
}

fn default_retention_days() -> usize {
    90
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            max_items: 1000,
            max_payload_bytes: 25 * 1024 * 1024,
            trim_whitespace_for_text_dedup: true,
            use_sampling_hash: true,
            retention_days: 90,
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
const SAMPLING_HASH_THRESHOLD: usize = 256 * 1024;
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

        // Run database migrations
        let db_path = app_data_dir.join("history.sqlite3");
        if let Err(e) = migrations::backup_database(&db_path) {
            tracing::warn!("database backup failed: {e}");
        }
        if let Err(e) = migrations::run_migrations(&conn) {
            tracing::error!("database migration failed: {e}");
        }
        migrations::check_integrity(&conn).ok();

        // WAL checkpoint on startup to bound WAL file growth
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").ok();

        let store = Self {
            conn,
            blobs_dir,
            cached_settings: Mutex::new(None),
            insert_count: Cell::new(0),
        };

        // Cleanup orphaned blob files on startup
        store.cleanup_orphaned_blobs().ok();

        // Auto-prune on startup if retention is configured
        if let Ok(settings) = store.get_settings() {
            if settings.retention_days > 0 {
                match store.auto_prune(settings.retention_days) {
                    Ok(count) if count > 0 => {
                        tracing::info!("startup auto-prune: removed {count} items older than {} days", settings.retention_days);
                    }
                    Err(e) => {
                        tracing::warn!("startup auto-prune failed: {e}");
                    }
                    _ => {}
                }
            }
        }

        Ok(store)
    }

    pub fn insert_clip(&self, item: ClipboardItem) -> rusqlite::Result<String> {
        if item.payloads.is_empty() {
            return Ok(String::new());
        }

        let settings = self.get_settings()?;
        let total_payload_bytes = item.payloads.iter().map(|payload| payload.data.len()).sum::<usize>();
        if total_payload_bytes > settings.max_payload_bytes {
            return Ok(String::new());
        }

        // Retry on SQLITE_BUSY up to 3 times with backoff
        for attempt in 0..3u32 {
            match self.try_insert_clip(&item, &settings) {
                Ok(id) => return Ok(id),
                Err(e) if attempt < 2 && is_sqlite_busy(&e) => {
                    thread::sleep(Duration::from_millis(50 * (attempt + 1) as u64));
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
        unreachable!()
    }

    fn try_insert_clip(&self, item: &ClipboardItem, settings: &AppSettings) -> rusqlite::Result<String> {

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
            // Periodic WAL checkpoint to bound WAL file growth
            self.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").ok();
        }

        Ok(clip_id)
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
        let filters = parse_search_query(query);
        let has_structured = !filters.tags.is_empty()
            || !filters.exclude_tags.is_empty()
            || filters.has_tag.is_some()
            || !filters.types.is_empty()
            || !filters.exclude_types.is_empty()
            || filters.date_from.is_some()
            || filters.date_to.is_some()
            || filters.pinned.is_some()
            || filters.min_size.is_some()
            || filters.max_size.is_some();

        if !has_structured && filters.free_text.is_empty() {
            let mut stmt = self.conn.prepare(
                "
                SELECT id, created_at, kind, text_preview, payload_ref, is_pinned
                FROM clips
                ORDER BY is_pinned DESC, created_at DESC
                LIMIT ?1 OFFSET ?2
                ",
            )?;

            let clips: Vec<ClipSummary> = stmt
                .query_map(params![limit as i64, offset as i64], |row| {
                    Ok(ClipSummary {
                        id: row.get(0)?,
                        created_at: row.get(1)?,
                        kind: ClipKind::from_str(row.get::<_, String>(2)?.as_str()),
                        text_preview: row.get(3)?,
                        payload_ref: row.get(4)?,
                        is_pinned: row.get::<_, i64>(5)? == 1,
                        tags: Vec::new(),
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;

            return self.attach_tags_to_clips(clips);
        }

        // Build SQL with structured filters
        let mut sql = String::from(
            "SELECT c.id, c.created_at, c.kind, c.text_preview, c.payload_ref, c.is_pinned
             FROM clips c WHERE 1=1"
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        // Tag include filters
        for tag_name in &filters.tags {
            let idx = param_values.len() + 1;
            sql.push_str(&format!(
                " AND EXISTS (SELECT 1 FROM clip_tags ct JOIN tags t ON t.id = ct.tag_id WHERE ct.clip_id = c.id AND t.name = ?{idx})"
            ));
            param_values.push(Box::new(tag_name.clone()));
        }

        // Tag exclude filters
        for tag_name in &filters.exclude_tags {
            let idx = param_values.len() + 1;
            sql.push_str(&format!(
                " AND NOT EXISTS (SELECT 1 FROM clip_tags ct JOIN tags t ON t.id = ct.tag_id WHERE ct.clip_id = c.id AND t.name = ?{idx})"
            ));
            param_values.push(Box::new(tag_name.clone()));
        }

        // Has any tag
        if filters.has_tag == Some(true) {
            sql.push_str(" AND EXISTS (SELECT 1 FROM clip_tags ct WHERE ct.clip_id = c.id)");
        }

        // Type filters
        if !filters.types.is_empty() {
            let placeholders: Vec<String> = filters.types.iter().enumerate()
                .map(|(i, _)| format!("?{}", param_values.len() + i + 1))
                .collect();
            sql.push_str(&format!(" AND c.kind IN ({})", placeholders.join(",")));
            for t in &filters.types {
                param_values.push(Box::new(t.clone()));
            }
        }
        for t in &filters.exclude_types {
            let idx = param_values.len() + 1;
            sql.push_str(&format!(" AND c.kind != ?{idx}"));
            param_values.push(Box::new(t.clone()));
        }

        // Date filters
        if let Some(ref from) = filters.date_from {
            let idx = param_values.len() + 1;
            sql.push_str(&format!(" AND c.created_at >= ?{idx}"));
            param_values.push(Box::new(from.clone()));
        }
        if let Some(ref to) = filters.date_to {
            let idx = param_values.len() + 1;
            sql.push_str(&format!(" AND c.created_at <= ?{idx}"));
            param_values.push(Box::new(to.clone()));
        }

        // Pinned filter
        if let Some(pinned) = filters.pinned {
            let idx = param_values.len() + 1;
            sql.push_str(&format!(" AND c.is_pinned = ?{idx}"));
            param_values.push(Box::new(if pinned { 1i64 } else { 0i64 }));
        }

        // Size filters (based on payload sizes)
        if filters.min_size.is_some() || filters.max_size.is_some() {
            sql.push_str(" AND (SELECT COALESCE(SUM(LENGTH(p.value)), 0) FROM payloads p WHERE p.clip_id = c.id AND p.storage = 'inline')");
            if let Some(min) = filters.min_size {
                let idx = param_values.len() + 1;
                sql.push_str(&format!(" >= ?{idx}"));
                param_values.push(Box::new(min as i64));
            }
            if let Some(max) = filters.max_size {
                let idx = param_values.len() + 1;
                sql.push_str(&format!(" <= ?{idx}"));
                param_values.push(Box::new(max as i64));
            }
        }

        sql.push_str(" ORDER BY c.is_pinned DESC, c.created_at DESC");

        // If we have free-text, fetch more rows than needed for fuzzy scoring
        // but still bound the SQL result set
        if !filters.free_text.is_empty() {
            let fetch_limit = ((offset + limit) * 5).max(500);
            sql.push_str(&format!(" LIMIT {fetch_limit}"));
        }

        let params_ref: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
        let mut stmt = self.conn.prepare(&sql)?;

        let mut clips: Vec<ClipSummary> = stmt
            .query_map(params_ref.as_slice(), |row| {
                Ok(ClipSummary {
                    id: row.get(0)?,
                    created_at: row.get(1)?,
                    kind: ClipKind::from_str(row.get::<_, String>(2)?.as_str()),
                    text_preview: row.get(3)?,
                    payload_ref: row.get(4)?,
                    is_pinned: row.get::<_, i64>(5)? == 1,
                    tags: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        // Apply fuzzy text matching on the free-text portion
        if !filters.free_text.is_empty() {
            clips = clips
                .into_iter()
                .filter_map(|clip| {
                    score_clip(&filters.free_text, &clip.text_preview).map(|score| (clip, score))
                })
                .collect::<Vec<_>>()
                .sorted_by_score()
                .into_iter()
                .map(|(clip, _score)| clip)
                .collect();
        }

        // Apply offset + limit
        let total = clips.len();
        let start = offset.min(total);
        let end = (start + limit).min(total);
        let clips = clips[start..end].to_vec();

        self.attach_tags_to_clips(clips)
    }

    fn attach_tags_to_clips(&self, mut clips: Vec<ClipSummary>) -> rusqlite::Result<Vec<ClipSummary>> {
        if clips.is_empty() {
            return Ok(clips);
        }
        let ids: Vec<String> = clips.iter().map(|c| c.id.clone()).collect();
        let placeholders: Vec<String> = ids.iter().enumerate().map(|(i, _)| format!("?{}", i + 1)).collect();
        let sql = format!(
            "SELECT ct.clip_id, t.name FROM tags t
             INNER JOIN clip_tags ct ON ct.tag_id = t.id
             WHERE ct.clip_id IN ({})
             ORDER BY t.name",
            placeholders.join(",")
        );
        let params_ref: Vec<&dyn rusqlite::types::ToSql> = ids.iter().map(|id| id as &dyn rusqlite::types::ToSql).collect();
        let mut stmt = self.conn.prepare(&sql)?;
        let mut tag_map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
        let rows = stmt.query_map(params_ref.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows.flatten() {
            tag_map.entry(row.0).or_default().push(row.1);
        }
        for clip in &mut clips {
            clip.tags = tag_map.remove(&clip.id).unwrap_or_default();
        }
        Ok(clips)
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
            read_image_thumbnail(first_path)
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

    /// Update the text content of a text-type clip.
    pub fn update_clip_text(&self, id: &str, new_text: &str) -> rusqlite::Result<()> {
        let kind: String = self.conn.query_row(
            "SELECT kind FROM clips WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        if kind != "text" {
            return Err(rusqlite::Error::InvalidParameterName(
                "only text clips can be edited".to_string(),
            ));
        }

        let settings = self.get_settings()?;

        let mut hasher = Sha256::new();
        hasher.update(b"text");
        if settings.trim_whitespace_for_text_dedup {
            hasher.update(new_text.trim().as_bytes());
        } else {
            hasher.update(new_text.as_bytes());
        }
        let new_hash = hex::encode(hasher.finalize());

        let collision: bool = self.conn.query_row(
            "SELECT COUNT(*) > 0 FROM clips WHERE pasteboard_hash = ?1 AND id != ?2",
            params![new_hash, id],
            |row| row.get(0),
        )?;
        if collision {
            return Err(rusqlite::Error::InvalidParameterName(
                "another clip with the same content already exists".to_string(),
            ));
        }

        let encoded = base64::engine::general_purpose::STANDARD.encode(new_text.as_bytes());

        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| -> rusqlite::Result<()> {
            self.conn.execute(
                "UPDATE clips SET text_preview = ?2, pasteboard_hash = ?3 WHERE id = ?1",
                params![id, new_text, new_hash],
            )?;
            self.conn.execute(
                "UPDATE payloads SET value = ?2, storage = 'inline'
                 WHERE clip_id = ?1 AND uti = 'public.utf8-plain-text'",
                params![id, encoded],
            )?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.conn.execute_batch("COMMIT")?;
                Ok(())
            }
            Err(e) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
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
            retention_days: settings.retention_days,
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

    // ── Export / Import ──────────────────────────────────────────────

    /// Export all clips (or a filtered subset) as a JSON manifest string.
    pub fn export_to_json(
        &self,
        ids: Option<Vec<String>>,
        kind: Option<ClipKind>,
        date_from: Option<String>,
        date_to: Option<String>,
    ) -> rusqlite::Result<String> {
        let items = self.collect_export_clips(ids, kind, date_from, date_to)?;
        let manifest = ExportManifest {
            version: "1.0.7".to_string(),
            exported_at: OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default(),
            items,
        };
        serde_json::to_string_pretty(&manifest)
            .map_err(|e| to_sql_error(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))
    }

    /// Export clips as CSV.
    /// Columns: id, text_preview, kind, created_at, is_pinned
    pub fn export_to_csv(
        &self,
        ids: Option<Vec<String>>,
        kind: Option<ClipKind>,
        date_from: Option<String>,
        date_to: Option<String>,
    ) -> rusqlite::Result<String> {
        let items = self.collect_export_clips(ids, kind, date_from, date_to)?;
        let mut csv = String::from("\u{FEFF}"); // UTF-8 BOM for Excel
        csv.push_str("id,text_preview,kind,created_at,is_pinned\n");
        for item in &items {
            let escaped = escape_csv(&item.text_preview);
            csv.push_str(&format!(
                "{},{},{},{},{}\n",
                item.id,
                escaped,
                item.kind.as_str(),
                item.created_at,
                if item.is_pinned { "true" } else { "false" }
            ));
        }
        Ok(csv)
    }

    /// Import clips from a JSON manifest string.
    /// `mode`: "merge" (skip duplicates), "replace" (clear first), or "append" (no dedup).
    /// Returns `ImportResult` with an optional `version_warning` if the manifest is from a newer version.
    pub fn import_from_json(&self, json: &str, mode: &str) -> rusqlite::Result<ImportResult> {
        let manifest: ExportManifest = serde_json::from_str(json)
            .map_err(|e| to_sql_error(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;

        let version_warning = Self::check_import_version(&manifest.version);
        if let Some(ref warning) = version_warning {
            tracing::warn!("{warning}");
        }

        if mode == "replace" {
            // Delete all existing clips
            let ids: Vec<String> = self
                .conn
                .prepare("SELECT id FROM clips")?
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for id in &ids {
                let _ = self.delete_clip(id);
            }
        }

        let mut result = ImportResult {
            added: 0,
            skipped: 0,
            failed: 0,
            version_warning,
        };

        for item in &manifest.items {
            // For merge mode, check if a clip with the same text_preview+kind exists
            if mode == "merge" {
                let exists: bool = self
                    .conn
                    .query_row(
                        "SELECT COUNT(*) > 0 FROM clips WHERE kind = ?1 AND text_preview = ?2",
                        params![item.kind.as_str(), item.text_preview],
                        |row| row.get(0),
                    )
                    .unwrap_or(false);
                if exists {
                    result.skipped += 1;
                    continue;
                }
            }

            // Build ClipboardItem and insert
            let mut payloads: Vec<crate::macos_bridge::ClipboardPayload> = Vec::new();
            for ep in &item.payloads {
                let data = base64::engine::general_purpose::STANDARD
                    .decode(&ep.data)
                    .unwrap_or_default();
                payloads.push(crate::macos_bridge::ClipboardPayload {
                    uti: ep.uti.clone(),
                    data,
                });
            }

            if mode == "append" {
                // Append mode: insert directly with a unique hash to bypass dedup
                let id = Uuid::new_v4().to_string();
                let created_at = OffsetDateTime::now_utc()
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default();
                let payload_ref = item.payloads.first().map(|p| p.uti.clone());

                if let Err(e) = self.conn.execute(
                    "INSERT INTO clips (id, created_at, kind, text_preview, payload_ref, pasteboard_hash, is_pinned)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        id, created_at, item.kind.as_str(), item.text_preview,
                        payload_ref, Uuid::new_v4().to_string(),
                        if item.is_pinned { 1 } else { 0 },
                    ],
                ) {
                    result.failed += 1;
                    continue;
                }
                for (pos, p) in item.payloads.iter().enumerate() {
                    let raw = base64::engine::general_purpose::STANDARD
                        .decode(&p.data)
                        .unwrap_or_default();
                    let (storage, value) = if raw.len() > 256 * 1024 {
                        let file_name = format!("{}-{}.bin", id, sanitize_uti(&p.uti));
                        let _ = std::fs::write(self.blobs_dir.join(&file_name), &raw);
                        ("blob".to_string(), file_name)
                    } else {
                        ("inline".to_string(), base64::engine::general_purpose::STANDARD.encode(&raw))
                    };
                    let _ = self.conn.execute(
                        "INSERT INTO payloads (clip_id, uti, storage, value, position) VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![id, p.uti, storage, value, pos as i64],
                    );
                }
                result.added += 1;
            } else {
                let clipboard_item = crate::macos_bridge::ClipboardItem {
                    kind: item.kind,
                    text_preview: item.text_preview.clone(),
                    payloads,
                };
                match self.insert_clip(clipboard_item) {
                    Ok(_) => {
                        result.added += 1;
                        if item.is_pinned {
                            let new_id: Option<String> = self
                                .conn
                                .query_row(
                                    "SELECT id FROM clips WHERE kind = ?1 AND text_preview = ?2 ORDER BY created_at DESC LIMIT 1",
                                    params![item.kind.as_str(), item.text_preview],
                                    |row| row.get(0),
                                )
                                .optional()?;
                            if let Some(new_id) = new_id {
                                let _ = self.pin_clip(&new_id, true);
                            }
                        }
                    }
                    Err(_) => {
                        result.failed += 1;
                    }
                }
            }
        }

        Ok(result)
    }

    /// Import clips from a CSV string.
    /// Expected columns: id, text_preview, kind, created_at, is_pinned
    pub fn import_from_csv(&self, csv_data: &str, mode: &str) -> rusqlite::Result<ImportResult> {
        let csv_data = csv_data.strip_prefix('\u{FEFF}').unwrap_or(csv_data);

        let mut reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .from_reader(csv_data.as_bytes());

        if mode == "replace" {
            let ids: Vec<String> = self
                .conn
                .prepare("SELECT id FROM clips")?
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for id in &ids {
                let _ = self.delete_clip(id);
            }
        }

        let mut result = ImportResult {
            added: 0,
            skipped: 0,
            failed: 0,
            version_warning: None,
        };

        for record in reader.records() {
            let record = match record {
                Ok(r) => r,
                Err(_) => {
                    result.failed += 1;
                    continue;
                }
            };

            let text_preview = record.get(1).unwrap_or("").to_string();
            let kind_str = record.get(2).unwrap_or("text");
            let kind = ClipKind::from_str(kind_str);
            let is_pinned = record.get(4).unwrap_or("false") == "true";

            if mode == "merge" {
                let exists: bool = self
                    .conn
                    .query_row(
                        "SELECT COUNT(*) > 0 FROM clips WHERE kind = ?1 AND text_preview = ?2",
                        params![kind.as_str(), text_preview],
                        |row| row.get(0),
                    )
                    .unwrap_or(false);
                if exists {
                    result.skipped += 1;
                    continue;
                }
            }

            let id = Uuid::new_v4().to_string();
            let default_ts = OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default();
            let created_at = record.get(3).unwrap_or(&default_ts);

            if self
                .conn
                .execute(
                    "INSERT INTO clips (id, created_at, kind, text_preview, payload_ref, pasteboard_hash, is_pinned)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        id,
                        created_at,
                        kind.as_str(),
                        text_preview,
                        Option::<String>::None,
                        Uuid::new_v4().to_string(),
                        if is_pinned { 1 } else { 0 },
                    ],
                )
                .is_err()
            {
                result.failed += 1;
                continue;
            }
            result.added += 1;
        }

        Ok(result)
    }

    /// Collect clips for export based on optional filters.
    fn collect_export_clips(
        &self,
        ids: Option<Vec<String>>,
        kind: Option<ClipKind>,
        date_from: Option<String>,
        date_to: Option<String>,
    ) -> rusqlite::Result<Vec<ExportClip>> {
        let mut sql = String::from(
            "SELECT id, created_at, kind, text_preview, payload_ref, is_pinned FROM clips WHERE 1=1"
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ref id_list) = ids {
            let placeholders: Vec<String> = id_list.iter().enumerate()
                .map(|(i, _)| format!("?{}", i + 1))
                .collect();
            sql.push_str(&format!(" AND id IN ({})", placeholders.join(",")));
            for id in id_list {
                param_values.push(Box::new(id.clone()));
            }
        }
        if let Some(ref k) = kind {
            sql.push_str(&format!(" AND kind = ?{}", param_values.len() + 1));
            param_values.push(Box::new(k.as_str().to_string()));
        }
        if let Some(ref from) = date_from {
            sql.push_str(&format!(" AND created_at >= ?{}", param_values.len() + 1));
            param_values.push(Box::new(from.clone()));
        }
        if let Some(ref to) = date_to {
            sql.push_str(&format!(" AND created_at <= ?{}", param_values.len() + 1));
            param_values.push(Box::new(to.clone()));
        }
        sql.push_str(" ORDER BY created_at DESC");

        let params_ref: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
        let mut stmt = self.conn.prepare(&sql)?;
        let summaries = stmt
            .query_map(params_ref.as_slice(), |row| {
                Ok(ClipSummary {
                    id: row.get(0)?,
                    created_at: row.get(1)?,
                    kind: ClipKind::from_str(row.get::<_, String>(2)?.as_str()),
                    text_preview: row.get(3)?,
                    payload_ref: row.get(4)?,
                    is_pinned: row.get::<_, i64>(5)? == 1,
                    tags: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut items = Vec::with_capacity(summaries.len());
        for s in &summaries {
            let clip = self.get_clip(&s.id)?;
            let payloads: Vec<ExportPayload> = clip
                .payloads
                .iter()
                .map(|p| ExportPayload {
                    uti: p.uti.clone(),
                    data: base64::engine::general_purpose::STANDARD.encode(&p.data),
                })
                .collect();
            items.push(ExportClip {
                id: s.id.clone(),
                created_at: s.created_at.clone(),
                kind: s.kind,
                text_preview: s.text_preview.clone(),
                payloads,
                is_pinned: s.is_pinned,
                tags: Vec::new(),
            });
        }
        Ok(items)
    }

    // ── Data Management ───────────────────────────────────────────────

    /// Delete clips within a date range. Returns number of items deleted.
    pub fn delete_by_date_range(&self, from: &str, to: &str) -> rusqlite::Result<usize> {
        let ids: Vec<String> = self
            .conn
            .prepare(
                "SELECT id FROM clips WHERE created_at >= ?1 AND created_at <= ?2 AND is_pinned = 0"
            )?
            .query_map(params![from, to], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let count = ids.len();
        for id in &ids {
            let _ = self.delete_clip(id);
        }
        Ok(count)
    }

    /// Count clips within a date range (for preview before deletion).
    pub fn count_by_date_range(&self, from: &str, to: &str) -> rusqlite::Result<usize> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM clips WHERE created_at >= ?1 AND created_at <= ?2 AND is_pinned = 0",
            params![from, to],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Delete clips by a list of ids. Returns number of items deleted.
    pub fn delete_selected(&self, ids: &[String]) -> rusqlite::Result<usize> {
        let mut count = 0;
        for id in ids {
            if self.delete_clip(id).is_ok() {
                count += 1;
            }
        }
        Ok(count)
    }

    /// Delete all clips of a given kind. Returns number of items deleted.
    pub fn delete_by_type(&self, kind: ClipKind) -> rusqlite::Result<usize> {
        let ids: Vec<String> = self
            .conn
            .prepare("SELECT id FROM clips WHERE kind = ?1 AND is_pinned = 0")?
            .query_map(params![kind.as_str()], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let count = ids.len();
        for id in &ids {
            let _ = self.delete_clip(id);
        }
        Ok(count)
    }

    /// Count clips of a given kind (for preview before deletion).
    pub fn count_by_type(&self, kind: ClipKind) -> rusqlite::Result<usize> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM clips WHERE kind = ?1 AND is_pinned = 0",
            params![kind.as_str()],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Auto-prune clips older than `retention_days`, skipping pinned items.
    /// Returns number of items removed. If `retention_days` is 0, does nothing.
    pub fn auto_prune(&self, retention_days: usize) -> rusqlite::Result<usize> {
        if retention_days == 0 {
            return Ok(0);
        }
        let cutoff = OffsetDateTime::now_utc()
            .saturating_sub(time::Duration::days(retention_days as i64))
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        let ids: Vec<String> = self
            .conn
            .prepare(
                "SELECT id FROM clips WHERE created_at < ?1 AND is_pinned = 0"
            )?
            .query_map(params![cutoff], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let count = ids.len();
        for id in &ids {
            let _ = self.delete_clip(id);
        }
        Ok(count)
    }

    /// Count clips that would be pruned (for preview before auto-prune).
    pub fn count_prunable(&self, retention_days: usize) -> rusqlite::Result<usize> {
        if retention_days == 0 {
            return Ok(0);
        }
        let cutoff = OffsetDateTime::now_utc()
            .saturating_sub(time::Duration::days(retention_days as i64))
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM clips WHERE created_at < ?1 AND is_pinned = 0",
            params![cutoff],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    const CURRENT_VERSION: &'static str = "1.0.7";

    fn check_import_version(manifest_version: &str) -> Option<String> {
        let current: Vec<u32> = Self::CURRENT_VERSION
            .split('.')
            .filter_map(|s| s.parse().ok())
            .collect();
        let imported: Vec<u32> = manifest_version
            .split('.')
            .filter_map(|s| s.parse().ok())
            .collect();
        if imported > current {
            Some(format!(
                "Importing from version {manifest_version} (newer than {0}). Some data may not be fully compatible.",
                Self::CURRENT_VERSION
            ))
        } else {
            None
        }
    }

    /// Return disk usage statistics: total count, total bytes, by type, by age.
    pub fn get_disk_usage(&self) -> rusqlite::Result<DiskUsage> {
        let total_items: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM clips", [], |row| row.get(0))?;
        let total_items_usize: usize = total_items as usize;

        // Total bytes: inline payloads + blob files
        let inline_bytes: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(SUM(LENGTH(value)), 0) FROM payloads WHERE storage = 'inline'",
                [],
                |row| row.get(0),
            )?;
        let blob_bytes = self
            .compute_blob_bytes();
        let total_bytes = inline_bytes as u64 + blob_bytes;

        // By type
        let mut by_type_stmt = self.conn.prepare(
            "SELECT kind, COUNT(*), COALESCE(SUM(LENGTH(p.value)), 0)
             FROM clips c
             LEFT JOIN payloads p ON p.clip_id = c.id AND p.storage = 'inline'
             GROUP BY c.kind
             ORDER BY COUNT(*) DESC"
        )?;
        let mut by_type: Vec<TypeBreakdown> = by_type_stmt
            .query_map([], |row| {
                let kind: String = row.get(0)?;
                let count_i64: i64 = row.get(1)?;
                let bytes: u64 = row.get::<_, i64>(2)? as u64;
                Ok(TypeBreakdown { kind, count: count_i64 as usize, bytes })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        // Add blob bytes proportionally to each type
        let type_count = by_type.len();
        for bt in &mut by_type {
            if total_items > 0 && type_count > 0 {
                bt.bytes += (blob_bytes * bt.count as u64) / total_items as u64;
            }
        }

        // By age
        let now = OffsetDateTime::now_utc();
        let threshold_30 = now.saturating_sub(time::Duration::days(30))
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        let threshold_90 = now.saturating_sub(time::Duration::days(90))
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();

        let count_lt_30: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM clips WHERE created_at >= ?1",
            params![threshold_30],
            |row| row.get(0),
        )?;
        let count_30_90: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM clips WHERE created_at >= ?1 AND created_at < ?2",
            params![threshold_90, threshold_30],
            |row| row.get(0),
        )?;
        let count_gt_90: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM clips WHERE created_at < ?1",
            params![threshold_90],
            |row| row.get(0),
        )?;

        let by_age = vec![
            AgeBreakdown { range: "<30 days".to_string(), count: count_lt_30 as usize },
            AgeBreakdown { range: "30-90 days".to_string(), count: count_30_90 as usize },
            AgeBreakdown { range: ">90 days".to_string(), count: count_gt_90 as usize },
        ];

        Ok(DiskUsage { total_items: total_items_usize, total_bytes, by_type, by_age })
    }

    // ── Tags CRUD ────────────────────────────────────────────────────

    pub fn create_tag(&self, name: &str, color: Option<&str>) -> rusqlite::Result<Tag> {
        self.conn.execute(
            "INSERT INTO tags (name, color) VALUES (?1, ?2)",
            params![name, color],
        )?;
        let id = self.conn.last_insert_rowid();
        self.conn.query_row(
            "SELECT id, name, color, created_at FROM tags WHERE id = ?1",
            params![id],
            |row| {
                Ok(Tag {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: row.get(2)?,
                    created_at: row.get(3)?,
                })
            },
        )
    }

    pub fn list_tags(&self) -> rusqlite::Result<Vec<Tag>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, color, created_at FROM tags ORDER BY name")?;
        let tags = stmt
            .query_map([], |row| {
                Ok(Tag {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(tags)
    }

    pub fn delete_tag(&self, id: i64) -> rusqlite::Result<()> {
        self.conn
            .execute("DELETE FROM tags WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn update_tag(&self, id: i64, name: &str, color: Option<&str>) -> rusqlite::Result<Tag> {
        self.conn.execute(
            "UPDATE tags SET name = ?1, color = ?2 WHERE id = ?3",
            params![name, color, id],
        )?;
        self.conn.query_row(
            "SELECT id, name, color, created_at FROM tags WHERE id = ?1",
            params![id],
            |row| {
                Ok(Tag {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: row.get(2)?,
                    created_at: row.get(3)?,
                })
            },
        )
    }

    pub fn add_tag_to_clip(&self, clip_id: &str, tag_id: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO clip_tags (clip_id, tag_id) VALUES (?1, ?2)",
            params![clip_id, tag_id],
        )?;
        Ok(())
    }

    pub fn remove_tag_from_clip(&self, clip_id: &str, tag_id: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "DELETE FROM clip_tags WHERE clip_id = ?1 AND tag_id = ?2",
            params![clip_id, tag_id],
        )?;
        Ok(())
    }

    pub fn get_clip_tags(&self, clip_id: &str) -> rusqlite::Result<Vec<Tag>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.name, t.color, t.created_at FROM tags t
             INNER JOIN clip_tags ct ON ct.tag_id = t.id
             WHERE ct.clip_id = ?1 ORDER BY t.name",
        )?;
        let tags = stmt
            .query_map(params![clip_id], |row| {
                Ok(Tag {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(tags)
    }

    // ── Rules CRUD ───────────────────────────────────────────────────

    pub fn create_rule(
        &self,
        name: &str,
        pattern: &str,
        pattern_type: &str,
        action: &str,
        action_value: Option<&str>,
    ) -> rusqlite::Result<Rule> {
        self.conn.execute(
            "INSERT INTO rules (name, pattern, pattern_type, action, action_value)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![name, pattern, pattern_type, action, action_value],
        )?;
        let id = self.conn.last_insert_rowid();
        self.get_rule(id)
    }

    pub fn get_rule(&self, id: i64) -> rusqlite::Result<Rule> {
        self.conn.query_row(
            "SELECT id, name, pattern, pattern_type, action, action_value, enabled, priority, created_at
             FROM rules WHERE id = ?1",
            params![id],
            |row| {
                Ok(Rule {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    pattern: row.get(2)?,
                    pattern_type: row.get(3)?,
                    action: row.get(4)?,
                    action_value: row.get(5)?,
                    enabled: row.get::<_, i64>(6)? == 1,
                    priority: row.get(7)?,
                    created_at: row.get(8)?,
                })
            },
        )
    }

    pub fn list_rules(&self) -> rusqlite::Result<Vec<Rule>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, pattern, pattern_type, action, action_value, enabled, priority, created_at
             FROM rules ORDER BY priority DESC, name",
        )?;
        let rules = stmt
            .query_map([], |row| {
                Ok(Rule {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    pattern: row.get(2)?,
                    pattern_type: row.get(3)?,
                    action: row.get(4)?,
                    action_value: row.get(5)?,
                    enabled: row.get::<_, i64>(6)? == 1,
                    priority: row.get(7)?,
                    created_at: row.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rules)
    }

    pub fn get_enabled_rules(&self) -> rusqlite::Result<Vec<Rule>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, pattern, pattern_type, action, action_value, enabled, priority, created_at
             FROM rules WHERE enabled = 1 ORDER BY priority DESC",
        )?;
        let rules = stmt
            .query_map([], |row| {
                Ok(Rule {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    pattern: row.get(2)?,
                    pattern_type: row.get(3)?,
                    action: row.get(4)?,
                    action_value: row.get(5)?,
                    enabled: row.get::<_, i64>(6)? == 1,
                    priority: row.get(7)?,
                    created_at: row.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rules)
    }

    pub fn update_rule(
        &self,
        id: i64,
        name: &str,
        pattern: &str,
        pattern_type: &str,
        action: &str,
        action_value: Option<&str>,
        enabled: bool,
        priority: i64,
    ) -> rusqlite::Result<Rule> {
        self.conn.execute(
            "UPDATE rules SET name = ?1, pattern = ?2, pattern_type = ?3, action = ?4,
             action_value = ?5, enabled = ?6, priority = ?7 WHERE id = ?8",
            params![
                name,
                pattern,
                pattern_type,
                action,
                action_value,
                enabled as i64,
                priority,
                id
            ],
        )?;
        self.get_rule(id)
    }

    pub fn delete_rule(&self, id: i64) -> rusqlite::Result<()> {
        self.conn
            .execute("DELETE FROM rules WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Execute all enabled rules against a clip. Returns the actions taken.
    pub fn apply_rules(&self, clip_id: &str, text_preview: &str, kind: &str) -> rusqlite::Result<Vec<String>> {
        let rules = self.get_enabled_rules()?;
        let mut actions_taken = Vec::new();

        for rule in &rules {
            if !self.matches_rule(text_preview, kind, &rule.pattern, &rule.pattern_type) {
                continue;
            }

            match rule.action.as_str() {
                "tag" => {
                    if let Some(ref tag_name) = rule.action_value {
                        // Find or create the tag
                        let tag_id = match self.conn.query_row(
                            "SELECT id FROM tags WHERE name = ?1",
                            params![tag_name],
                            |row| row.get::<_, i64>(0),
                        ) {
                            Ok(id) => id,
                            Err(_) => {
                                // Auto-create the tag
                                self.conn.execute(
                                    "INSERT INTO tags (name) VALUES (?1)",
                                    params![tag_name],
                                )?;
                                self.conn.last_insert_rowid()
                            }
                        };
                        let _ = self.add_tag_to_clip(clip_id, tag_id);
                        actions_taken.push(format!("tagged:{}", tag_name));
                    }
                }
                "delete" => {
                    let _ = self.delete_clip(clip_id);
                    actions_taken.push("deleted".to_string());
                    return Ok(actions_taken); // Stop processing further rules
                }
                "notify" => {
                    let msg = rule.action_value.as_deref().unwrap_or("Rule matched");
                    actions_taken.push(format!("notify:{}", msg));
                }
                _ => {}
            }
        }

        Ok(actions_taken)
    }

    fn matches_rule(&self, text: &str, kind: &str, pattern: &str, pattern_type: &str) -> bool {
        match pattern_type {
            "regex" => {
                if let Ok(re) = regex::Regex::new(pattern) {
                    re.is_match(text)
                } else {
                    false
                }
            }
            "literal" => text.contains(pattern),
            "url" => {
                if kind == "file_url" || text.starts_with("http") {
                    text.contains(pattern)
                } else {
                    false
                }
            }
            "email" => {
                if let Ok(re) = regex::Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}") {
                    if pattern.starts_with('@') {
                        // Match domain part
                        re.find(text).map_or(false, |m| m.as_str().ends_with(pattern))
                    } else {
                        re.is_match(text) && text.contains(pattern)
                    }
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Compute total size of blob files on disk.
    fn compute_blob_bytes(&self) -> u64 {
        let mut total: u64 = 0;
        if let Ok(entries) = std::fs::read_dir(&self.blobs_dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    total += meta.len();
                }
            }
        }
        total
    }

    /// Run integrity check and cleanup, returning a report.
    pub fn verify_integrity(&self) -> rusqlite::Result<IntegrityReport> {
        let result: String = self
            .conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        let orphaned = self.cleanup_orphaned_blobs().unwrap_or(0);
        Ok(IntegrityReport {
            ok: result == "ok",
            message: result,
            orphaned_blobs: orphaned,
        })
    }

    /// Remove blob files that have no corresponding payload row in the database.
    fn cleanup_orphaned_blobs(&self) -> rusqlite::Result<usize> {
        let mut stmt = self.conn.prepare("SELECT value FROM payloads WHERE storage = 'blob'")?;
        let referenced: std::collections::HashSet<String> = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();

        let mut removed = 0;
        if let Ok(entries) = std::fs::read_dir(&self.blobs_dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if !referenced.contains(name) {
                        if std::fs::remove_file(entry.path()).is_ok() {
                            removed += 1;
                        }
                    }
                }
            }
        }
        if removed > 0 {
            tracing::info!("cleanup_orphaned_blobs: removed {removed} orphaned blob files");
        }
        Ok(removed)
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

/// Escape special characters for CSV output (RFC 4180).
fn escape_csv(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
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

fn is_sqlite_busy(err: &rusqlite::Error) -> bool {
    matches!(err, rusqlite::Error::SqliteFailure(_, Some(ref msg)) if msg.contains("locked") || msg.contains("busy"))
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
            retention_days: 90,
        };
        store.save_settings(settings).unwrap();
        let loaded = store.get_settings().unwrap();
        assert_eq!(loaded.max_items, 500);
        assert_eq!(loaded.max_payload_bytes, 10 * 1024 * 1024);
        assert!(!loaded.trim_whitespace_for_text_dedup);
        assert!(loaded.use_sampling_hash);

        cleanup(&dir);
    }

    #[test]
    fn export_json_roundtrip() {
        let (store, dir) = temp_store();
        store.insert_clip(text_item("hello world")).unwrap();
        store.insert_clip(text_item("foo bar")).unwrap();

        let json = store.export_to_json(None, None, None, None).unwrap();
        assert!(json.contains("hello world"));
        assert!(json.contains("foo bar"));
        assert!(json.contains("\"version\": \"1.0.7\""));

        // Round-trip: export → import (replace mode)
        let result = store.import_from_json(&json, "replace").unwrap();
        assert!(result.added >= 2, "expected at least 2 added, got {}", result.added);

        cleanup(&dir);
    }

    #[test]
    fn export_csv_contains_expected_columns() {
        let (store, dir) = temp_store();
        store.insert_clip(text_item("test, with, commas")).unwrap();

        let csv = store.export_to_csv(None, None, None, None).unwrap();
        // Should have BOM
        assert!(csv.starts_with('\u{FEFF}'));
        // Should have header
        assert!(csv.contains("id,text_preview,kind,created_at,is_pinned"));
        // CSV escaping
        assert!(csv.contains('"'));

        cleanup(&dir);
    }

    #[test]
    fn import_merge_skips_duplicates() {
        let (store, dir) = temp_store();
        store.insert_clip(text_item("unique item")).unwrap();

        let json = store.export_to_json(None, None, None, None).unwrap();
        // Merge should skip the existing item
        let result = store.import_from_json(&json, "merge").unwrap();
        assert_eq!(result.skipped, 1);
        assert_eq!(result.added, 0);

        cleanup(&dir);
    }

    #[test]
    fn import_append_adds_duplicates() {
        let (store, dir) = temp_store();
        store.insert_clip(text_item_with_data("dup", "content1")).unwrap();

        let json = store.export_to_json(None, None, None, None).unwrap();
        let result = store.import_from_json(&json, "append").unwrap();
        assert!(result.added > 0);

        // Should now have 2 items
        let results = store.search("", 10, 0).unwrap();
        assert_eq!(results.len(), 2);

        cleanup(&dir);
    }

    #[test]
    fn import_replace_clears_first() {
        let (store, dir) = temp_store();
        store.insert_clip(text_item_with_data("old1", "data1")).unwrap();
        store.insert_clip(text_item_with_data("old2", "data2")).unwrap();

        // Export only one item, then replace — should leave only that one
        let ids: Vec<String> = store
            .search("old1", 1, 0)
            .unwrap()
            .iter()
            .map(|c| c.id.clone())
            .collect();
        let json = store
            .export_to_json(Some(ids), None, None, None)
            .unwrap();
        let result = store.import_from_json(&json, "replace").unwrap();
        assert_eq!(result.added, 1);

        let results = store.search("", 10, 0).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].text_preview, "old1");

        cleanup(&dir);
    }

    #[test]
    fn delete_by_date_range() {
        let (store, dir) = temp_store();
        store.insert_clip(text_item_with_data("item", "data-1")).unwrap();
        store.insert_clip(text_item_with_data("item", "data-2")).unwrap();

        // Delete all items in a wide date range
        let from = "2000-01-01T00:00:00Z";
        let to = "2100-01-01T00:00:00Z";
        let count = store.delete_by_date_range(from, to).unwrap();
        assert_eq!(count, 2);

        let results = store.search("", 10, 0).unwrap();
        assert_eq!(results.len(), 0);

        cleanup(&dir);
    }

    #[test]
    fn delete_selected() {
        let (store, dir) = temp_store();
        store.insert_clip(text_item_with_data("keep", "data-keep")).unwrap();
        store.insert_clip(text_item_with_data("del", "data-del")).unwrap();

        let all = store.search("", 10, 0).unwrap();
        let del_ids: Vec<String> = all
            .iter()
            .filter(|c| c.text_preview.contains("del"))
            .map(|c| c.id.clone())
            .collect();

        let count = store.delete_selected(&del_ids).unwrap();
        assert_eq!(count, 1);

        let results = store.search("", 10, 0).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].text_preview, "keep");

        cleanup(&dir);
    }

    #[test]
    fn delete_by_type_only_removes_target_kind() {
        let (store, dir) = temp_store();
        store.insert_clip(text_item("text clip")).unwrap();

        // Insert an image clip
        let image_item = ClipboardItem {
            kind: ClipKind::Image,
            text_preview: "[Image]".to_string(),
            payloads: vec![ClipboardPayload {
                uti: "image/png".to_string(),
                data: vec![1, 2, 3],
            }],
        };
        store.insert_clip(image_item).unwrap();

        // Delete only image type
        let count = store.delete_by_type(ClipKind::Image).unwrap();
        assert_eq!(count, 1);

        let results = store.search("", 10, 0).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].kind, ClipKind::Text);

        cleanup(&dir);
    }

    #[test]
    fn auto_prune_respects_retention() {
        let (store, dir) = temp_store();
        store
            .save_settings(AppSettings {
                retention_days: 90,
                ..Default::default()
            })
            .unwrap();

        store.insert_clip(text_item_with_data("recent", "data-recent")).unwrap();

        // With 0 retention days, nothing is pruned
        let count = store.auto_prune(0).unwrap();
        assert_eq!(count, 0);

        // With very large retention (9999 days), recent items survive
        let count = store.auto_prune(9999).unwrap();
        assert_eq!(count, 0);

        // Recent items should still exist
        let results = store.search("", 10, 0).unwrap();
        assert_eq!(results.len(), 1);

        cleanup(&dir);
    }

    #[test]
    fn disk_usage_reports_stats() {
        let (store, dir) = temp_store();
        store.insert_clip(text_item("hello")).unwrap();
        store.insert_clip(text_item("world")).unwrap();

        let usage = store.get_disk_usage().unwrap();
        assert_eq!(usage.total_items, 2);
        assert!(usage.total_bytes > 0);
        assert!(!usage.by_type.is_empty());
        assert_eq!(usage.by_age.len(), 3);

        cleanup(&dir);
    }

    #[test]
    fn count_by_type_returns_correct_count() {
        let (store, dir) = temp_store();
        store.insert_clip(text_item("text1")).unwrap();
        store.insert_clip(text_item("text2")).unwrap();

        let image_item = ClipboardItem {
            kind: ClipKind::Image,
            text_preview: "[Image]".to_string(),
            payloads: vec![ClipboardPayload {
                uti: "image/png".to_string(),
                data: vec![1, 2, 3],
            }],
        };
        store.insert_clip(image_item).unwrap();

        assert_eq!(store.count_by_type(ClipKind::Text).unwrap(), 2);
        assert_eq!(store.count_by_type(ClipKind::Image).unwrap(), 1);

        cleanup(&dir);
    }

    #[test]
    fn count_by_date_range_returns_correct_count() {
        let (store, dir) = temp_store();
        store.insert_clip(text_item("item1")).unwrap();
        store.insert_clip(text_item("item2")).unwrap();

        let count = store.count_by_date_range("2000-01-01T00:00:00Z", "2100-01-01T00:00:00Z").unwrap();
        assert_eq!(count, 2);

        let count = store.count_by_date_range("2000-01-01T00:00:00Z", "2000-01-02T00:00:00Z").unwrap();
        assert_eq!(count, 0);

        cleanup(&dir);
    }

    #[test]
    fn count_prunable_returns_correct_count() {
        let (store, dir) = temp_store();
        store.insert_clip(text_item("recent")).unwrap();

        assert_eq!(store.count_prunable(0).unwrap(), 0);
        assert_eq!(store.count_prunable(9999).unwrap(), 0);

        cleanup(&dir);
    }

    #[test]
    fn import_version_warning_for_newer_version() {
        let (store, dir) = temp_store();
        store.insert_clip(text_item("test")).unwrap();

        let mut json = store.export_to_json(None, None, None, None).unwrap();
        json = json.replace("\"1.0.7\"", "\"2.0.0\"");

        let result = store.import_from_json(&json, "merge").unwrap();
        assert!(result.version_warning.is_some());
        assert!(result.version_warning.unwrap().contains("2.0.0"));

        cleanup(&dir);
    }

    #[test]
    fn import_no_warning_for_same_version() {
        let (store, dir) = temp_store();
        store.insert_clip(text_item("test")).unwrap();

        let json = store.export_to_json(None, None, None, None).unwrap();
        let result = store.import_from_json(&json, "merge").unwrap();
        assert!(result.version_warning.is_none());

        cleanup(&dir);
    }

    #[test]
    fn settings_retention_days_default() {
        let (store, dir) = temp_store();
        let settings = store.get_settings().unwrap();
        assert_eq!(settings.retention_days, 90);

        store
            .save_settings(AppSettings {
                retention_days: 365,
                ..Default::default()
            })
            .unwrap();
        let settings = store.get_settings().unwrap();
        assert_eq!(settings.retention_days, 365);

        cleanup(&dir);
    }
}
