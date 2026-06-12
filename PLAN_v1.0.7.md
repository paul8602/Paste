# Paste v1.0.7 — Data Foundation
**Release Date**: 2026-06-17
**Duration**: 2 weeks (2026-06-04 → 2026-06-17)
**Goal**: Strengthen data management and prepare for smart features

---

## Version Overview

v1.0.7 focuses on building a solid data foundation for future smart features. This release adds export/import functionality, refactors the database schema for extensibility, and introduces basic data management tools.

### Key Deliverables
- ✅ Complete export/import system (JSON/CSV)
- ✅ Database schema refactoring for tags and rules
- ✅ Selective deletion and auto-pruning
- ✅ Improved data reliability and backup mechanisms

---

## Feature Breakdown

### 1. Export/Import System
**Priority**: HIGH | **Effort**: 30 hours

#### Export Functionality
- [x] Export clipboard history as JSON format
  - Preserve all metadata (timestamp, type, pinned status)
  - Include tags (empty array for now)
  - Base64 encode binary data (images, files)
  - Progress indicator for large exports (>100 items)

- [x] Export clipboard history as CSV format
  - Columns: id, content_preview, type, created_at, pinned
  - UTF-8 encoding with BOM for Excel compatibility
  - Escape special characters properly

- [x] Export options
  - Export all items
  - Export selected items (multi-select)
  - Export by date range
  - Export by type (text, HTML, RTF, images, URLs)

#### Import Functionality
- [x] Import from JSON backup file
  - Validate JSON schema
  - Detect duplicate items (by hash)
  - Progress indicator with cancel option

- [x] Import modes
  - **Merge**: Add new items, skip duplicates
  - **Replace**: Delete existing, import fresh
  - **Append**: Add all items regardless of duplicates

- [x] Import validation
  - Check file format and schema version
  - Warn if importing from newer version
  - Report import summary (added, skipped, failed)

#### Testing
- [x] Unit tests for JSON serialization/deserialization
- [x] Unit tests for CSV parsing
- [x] Round-trip test: export → import → verify identical data
- [x] Edge cases: empty export, corrupted file, version mismatch

---

### 2. Database Refactoring
**Priority**: HIGH | **Effort**: 25 hours

#### Schema Changes
- [x] Add `tags` table
  ```sql
  CREATE TABLE IF NOT EXISTS tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    color TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
  );
  ```

- [x] Add `clip_tags` junction table
  ```sql
  CREATE TABLE IF NOT EXISTS clip_tags (
    clip_id INTEGER NOT NULL,
    tag_id INTEGER NOT NULL,
    PRIMARY KEY (clip_id, tag_id),
    FOREIGN KEY (clip_id) REFERENCES clips(id) ON DELETE CASCADE,
    FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE
  );
  ```

- [x] Add `rules` table
  ```sql
  CREATE TABLE IF NOT EXISTS rules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    pattern TEXT NOT NULL,
    pattern_type TEXT CHECK(pattern_type IN ('regex', 'literal', 'url', 'email')),
    action TEXT CHECK(action IN ('tag', 'delete', 'notify')),
    action_value TEXT,
    enabled BOOLEAN DEFAULT 1,
    priority INTEGER DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
  );
  ```

- [x] Add timestamps to existing `clips` table
  - `created_at DATETIME DEFAULT CURRENT_TIMESTAMP`
  - `modified_at DATETIME DEFAULT CURRENT_TIMESTAMP`

#### Migration System
- [x] Create migration framework
  - Store current schema version in `db_meta` table
  - Sequential migration files (001_add_tags.sql, 002_add_rules.sql, etc.)
  - Run migrations on app startup

- [x] Implement safe migrations
  - Backup database before migration
  - Wrap migrations in transactions
  - Rollback on failure
  - Log migration status

