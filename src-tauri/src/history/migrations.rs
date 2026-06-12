use rusqlite::Connection;

/// List of all migrations in order. Each migration is (version, SQL).
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("migrations/001_add_tags.sql")),
    (2, include_str!("migrations/002_add_rules.sql")),
    (3, include_str!("migrations/003_add_clip_timestamps.sql")),
    (4, include_str!("migrations/004_add_indexes.sql")),
];

/// Ensure the `db_meta` table exists and return the current schema version.
fn current_version(conn: &Connection) -> rusqlite::Result<i64> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS db_meta (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    )?;

    let version: i64 = conn
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM db_meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    Ok(version)
}

/// Run all pending migrations inside a single transaction.
/// Returns the new schema version.
pub fn run_migrations(conn: &Connection) -> rusqlite::Result<i64> {
    let current = current_version(conn)?;
    let mut latest = current;

    for &(version, sql) in MIGRATIONS {
        if version <= current {
            continue;
        }
        tracing::info!("running migration {version}");

        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = conn.execute_batch(sql);

        if let Err(e) = result {
            let _ = conn.execute_batch("ROLLBACK");
            tracing::error!("migration {version} failed: {e}");
            return Err(e);
        }

        conn.execute(
            "INSERT OR REPLACE INTO db_meta (key, value) VALUES ('schema_version', ?1)",
            [version],
        )?;

        conn.execute_batch("COMMIT")?;
        tracing::info!("migration {version} applied");
        latest = version;
    }

    Ok(latest)
}

/// Create a timestamped backup of the database file before running migrations.
pub fn backup_database(db_path: &std::path::Path) -> std::io::Result<()> {
    if !db_path.exists() {
        return Ok(());
    }

    let parent = db_path.parent().unwrap_or(db_path);
    let timestamp = chrono_free_timestamp();
    let backup_name = format!("paste.db.backup.{timestamp}");
    let backup_path = parent.join(&backup_name);

    std::fs::copy(db_path, &backup_path)?;
    tracing::info!("database backed up to {}", backup_path.display());

    // Rotate: keep only the 3 most recent backups
    rotate_backups(parent, 3)?;

    Ok(())
}

/// Generate a timestamp string without pulling in chrono (YYYYMMDD_HHMMSS).
fn chrono_free_timestamp() -> String {
    use std::time::SystemTime;
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();

    // Simple conversion from unix timestamp to date components
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Civil date from days since epoch (based on Howard Hinnant's algorithm)
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!(
        "{:04}{:02}{:02}_{:02}{:02}{:02}",
        y, m, d, hours, minutes, seconds
    )
}

/// Remove old backup files, keeping only the `keep` most recent ones.
fn rotate_backups(dir: &std::path::Path, keep: usize) -> std::io::Result<()> {
    let mut backups: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .map_or(false, |n| n.starts_with("paste.db.backup"))
        })
        .collect();

    backups.sort_by(|a, b| b.file_name().cmp(&a.file_name()));

    for entry in backups.into_iter().skip(keep) {
        let _ = std::fs::remove_file(entry.path());
    }

    Ok(())
}

/// Run PRAGMA integrity_check and log the result.
pub fn check_integrity(conn: &Connection) -> rusqlite::Result<()> {
    let result: String =
        conn.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if result != "ok" {
        tracing::warn!("integrity check returned: {result}");
    } else {
        tracing::info!("database integrity check passed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn fresh_db_starts_at_version_0() {
        let conn = Connection::open_in_memory().unwrap();
        let version = current_version(&conn).unwrap();
        assert_eq!(version, 0);
    }

    /// Helper: create the clips table that migrations expect to already exist.
    fn ensure_clips_table(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS clips (
                id TEXT PRIMARY KEY,
                created_at TEXT NOT NULL,
                kind TEXT NOT NULL,
                text_preview TEXT NOT NULL,
                payload_ref TEXT,
                pasteboard_hash TEXT NOT NULL UNIQUE,
                source_app TEXT,
                is_pinned INTEGER NOT NULL DEFAULT 0
            );"
        ).unwrap();
    }

    #[test]
    fn migrations_run_sequentially() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON").unwrap();
        ensure_clips_table(&conn);
        let version = run_migrations(&conn).unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
    }

    #[test]
    fn migration_creates_tables() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON").unwrap();
        ensure_clips_table(&conn);
        run_migrations(&conn).unwrap();

        // Check tags table exists
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tags", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);

        // Check rules table exists
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM rules", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);

        // Check clip_tags table exists
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM clip_tags", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn backup_and_rotate_works() {
        let dir = std::env::temp_dir().join(format!("paste-backup-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("paste.db");
        std::fs::write(&db_path, "test").unwrap();

        // Create 5 backups
        for i in 0..5 {
            let name = format!("paste.db.backup.2026060{i}1_120000");
            std::fs::write(dir.join(&name), "backup").unwrap();
        }

        backup_database(&db_path).unwrap();

        // Should keep only 3 + the new one = 4 total
        let count = std::fs::read_dir(&dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_str()
                    .unwrap()
                    .starts_with("paste.db.backup")
            })
            .count();

        assert!(count <= 4);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
