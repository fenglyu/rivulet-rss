CREATE TABLE IF NOT EXISTS auth_profiles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    site_url TEXT NOT NULL,
    profile_dir TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_checked_at TEXT,
    last_status TEXT
);

CREATE INDEX IF NOT EXISTS idx_auth_profiles_name ON auth_profiles(name);
