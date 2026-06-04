# Paste v1.0.8 — Smart Organization
**Release Date**: 2026-07-01
**Duration**: 2 weeks (2026-06-18 → 2026-07-01)
**Goal**: Add intelligent organization and advanced search features

---

## Version Overview

v1.0.8 builds on the data foundation from v1.0.7 to add smart organization features. This release introduces tags, clipboard rules, and advanced search syntax, transforming Paste from a simple clipboard manager into an intelligent productivity tool.

### Key Deliverables
- ✅ Complete tag system (add, remove, filter, search)
- ✅ Clipboard rules engine with auto-tagging
- ✅ Advanced search syntax (tag:, type:, date:)
- ✅ Right-click context menu and keyboard navigation
- ✅ Search history and suggestions

---

## Feature Breakdown

### 1. Tag System
**Priority**: HIGH | **Effort**: 35 hours

#### Tag Management
- [ ] Create new tags
  - Name: required, max 50 characters, unique
  - Color: optional, hex color picker (12 preset colors)
  - Created date: auto-generated

- [ ] Edit existing tags
  - Rename tag (with uniqueness validation)
  - Change color
  - View tag usage count

- [ ] Delete tags
  - Confirmation dialog
  - Option to remove tag from all clips or delete clips with only this tag
  - Cannot delete tags used in active rules

#### Tag Operations on Clips
- [ ] Add tags to individual clips
  - Multi-select tags from dropdown
  - Quick add: type tag name, autocomplete suggests existing tags
  - Max 10 tags per clip

- [ ] Remove tags from clips
  - Click X on tag badge
  - Remove all tags option

- [ ] Display tags on clips
  - Tag badges with colors in clip preview
  - Hover to see full tag name
  - Click tag to filter by that tag

#### Tag Filtering
- [ ] Filter clips by tag
  - Single tag filter
  - Multiple tag filter (AND/OR logic)
  - Exclude tags (NOT filter)
  - Show untagged items

- [ ] Tag statistics
  - Count of clips per tag
  - Most used tags
  - Recently used tags

#### Auto-Suggest Tags
- [ ] Smart tag suggestions based on content
  - URLs → suggest "link" tag
  - Email addresses → suggest "email" tag
  - Phone numbers → suggest "phone" tag
  - Code snippets → suggest "code" tag (detected by syntax)

- [ ] Machine learning (simple)
  - Remember user's tag patterns
  - Suggest tags based on similar content
  - Improve suggestions over time

#### Testing
- [ ] Unit tests for tag CRUD operations
- [ ] Unit tests for tag filtering logic
- [ ] Test tag auto-suggest accuracy
- [ ] Performance test: 1000+ tags, 10,000+ clips
- [ ] Edge cases: duplicate names, special characters, max tags per clip

---

### 2. Clipboard Rules Engine
**Priority**: HIGH | **Effort**: 30 hours

#### Rule Creation
- [ ] Rule configuration UI
  - Name: required, descriptive
  - Pattern: required (see pattern types below)
  - Pattern type: dropdown (regex, literal, url, email)
  - Action: dropdown (auto-tag, delete, notify)
  - Action value: tag name (for auto-tag) or notification message
  - Priority: number (higher = processed first)
  - Enabled: toggle

#### Pattern Types
- [ ] **Regex patterns**
  - Full regex support (JavaScript flavor)
  - Test pattern against sample text
  - Common patterns library:
    - URLs: `https?:\/\/[^\s]+`
    - Emails: `[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}`
    - Phone: `\+?[\d\s\-\(\)]{7,15}`
    - IP addresses: `\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}`

- [ ] **Literal patterns**
  - Exact string match
  - Case-sensitive option
  - Contains / starts with / ends with

- [ ] **URL patterns**
  - Match domain: `example.com`
  - Match path: `/api/v1/`
  - Match protocol: `https://`

- [ ] **Email patterns**
  - Match domain: `@company.com`
  - Match address: `user@example.com`

#### Rule Actions
- [ ] **Auto-tag**
  - Add specified tag to matching clips
  - Multiple tags allowed
  - Don't add if tag already exists

