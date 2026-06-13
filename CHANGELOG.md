# Changelog

All notable changes to Paste are documented in this file.

---

## [1.1.0] — 2026-08-01

### Added

- **Dark/Light theme toggle** — system-aware theme that follows OS dark/light mode, with manual override cycling (System → Dark → Light → System). Theme preference persists across restarts. CSS custom properties for all colors ensure consistent theming.
- **Compact mode** — toggle from the header bar reduces clip item height by ~35%, showing more items on screen. Preference persists across restarts.
- **Clip preview modal** — right-click a clip and select "View Full Content" to open a full-screen preview modal. Shows rendered content (text or image), metadata (timestamp, type, pin status, tags), and action buttons (Copy, Paste, Pin, Close).
- **Accessibility improvements** — ARIA attributes on clip list (`role="listbox"`, `aria-selected`), search input (`aria-label`), context menu (`role="menu"`), preview modal (`role="dialog"`, `aria-modal`), and footer (`aria-label`). Updated keyboard shortcut hints to show all available shortcuts.

### Changed

- **CSS custom properties** — all colors now use CSS variables (`--text`, `--bg-panel`, `--border`, etc.) enabling proper dark/light theming. Dark theme is the default.
- **Search row grid** — updated to 4-column grid to accommodate compact and theme toggle buttons.

---

## [1.0.9] — 2026-07-15

### Added

- **Database verification** — "Verify Database" button in Settings > About runs `PRAGMA integrity_check`, cleans up orphaned blob files, and reports results. Auto-repair via `REINDEX` if corruption detected.
- **Orphaned blob cleanup** — on startup, blob files with no corresponding database record are automatically removed.
- **Clipboard read retry** — watcher retries clipboard read once with 100ms backoff on transient failures.
- **SQLITE_BUSY retry** — `insert_clip` retries up to 3 times with exponential backoff when the database is locked.

### Changed

- **Sampling hash enabled by default** — `use_sampling_hash` now defaults to `true` for new installations. Large items (>256KB) use sampling hash (head 64KB + tail 64KB + length) for 75% faster deduplication.
- **WAL checkpoint on startup** — `PRAGMA wal_checkpoint(TRUNCATE)` runs at app startup to bound WAL file growth. Periodic checkpoint also runs every 10 inserts alongside the existing prune cycle.
- **Auto-repair on integrity failure** — if `PRAGMA integrity_check` fails at startup, the system automatically attempts `REINDEX` and re-checks.
- **Tag query optimization** — `attach_tags_to_clips()` now uses a single batched JOIN query instead of N+1 per-clip queries.
- **Search SQL optimization** — structured-filter queries now include a SQL `LIMIT` clause (5x requested page size) to avoid fetching all rows when combined with free-text fuzzy matching.

---

## [1.0.8] — 2026-07-01

### Added

- **Advanced search syntax** — search queries now support structured filters: `tag:name`, `type:text`, `date:today/week/month`, `pinned:true`, `size:>1MB`. Filters are applied as SQL WHERE clauses before fuzzy text matching, dramatically improving search performance on large datasets. Supports negation (`-tag:spam`, `-type:image`), date ranges (`date:2026-01-01..2026-06-01`), and `tag:*` for "has any tag".
- **Rule execution engine** — enabled rules are now automatically evaluated on every new clipboard item. Pattern types: regex, literal, URL, email. Actions: auto-tag (creates tag if missing), delete (removes clip), notify (logs action). Rules are processed in priority order. "Run Rules on All Clips" button in Settings applies rules retroactively.
- **Tag display on clips** — clips now show colored tag badges in the preview. Click a tag badge to filter by that tag.
- **Right-click context menu** — right-click any clip for quick actions: Paste, Copy, Pin/Unpin, Edit Tags, Export Item, Delete.
- **Search history** — last 20 searches are saved to localStorage. A dropdown appears below the search bar on focus, showing recent queries. Click to repeat.
- **Keyboard shortcuts** — `Delete`/`Backspace` to delete selected clip, `Cmd+F` to focus search, `Cmd+P` to pin/unpin, `Shift+Enter` to toggle multi-select mode.
- **`update_tag`** — tags can now be renamed and recolored via backend command.
- **`batch_apply_rules`** — new Tauri command to run all enabled rules against all existing clips.
- **`apply_rules_to_clip`** — new Tauri command to run rules against a specific clip.

### Changed

- **`ClipSummary` now includes `tags`** — the search results now return tag names for each clip, displayed as badges.
- **`insert_clip` returns `clip_id`** — changed from `Result<()>` to `Result<String>` to support rule execution after insertion.
- **Search architecture** — non-empty queries now use SQL-level filtering for structured filters, with fuzzy matching only on the free-text portion of filtered results.

---

## [1.0.7] — 2026-06-17

### Added

