# Changelog

All notable changes to Paste are documented in this file.

---

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
