-- Migration 004: Add performance indexes
CREATE INDEX IF NOT EXISTS idx_clips_type ON clips(kind);
CREATE INDEX IF NOT EXISTS idx_clips_pinned ON clips(is_pinned);
CREATE INDEX IF NOT EXISTS idx_clip_tags_clip_id ON clip_tags(clip_id);
CREATE INDEX IF NOT EXISTS idx_clip_tags_tag_id ON clip_tags(tag_id);
CREATE INDEX IF NOT EXISTS idx_rules_priority ON rules(priority DESC);