- **Export/Import system** — clipboard history can now be exported as JSON or CSV and imported from JSON or CSV backups. Supports three import modes: merge (skip duplicates), replace (clear then import), and append (allow duplicates). Export filters allow selecting by date range and content type.
- **Tags and Rules CRUD** — full create/read/update/delete for tags (with color) and rules (with pattern matching by literal, regex, URL, or email). Tags can be assigned to clips; rules can trigger tag, delete, or notify actions. Schema tables created via migration in v1.0.7; CRUD and management UI now complete.
- **Data management tools** — selective deletion by date range, by type, or by selecting specific items. Configurable auto-prune retention policy (default 90 days) with manual "Clean Up Old Items" button.
- **Multi-select mode** — toggle select mode from the clip list to bulk-select items with checkboxes. Select All / Deselect All / Delete Selected actions available in a floating action bar.
- **Disk usage statistics** — storage overview showing total item count, total size, breakdown by type (text/HTML/RTF/image/file), and breakdown by age (<30d, 30-90d, >90d). Shows optimization suggestions when storage exceeds 512 MB or 1 GB.
- **Database schema for tags and rules** — new `tags`, `clip_tags`, and `rules` tables via migration framework. Tags and rules are stored but not yet exposed in the UI.
- **Database migration framework** — sequential SQL migrations stored in `src-tauri/src/history/migrations/`, with automatic backup before migration (keeps last 3 backups), transaction-wrapped execution, and integrity checks on startup.
- **`retentionDays` setting** — configurable retention period in Preferences (0 = never delete). Used by auto-prune to clean items older than the specified number of days.
- **Loading overlay** — long operations (export, import, bulk delete) now show a loading spinner overlay with status message.
- **Error report** — "Copy Error Report" button in Settings > About copies version, platform, storage stats, and settings to clipboard for bug reports.

### Changed

- **AppSettings extended** — new `retentionDays` field (default 90) with `#[serde(default)]` for backward compatibility with existing saved settings.
- **Settings UI redesigned** — divided into sections (Storage, Deduplication, Export/Import, Data Management, Tags, Rules, About) with action buttons and result feedback.

### Fixed

- **Corner cases in prune** — existing `max_items`-based prune now correctly handles edge cases with pinned items and blob files.

## [1.0.6] — 2026-05-14

### Added

- **Sampling hash for large items** — items larger than 1MB use a sampling hash (head 64KB + tail 64KB + total length) instead of full SHA-256, reducing CPU usage. Controlled by `useSamplingHash` setting (off by default).
- **Virtual scrolling / pagination** — a "Load more" button at the bottom of the list replaces the hard 40-item limit. Browsing through 1000+ history items is now practical.
- **Search result highlighting** — matching characters in search results are highlighted in yellow.
- **Empty state guide** — first-time users see a welcome screen with step-by-step instructions and keyboard shortcut hints.
- **Keyboard shortcut hints** — a footer bar displays available shortcuts (↑↓ Navigate, 1-9 Quick Paste, Enter Paste, Esc Close).
- **List item tooltips** — hovering over a clip preview shows the full text and timestamp.

### Changed

- **insert_clip uses explicit transactions** — all DB writes for a single clip insert are wrapped in `BEGIN IMMEDIATE` / `COMMIT`, with rollback on error. Blob files are written after commit to keep file I/O outside the transaction.
- **Paste timing uses changeCount polling** — the fixed 80ms sleep is replaced by polling the clipboard `changeCount` (up to 200ms, 15ms steps) to confirm the write completed before sending the paste keystroke.
- **WAL checkpoint on settings save** — `PRAGMA wal_checkpoint(TRUNCATE)` runs when settings are saved, bounding WAL file growth.

### Fixed

- **search() returns all clips for fuzzy search before applying offset/limit** — pagination now works correctly with search queries (previously only empty queries respected limit).

### Windows

- ClipboardBridge implementation (read/write/changeCount/paste keystroke) using Win32 API.
- Windows CI job in release workflow produces MSI and NSIS installers.
- Platform-specific shortcut: `Ctrl+Shift+V` on Windows, `Cmd+Shift+V` on macOS.
- Known limitations documented in `docs/windows-support-plan.md`.

---

## [0.1.0] — 2026-04-29

### Initial release

- Menu bar app with global shortcut `Cmd+Shift+V`.
- Spotlight-style floating panel with frosted glass dark theme.
- Fuzzy search via skim algorithm with smart case matching.
- Quick paste with number keys 1-9.
- Multi-type support: plain text, RTF, HTML, PNG/TIFF images, file URLs.
- SQLite storage with WAL mode, blob files for large payloads (>256KB).
- Deduplication via SHA-256 hash with optional whitespace trimming.
- Pin and delete for individual clips.
- Configurable settings: max items (50-10,000), max payload size (1-500MB), trim whitespace for dedup.
- Accessibility permission management with guided setup.
- Unit tests for storage, search, and text utilities.
- macOS universal binary (x86_64 + arm64) CI/CD via GitHub Actions.