- [ ] **Delete**
  - Auto-delete matching clips (with confirmation dialog)
  - Move to trash first (recoverable for 30 days)
  - Log deleted items

- [ ] **Notify**
  - Show notification when rule matches
  - Custom notification message
  - Sound alert (optional)

#### Rule Management
- [ ] Rule list view
  - Show all rules with status
  - Enable/disable toggle
  - Priority drag-and-drop reordering
  - Filter by action type

- [ ] Rule testing
  - Test rule against existing clips
  - Preview matches before applying
  - Show count of affected clips

- [ ] Rule import/export
  - Export rules as JSON
  - Import rules from JSON
  - Merge or replace options

#### Rule Execution
- [ ] Real-time processing
  - Run rules on new clips immediately
  - Process in priority order
  - Stop on first match (configurable)

- [ ] Batch processing
  - Run rules on existing clips
  - Progress indicator
  - Undo support (keep original state)

#### Testing
- [ ] Unit tests for each pattern type
- [ ] Unit tests for rule actions
- [ ] Test rule priority and execution order
- [ ] Test batch processing performance
- [ ] Edge cases: empty patterns, special characters, concurrent rules

---

### 3. Advanced Search Syntax
**Priority**: HIGH | **Effort**: 25 hours

#### Search Filters
- [ ] **Tag filter**: `tag:name`
  - Single tag: `tag:important`
  - Multiple tags: `tag:important tag:work`
  - Exclude tag: `-tag:spam`
  - Any tag: `tag:*` (has at least one tag)

- [ ] **Type filter**: `type:text/html/url/image/file`
  - Single type: `type:image`
  - Multiple types: `type:text type:html`
  - Exclude type: `-type:image`

- [ ] **Date filter**: `date:today/week/month/year`
  - Relative: `date:today`, `date:week`, `date:month`
  - Absolute: `date:2026-06-01`
  - Range: `date:2026-06-01..2026-06-15`
  - Before/after: `date:<2026-06-01`, `date:>2026-06-01`

- [ ] **Pinned filter**: `pinned:true/false`
  - Only pinned: `pinned:true`
  - Only unpinned: `pinned:false`

- [ ] **Size filter**: `size:>100KB`, `size:<1MB`
  - Greater than: `size:>100KB`
  - Less than: `size:<1MB`
  - Range: `size:100KB..1MB`
  - Units: B, KB, MB, GB

#### Combined Filters
- [ ] AND logic (default)
  - `tag:work type:email` → clips with "work" tag AND type "email"

- [ ] OR logic with pipe
  - `tag:work | tag:personal` → clips with either tag

- [ ] Grouping with parentheses
  - `(tag:work | tag:personal) type:text` → text clips with either tag

- [ ] Negation
  - `-tag:spam -type:image` → exclude spam-tagged and images

#### Search UI Enhancements
- [ ] Autocomplete for filters
  - Suggest tag names after `tag:`
  - Suggest types after `type:`
  - Suggest date shortcuts after `date:`

- [ ] Filter chips
  - Visual representation of active filters
  - Click to remove filter
  - Drag to reorder

- [ ] Syntax highlighting
  - Color-code filter keywords
  - Highlight syntax errors

#### Testing
- [ ] Unit tests for each filter type
- [ ] Test combined filters
- [ ] Test edge cases (invalid syntax, special characters)
- [ ] Performance test: search 10,000+ clips

---

### 4. Search Enhancements
**Priority**: MEDIUM | **Effort**: 15 hours

#### Search History
- [ ] Track recent searches
  - Store last 20 searches
  - Include filters and text queries
  - Timestamp each search

- [ ] Search history UI
  - Dropdown below search bar
  - Show recent searches with timestamps
  - Click to repeat search
  - Clear history option

- [ ] Persistence
  - Save history to local storage
  - Sync across app restarts

#### Search Suggestions
- [ ] Content-based suggestions
  - Suggest common words from clip content
  - Suggest frequently searched terms
  - Real-time suggestions as you type

- [ ] Filter suggestions
  - Suggest frequently used tags
  - Suggest common filter combinations
  - "Did you mean?" for typos

