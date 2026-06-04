-- Migration 003: Add modified_at to clips table
-- created_at already exists from v1.0.6 schema
ALTER TABLE clips ADD COLUMN modified_at TEXT;

-- Backfill: set modified_at = created_at for existing rows
UPDATE clips SET modified_at = created_at WHERE modified_at IS NULL;