- [x] Add indexes for performance
  - `idx_clips_created_at` on clips(created_at)
  - `idx_clips_type` on clips(type)
  - `idx_clips_pinned` on clips(pinned)
  - `idx_clip_tags_clip_id` on clip_tags(clip_id)
  - `idx_clip_tags_tag_id` on clip_tags(tag_id)

#### Data Integrity
- [x] Add database backup on startup
  - Copy `paste.db` to `paste.db.backup`
  - Keep last 3 backups with rotation

- [x] Implement integrity checks
  - PRAGMA integrity_check on startup
  - Fix common issues automatically
  - Warn user if corruption detected

#### Testing
- [x] Unit tests for migration framework
- [x] Test migration from v1.0.6 schema
- [x] Test rollback on migration failure
- [x] Performance test: indexes improve query speed

---

### 3. Data Management Tools
**Priority**: MEDIUM | **Effort**: 20 hours

#### Selective Deletion
- [x] Delete by date range
  - Date picker: "From" and "To"
  - Preview count before deletion
  - Confirmation dialog with item count

- [x] Bulk delete selected items
  - Multi-select with checkboxes
  - Select all / deselect all
  - Delete confirmation with count

- [x] Delete by type
  - Filter by content type
  - Select type → preview items → confirm delete

#### Auto-Prune System
- [x] Configurable retention policy
  - Options: 30 days, 60 days, 90 days, 6 months, 1 year, Never
  - Default: 90 days
  - Setting in Preferences

- [x] Auto-prune execution
  - Run on app startup (if more than 24h since last run)
  - Run manually via "Clean Up Old Items" button
  - Show preview before deletion
  - Never delete pinned items

- [x] Prune notifications
  - Notify when auto-prune runs (optional)
  - Summary: "Deleted 45 items older than 90 days"

#### Disk Usage Statistics
- [x] Show storage overview
  - Total items count
  - Total storage used (MB/GB)
  - Breakdown by type (text, HTML, images, files)
  - Breakdown by age (<30 days, 30-90 days, >90 days)

- [x] Storage optimization
  - Suggest cleanup if >1GB used
  - Compress old images (optional)
  - Remove orphaned blob files

#### Testing
- [x] Unit tests for selective deletion
- [x] Unit tests for auto-prune logic
- [x] Test disk usage calculation
- [x] Test bulk delete performance (1000+ items)

---

### 4. Bug Fixes & Polish
**Priority**: MEDIUM | **Effort**: 5 hours

#### Clipboard Monitoring Stability
- [x] Fix race conditions in clipboard change detection
- [x] Add debounce to prevent rapid duplicate writes
- [x] Improve error handling for permission issues

#### Error Handling
- [x] Add try-catch blocks for all database operations
- [x] Implement error logging to file
- [x] Show user-friendly error messages
- [x] Add "Send Error Report" option

#### UI Polish
- [x] Add progress indicators for long operations
- [x] Improve loading states
- [x] Add confirmation dialogs for destructive actions
- [x] Toast notifications for success/error messages

---

## Technical Details

### Database Changes
- **New Tables**: 3 (tags, clip_tags, rules)
- **New Indexes**: 5
- **Migration Required**: Yes (v1.0.6 → v1.0.7)
- **Backward Compatible**: Yes (additive changes only)

### API Changes
- **New Functions**:
  - `exportToJSON(options): Promise<ExportResult>`
  - `exportToCSV(options): Promise<ExportResult>`
  - `importFromJSON(file, mode): Promise<ImportResult>`
  - `deleteByDateRange(from, to): Promise<number>`
  - `deleteSelected(ids: number[]): Promise<number>`
  - `autoPrune(retentionDays): Promise<number>`
  - `getDiskUsage(): Promise<DiskUsage>`

### Performance Targets
- Export 10,000 items to JSON: <5 seconds
- Import 10,000 items from JSON: <10 seconds
- Database migration: <2 seconds
- Auto-prune 1,000 items: <1 second