#### Improved Highlighting
- [ ] Highlight matched terms in results
  - Yellow background for matches
  - Bold matched characters
  - Highlight in both preview and full view

- [ ] Highlight syntax errors
  - Red underline for invalid filters
  - Tooltip with error message
  - Suggest correction

#### Testing
- [ ] Unit tests for search history storage
- [ ] Test suggestion algorithm
- [ ] Test highlighting accuracy
- [ ] UI tests for search interactions

---

### 5. Quick Actions & Context Menu
**Priority**: MEDIUM | **Effort**: 15 hours

#### Right-Click Context Menu
- [ ] Clip actions
  - **Copy to clipboard**: Copy clip content
  - **Paste to active app**: Paste immediately
  - **Delete clip**: Remove with confirmation
  - **Edit tags**: Open tag manager
  - **Export single item**: Save as file
  - **View full content**: Open preview modal
  - **Pin/Unpin**: Toggle pin status
  - **Copy as plain text**: Strip formatting

- [ ] Multi-select actions
  - Select multiple clips (Shift+click or Ctrl+click)
  - Apply action to all selected
  - Bulk delete, bulk tag, bulk export

#### Keyboard Navigation
- [ ] Arrow keys
  - ↑↓: Navigate through clip list
  - ←→: Collapse/expand clip details

- [ ] Action shortcuts
  - `Enter`: Paste selected clip
  - `Delete`/`Backspace`: Delete selected clip
  - `Ctrl+D`: Delete selected (multi-select)
  - `Ctrl+T`: Add tag to selected
  - `Ctrl+E`: Export selected

- [ ] Selection shortcuts
  - `Tab`: Cycle through filter chips
  - `Shift+Enter`: Toggle multi-select mode
  - `Ctrl+A`: Select all visible clips
  - `Escape`: Clear selection

- [ ] Navigation shortcuts
  - `Ctrl+F`: Focus search bar
  - `Ctrl+P`: Pin/unpin selected
  - `Ctrl+L`: Clear search

#### Testing
- [ ] Unit tests for context menu actions
- [ ] Keyboard navigation integration tests
- [ ] Test multi-select operations
- [ ] Accessibility testing (keyboard-only navigation)

---

## Technical Details

### Database Changes
- **No schema changes** (tags table added in v1.0.7)
- **New queries**: Tag filtering, rule matching
- **Indexes**: May need composite indexes for tag queries

### API Changes
- **New Functions**:
  - `createTag(name, color): Promise<Tag>`
  - `updateTag(id, name, color): Promise<Tag>`
  - `deleteTag(id): Promise<void>`
  - `addTagToClip(clipId, tagId): Promise<void>`
  - `removeTagFromClip(clipId, tagId): Promise<void>`
  - `searchByTag(tagName): Promise<Clip[]>`
  - `createRule(rule): Promise<Rule>`
  - `updateRule(id, rule): Promise<Rule>`
  - `deleteRule(id): Promise<void>`
  - `executeRules(clip): Promise<void>`
  - `batchExecuteRules(clips): Promise<void>`
  - `parseSearchQuery(query): Promise<SearchFilters>`
  - `getSearchHistory(): Promise<SearchQuery[]>`
  - `saveSearchToHistory(query): Promise<void>`

### New Components
- **TagManager**: CRUD interface for tags
- **TagSelector**: Dropdown for adding tags to clips
- **RuleEditor**: Form for creating/editing rules
- **RuleList**: List view of all rules
- **ContextMenu**: Right-click menu component
- **FilterChips**: Visual filter display
- **SearchHistory**: Dropdown with recent searches

### Performance Targets
- Tag operations: <50ms
- Rule execution on new clip: <100ms
- Advanced search: <200ms for 10,000 clips
- Context menu: <100ms to appear

### Dependencies
- No new external dependencies
- Reuse v1.0.7 database schema

---

## Milestones

### Week 1 (June 18-24)
**Focus**: Tag system and rules engine

| Day | Task |
|-----|------|
| Day 1-2 | Tag CRUD operations and UI |
| Day 3 | Tag filtering and auto-suggest |
| Day 4-5 | Rule creation UI and pattern matching |

