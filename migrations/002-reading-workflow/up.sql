ALTER TABLE item_state ADD COLUMN is_queued INTEGER NOT NULL DEFAULT 0;
ALTER TABLE item_state ADD COLUMN is_saved INTEGER NOT NULL DEFAULT 0;
ALTER TABLE item_state ADD COLUMN is_archived INTEGER NOT NULL DEFAULT 0;
ALTER TABLE item_state ADD COLUMN queued_at TEXT;
ALTER TABLE item_state ADD COLUMN saved_at TEXT;
ALTER TABLE item_state ADD COLUMN archived_at TEXT;

CREATE INDEX IF NOT EXISTS idx_item_state_is_queued ON item_state(is_queued);
CREATE INDEX IF NOT EXISTS idx_item_state_is_saved ON item_state(is_saved);
CREATE INDEX IF NOT EXISTS idx_item_state_is_archived ON item_state(is_archived);
