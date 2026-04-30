use std::fs;
use std::path::PathBuf;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub max_items: usize,
    pub max_payload_bytes: usize,
    pub trim_whitespace_for_text_dedup: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            max_items: 1000,
            max_payload_bytes: 25 * 1024 * 1024,
            trim_whitespace_for_text_dedup: true,
        }
    }
}

pub struct HistoryStore {
    conn: Connection,
    blobs_dir: PathBuf,
}

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

        Ok(Self { conn, blobs_dir })
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

        let clip_id = self
            .conn
            .query_row("SELECT id FROM clips WHERE pasteboard_hash = ?1", params![hash], |row| row.get::<_, String>(0))?;

        self.conn
            .execute("DELETE FROM payloads WHERE clip_id = ?1", params![clip_id])?;

        for (position, payload) in item.payloads.into_iter().enumerate() {
            let (storage, value) = self.persist_payload(&clip_id, &payload, &settings)?;
            self.conn.execute(
                "
                INSERT INTO payloads (clip_id, uti, storage, value, position)
                VALUES (?1, ?2, ?3, ?4, ?5)
                ",
                params![clip_id, payload.uti, storage, value, position as i64],
            )?;
        }

        self.prune(settings.max_items)?;
        Ok(())
    }

    pub fn search(&self, query: &str, limit: usize) -> rusqlite::Result<Vec<ClipSummary>> {
        let mut stmt = self.conn.prepare(
            "
            SELECT id, created_at, kind, text_preview, payload_ref, is_pinned
            FROM clips
            ORDER BY is_pinned DESC, created_at DESC
            LIMIT 1000
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

        if !query.trim().is_empty() {
            clips = clips
                .into_iter()
                .filter_map(|clip| score_clip(query, &clip.text_preview).map(|score| (clip, score)))
                .collect::<Vec<_>>()
                .sorted_by_score()
                .into_iter()
                .map(|(clip, _score)| clip)
                .collect();
        }

        clips.truncate(limit);
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

    pub fn pin_clip(&self, id: &str, pinned: bool) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE clips SET is_pinned = ?2 WHERE id = ?1",
            params![id, if pinned { 1 } else { 0 }],
        )?;
        Ok(())
    }

    pub fn get_settings(&self) -> rusqlite::Result<AppSettings> {
        let value = self
            .conn
            .query_row("SELECT value FROM settings WHERE key = 'app'", [], |row| row.get::<_, String>(0))
            .optional()?;

        Ok(value
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default())
    }

    pub fn save_settings(&self, settings: AppSettings) -> rusqlite::Result<AppSettings> {
        let normalized = AppSettings {
            max_items: settings.max_items.clamp(50, 10_000),
            max_payload_bytes: settings.max_payload_bytes.clamp(1024 * 1024, 500 * 1024 * 1024),
            trim_whitespace_for_text_dedup: settings.trim_whitespace_for_text_dedup,
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
        Ok(normalized)
    }

    fn persist_payload(
        &self,
        clip_id: &str,
        payload: &ClipboardPayload,
        _settings: &AppSettings,
    ) -> rusqlite::Result<(String, String)> {
        if payload.is_blob_candidate() || payload.data.len() > 256 * 1024 {
            let file_name = format!("{}-{}.bin", clip_id, sanitize_uti(&payload.uti));
            fs::write(self.blobs_dir.join(&file_name), &payload.data).map_err(to_sql_error)?;
            return Ok(("blob".to_string(), file_name));
        }

        Ok((
            "inline".to_string(),
            base64::engine::general_purpose::STANDARD.encode(&payload.data),
        ))
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
            hasher.update(&payload.data);
        }

        hex::encode(hasher.finalize())
    }

    fn prune(&self, max_items: usize) -> rusqlite::Result<()> {
        let mut stmt = self.conn.prepare(
            "
            SELECT id
            FROM clips
            WHERE is_pinned = 0
            ORDER BY created_at DESC
            LIMIT -1 OFFSET ?1
            ",
        )?;
        let stale_ids = stmt
            .query_map(params![max_items as i64], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        for id in stale_ids {
            self.delete_clip(&id)?;
        }

        Ok(())
    }

    fn delete_blob_files(&self, id: &str) -> rusqlite::Result<()> {
        if !self.blobs_dir.exists() {
            return Ok(());
        }

        for entry in fs::read_dir(&self.blobs_dir).map_err(to_sql_error)? {
            let entry = entry.map_err(to_sql_error)?;
            if entry.file_name().to_string_lossy().starts_with(id) {
                let _ = fs::remove_file(entry.path());
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

fn to_sql_error(error: std::io::Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(error))
}
