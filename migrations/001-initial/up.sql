-- Feeds table
CREATE TABLE IF NOT EXISTS feeds (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    url TEXT NOT NULL UNIQUE,
    title TEXT,
    description TEXT,
    etag TEXT,
    last_modified TEXT,
    last_fetched_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Items table
CREATE TABLE IF NOT EXISTS items (
    id TEXT PRIMARY KEY,
    feed_id INTEGER NOT NULL,
    title TEXT,
    link TEXT,
    content TEXT,
    summary TEXT,
    author TEXT,
    published_at TEXT,
    fetched_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (feed_id) REFERENCES feeds(id) ON DELETE CASCADE
);

-- Item state table (read/starred status)
CREATE TABLE IF NOT EXISTS item_state (
    item_id TEXT PRIMARY KEY,
    is_read INTEGER NOT NULL DEFAULT 0,
    is_starred INTEGER NOT NULL DEFAULT 0,
    read_at TEXT,
    starred_at TEXT,
    FOREIGN KEY (item_id) REFERENCES items(id) ON DELETE CASCADE
);

-- Indexes for common queries
CREATE INDEX IF NOT EXISTS idx_items_feed_id ON items(feed_id);
CREATE INDEX IF NOT EXISTS idx_items_published_at ON items(published_at DESC);
CREATE INDEX IF NOT EXISTS idx_item_state_is_read ON item_state(is_read);
CREATE INDEX IF NOT EXISTS idx_item_state_is_starred ON item_state(is_starred);
