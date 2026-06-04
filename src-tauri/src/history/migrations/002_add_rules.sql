-- Migration 002: Add rules table for auto-tagging and pattern matching
CREATE TABLE IF NOT EXISTS rules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    pattern TEXT NOT NULL,
    pattern_type TEXT NOT NULL CHECK(pattern_type IN ('regex', 'literal', 'url', 'email')),
    action TEXT NOT NULL CHECK(action IN ('tag', 'delete', 'notify')),
    action_value TEXT,
    enabled BOOLEAN NOT NULL DEFAULT 1,
    priority INTEGER NOT NULL DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