### Dependencies
- Tauri 2.x (no changes)
- SQLite 3.x (no changes)
- No new external dependencies

---

## Milestones

### Week 1 (June 4-10)
**Focus**: Database refactoring and export system

| Day | Task |
|-----|------|
| Day 1-2 | Database schema design and migration framework |
| Day 3 | Add new tables (tags, rules) and indexes |
| Day 4-5 | Implement JSON export functionality |
| Day 5 | Implement CSV export functionality |

**Deliverable**: Working export system with new database schema

### Week 2 (June 11-17)
**Focus**: Import system, data management, and polish

| Day | Task |
|-----|------|
| Day 6-7 | Implement import functionality (JSON) |
| Day 8 | Selective deletion and bulk delete |
| Day 9 | Auto-prune system and disk usage stats |
| Day 10 | Testing, bug fixes, and documentation |

**Deliverable**: Complete v1.0.7 release

---

## Testing Plan

### Unit Tests
- [x] Export/Import serialization (JSON, CSV)
- [x] Database migration framework
- [x] Selective deletion logic
- [x] Auto-prune algorithm
- [x] Disk usage calculation

### Integration Tests
- [x] End-to-end export → import round-trip
- [x] Migration from v1.0.6 database
- [x] Large dataset operations (10,000+ items)

### Manual Testing
- [x] Export and import on macOS
- [x] Export and import on Windows
- [x] Database migration upgrade path
- [x] Auto-prune with various retention settings
- [x] UI/UX for all new features

### Performance Tests
- [x] Export/import speed with large datasets
- [x] Database migration speed
- [x] Bulk delete performance
- [x] Memory usage during operations

---

## Risk Assessment

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| Database migration fails | Low | High | Backup before migration, rollback support |
| Export format incompatibility | Medium | Medium | Strict schema validation, version field |
| Import data corruption | Low | High | Validate all data, skip invalid items |
| Performance issues with large exports | Medium | Medium | Progress indicators, chunked processing |
| Breaking changes break v1.0.6 compatibility | Low | High | Additive-only changes, thorough testing |

---

## Rollback Plan

If critical issues are discovered after release:

1. **Database Migration Issues**
   - Users can restore from `paste.db.backup`
   - Release hotfix with improved migration

2. **Export/Import Issues**
   - Document known limitations
   - Provide workaround (manual JSON editing)
   - Fix in v1.0.8

3. **Performance Regressions**
   - Disable auto-prune by default
   - Add "lite mode" for large collections
   - Optimize in v1.0.9

---

## Documentation

### User-Facing
- [x] Update CHANGELOG.md
- [x] Add "Export/Import" section to README
- [x] Add "Data Management" guide
- [x] Update keyboard shortcuts list

### Developer
- [x] Document database schema changes
- [x] Document migration framework
- [x] Update API documentation
- [x] Add code comments for new features

---

## Success Criteria

- [x] Export/import works for 10,000+ items without errors
- [x] Database migration completes in <5 seconds
- [x] Auto-prune correctly removes old items
- [x] No data loss during migration
- [x] All unit tests pass
- [x] No critical bugs reported in first week

---

## Post-Release Tasks

- [x] Monitor error reports
- [x] Gather user feedback on export/import
- [x] Document issues for v1.0.8
- [x] Plan v1.0.8 features based on feedback

---

## Notes

- **Backward Compatibility**: All database changes are additive. Users can downgrade to v1.0.6 if needed (though tags/rules data will be lost).

- **Feature Flags**: Consider adding feature flag for auto-prune to allow disabling if issues arise.

- **Performance**: Export/import operations should show progress and allow cancellation for large datasets.

- **Testing Priority**: Focus testing on database migration path (v1.0.6 → v1.0.7) as this is highest risk.

---

*Document Created: 2026-06-03*
*Last Updated: 2026-06-12*
*Status: COMPLETE*
*Version: 1.0.7*