**Deliverable**: Working tag system and basic rules

### Week 2 (June 25-July 1)
**Focus**: Advanced search, quick actions, polish

| Day | Task |
|-----|------|
| Day 6-7 | Advanced search syntax implementation |
| Day 8 | Search history and suggestions |
| Day 9 | Context menu and keyboard navigation |
| Day 10 | Testing, bug fixes, documentation |

**Deliverable**: Complete v1.0.8 release

---

## Testing Plan

### Unit Tests
- [ ] Tag CRUD operations
- [ ] Tag filtering logic
- [ ] Rule pattern matching (regex, literal, URL, email)
- [ ] Rule actions (auto-tag, delete, notify)
- [ ] Search query parser
- [ ] Search history storage

### Integration Tests
- [ ] Tag system with clips
- [ ] Rules engine with new clips
- [ ] Advanced search with filters
- [ ] Context menu actions

### Manual Testing
- [ ] Tag creation and management
- [ ] Rule creation and execution
- [ ] Advanced search syntax
- [ ] Keyboard navigation
- [ ] Context menu on macOS
- [ ] Context menu on Windows

### Performance Tests
- [ ] Tag operations with 1,000+ tags
- [ ] Rule execution speed
- [ ] Search performance with filters
- [ ] Memory usage with many tags

---

## Risk Assessment

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| Regex performance issues | Medium | Medium | Use optimized regex library, cache compiled patterns |
| Tag system complexity | Medium | Medium | Simple UI, limit max tags per clip |
| Search syntax confusion | Medium | Low | Clear documentation, autocomplete, examples |
| Rule conflicts | Low | Medium | Priority system, conflict detection |
| Context menu on different platforms | Low | Low | Platform-specific testing |

---

## Rollback Plan

If critical issues are discovered after release:

1. **Tag System Issues**
   - Disable tag filtering in search
   - Hide tag UI temporarily
   - Fix in v1.0.9

2. **Rules Engine Issues**
   - Disable auto-execution
   - Keep rule creation UI (for future)
   - Process rules manually

3. **Search Syntax Issues**
   - Fall back to simple text search
   - Disable advanced filters temporarily
   - Improve syntax parsing in v1.0.9

4. **Performance Issues**
   - Disable auto-suggest
   - Limit search history to 10 items
   - Optimize queries in v1.0.9

---

## Documentation

### User-Facing
- [ ] Update CHANGELOG.md
- [ ] Add "Tags" section to README
- [ ] Add "Clipboard Rules" guide
- [ ] Add "Advanced Search Syntax" reference
- [ ] Add keyboard shortcuts cheat sheet
- [ ] In-app tooltips for new features

### Developer
- [ ] Document tag system API
- [ ] Document rules engine architecture
- [ ] Document search query parser
- [ ] Update API documentation

---

## Success Criteria

- [ ] Tags work correctly with 1,000+ tags and 10,000+ clips
- [ ] Rules engine executes in <100ms per clip
- [ ] Advanced search returns results in <200ms
- [ ] Context menu works on macOS and Windows
- [ ] All keyboard shortcuts functional
- [ ] Search history persists across restarts
- [ ] No performance regression from v1.0.7
- [ ] All unit tests pass

---

## Post-Release Tasks

- [ ] Monitor tag usage patterns
- [ ] Gather feedback on rules engine
- [ ] Collect search syntax usage data
- [ ] Document issues for v1.0.9
- [ ] Plan v1.0.9 features based on feedback

---

## Notes

- **Tag Limits**: Max 10 tags per clip to prevent UI clutter. Can be increased in future versions.

- **Rule Priority**: Higher priority rules execute first. Use priority to resolve conflicts.

- **Search Syntax**: Inspired by Gmail, GitHub, and Jira search syntax for familiarity.

- **Context Menu**: Platform-specific shortcuts (Cmd on macOS, Ctrl on Windows).

- **Performance**: Focus on optimizing tag queries and rule execution as these are most user-facing.

---

*Document Created: 2026-06-03*
*Last Updated: 2026-06-03*
*Status: PLANNING*
*Version: 1.0.8*
