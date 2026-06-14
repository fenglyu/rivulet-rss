CREATE TABLE IF NOT EXISTS refresh_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    started_at TEXT NOT NULL,
    completed_at TEXT,
    total_feeds INTEGER NOT NULL,
    new_item_count INTEGER NOT NULL DEFAULT 0,
    error_count INTEGER NOT NULL DEFAULT 0,
    source TEXT NOT NULL CHECK (source IN ('tui', 'cli', 'daemon', 'import'))
);

CREATE TABLE IF NOT EXISTS refresh_run_items (
    refresh_run_id INTEGER NOT NULL,
    item_id TEXT NOT NULL,
    feed_id INTEGER NOT NULL,
    inserted_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (refresh_run_id, item_id),
    FOREIGN KEY (refresh_run_id) REFERENCES refresh_runs(id) ON DELETE CASCADE,
    FOREIGN KEY (item_id) REFERENCES items(id) ON DELETE CASCADE,
    FOREIGN KEY (feed_id) REFERENCES feeds(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_refresh_runs_completed_at
    ON refresh_runs(completed_at DESC);

CREATE INDEX IF NOT EXISTS idx_refresh_run_items_run
    ON refresh_run_items(refresh_run_id);

CREATE INDEX IF NOT EXISTS idx_refresh_run_items_item
    ON refresh_run_items(item_id);

CREATE INDEX IF NOT EXISTS idx_items_fetched_at
    ON items(fetched_at DESC);
