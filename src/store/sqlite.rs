use std::path::Path;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use rusqlite_migration::{Migrations, M};

use crate::app::{Result, RivuletError};
use crate::domain::{AuthProfile, Feed, FeedUpdate, Item, ItemState};
use crate::store::{AddItemsResult, ItemListFilter, RecentItem, RefreshSource, Store};

pub struct SqliteStore {
    conn: Mutex<Connection>,
}

impl SqliteStore {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.run_migrations()?;
        Ok(store)
    }

    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.run_migrations()?;
        Ok(store)
    }

    fn run_migrations(&self) -> Result<()> {
        let migrations = Migrations::new(vec![
            M::up(include_str!("../../migrations/001-initial/up.sql")),
            M::up(include_str!("../../migrations/002-reading-workflow/up.sql")),
            M::up(include_str!("../../migrations/003-search-index/up.sql")),
            M::up(include_str!("../../migrations/004-auth-profiles/up.sql")),
            M::up(include_str!("../../migrations/005-refresh-runs/up.sql")),
        ]);

        let mut conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        conn.execute("PRAGMA foreign_keys = ON", [])?;
        migrations
            .to_latest(&mut conn)
            .map_err(|_| RivuletError::Database(rusqlite::Error::InvalidQuery))?;
        Self::sweep_orphaned_refresh_runs_locked(&conn)?;

        Ok(())
    }

    fn parse_datetime(s: &str) -> Option<DateTime<Utc>> {
        DateTime::parse_from_rfc3339(s)
            .map(|dt| dt.with_timezone(&Utc))
            .ok()
            .or_else(|| s.parse::<DateTime<Utc>>().ok())
    }

    fn row_to_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<Item> {
        Ok(Item {
            id: row.get(0)?,
            feed_id: row.get(1)?,
            title: row.get(2)?,
            link: row.get(3)?,
            content: row.get(4)?,
            summary: row.get(5)?,
            author: row.get(6)?,
            published_at: row
                .get::<_, Option<String>>(7)?
                .and_then(|s| Self::parse_datetime(&s)),
            fetched_at: row
                .get::<_, String>(8)
                .ok()
                .and_then(|s| Self::parse_datetime(&s))
                .unwrap_or_else(Utc::now),
        })
    }

    fn row_to_recent_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<RecentItem> {
        let item = Self::row_to_item(row)?;
        let feed_title = row
            .get::<_, Option<String>>(9)?
            .unwrap_or_else(|| "(Unknown feed)".to_string());
        let is_latest_refresh_item = row.get::<_, i32>(10)? != 0;
        let arrived_at = row
            .get::<_, String>(11)
            .ok()
            .and_then(|s| Self::parse_datetime(&s))
            .unwrap_or(item.fetched_at);

        Ok(RecentItem {
            item,
            feed_title,
            is_latest_refresh_item,
            arrived_at,
        })
    }

    fn sweep_orphaned_refresh_runs_locked(conn: &Connection) -> Result<()> {
        conn.execute(
            "UPDATE refresh_runs
             SET completed_at = started_at,
                 error_count = total_feeds
             WHERE completed_at IS NULL
               AND julianday(started_at) < julianday('now', '-1 hour')",
            [],
        )?;
        Ok(())
    }

    fn get_items_where(&self, where_clause: Option<&str>) -> Result<Vec<Item>> {
        let conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        let sql = match where_clause {
            Some(where_clause) => format!(
                "SELECT i.id, i.feed_id, i.title, i.link, i.content, i.summary, i.author, i.published_at, i.fetched_at
                 FROM items i
                 LEFT JOIN item_state s ON i.id = s.item_id
                 WHERE {}
                 ORDER BY i.published_at DESC, i.fetched_at DESC",
                where_clause
            ),
            None => {
                "SELECT id, feed_id, title, link, content, summary, author, published_at, fetched_at
                 FROM items ORDER BY published_at DESC, fetched_at DESC"
                    .to_string()
            }
        };

        let mut stmt = conn.prepare(&sql)?;
        let items = stmt
            .query_map([], Self::row_to_item)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(items)
    }

    fn filter_clause(filter: ItemListFilter, state_alias: &str) -> String {
        match filter {
            ItemListFilter::All => {
                format!("({state_alias}.is_archived IS NULL OR {state_alias}.is_archived = 0)")
            }
            ItemListFilter::Unread => format!(
                "({state_alias}.is_archived IS NULL OR {state_alias}.is_archived = 0) AND ({state_alias}.is_read IS NULL OR {state_alias}.is_read = 0)"
            ),
            ItemListFilter::Starred => format!(
                "{state_alias}.is_starred = 1 AND ({state_alias}.is_archived IS NULL OR {state_alias}.is_archived = 0)"
            ),
            ItemListFilter::Queued => format!(
                "{state_alias}.is_queued = 1 AND ({state_alias}.is_archived IS NULL OR {state_alias}.is_archived = 0)"
            ),
            ItemListFilter::Saved => format!(
                "{state_alias}.is_saved = 1 AND ({state_alias}.is_archived IS NULL OR {state_alias}.is_archived = 0)"
            ),
            ItemListFilter::Archived => format!("{state_alias}.is_archived = 1"),
        }
    }

    fn refresh_search_index_for_item_locked(conn: &Connection, item_id: &str) -> Result<()> {
        conn.execute(
            "DELETE FROM item_search WHERE item_id = ?1",
            params![item_id],
        )?;
        conn.execute(
            "INSERT INTO item_search (item_id, title, author, summary, content, feed_title, link)
             SELECT i.id,
                    COALESCE(i.title, ''),
                    COALESCE(i.author, ''),
                    COALESCE(i.summary, ''),
                    COALESCE(i.content, ''),
                    COALESCE(f.title, f.url, ''),
                    COALESCE(i.link, '')
             FROM items i
             JOIN feeds f ON f.id = i.feed_id
             WHERE i.id = ?1",
            params![item_id],
        )?;
        Ok(())
    }

    fn refresh_search_index_for_feed_locked(conn: &Connection, feed_id: i64) -> Result<()> {
        let mut stmt = conn.prepare("SELECT id FROM items WHERE feed_id = ?1")?;
        let ids = stmt
            .query_map(params![feed_id], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        for item_id in ids {
            Self::refresh_search_index_for_item_locked(conn, &item_id)?;
        }

        Ok(())
    }

    fn row_to_auth_profile(row: &rusqlite::Row<'_>) -> rusqlite::Result<AuthProfile> {
        Ok(AuthProfile {
            id: row.get(0)?,
            name: row.get(1)?,
            site_url: row.get(2)?,
            profile_dir: row.get(3)?,
            created_at: row
                .get::<_, String>(4)
                .ok()
                .and_then(|s| Self::parse_datetime(&s))
                .unwrap_or_else(Utc::now),
            last_checked_at: row
                .get::<_, Option<String>>(5)?
                .and_then(|s| Self::parse_datetime(&s)),
            last_status: row.get(6)?,
        })
    }
}

impl Store for SqliteStore {
    fn add_feed(&self, feed: &Feed) -> Result<i64> {
        let conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        conn.execute(
            "INSERT INTO feeds (url, title, description, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![
                feed.url,
                feed.title,
                feed.description,
                feed.created_at.to_rfc3339()
            ],
        )?;

        Ok(conn.last_insert_rowid())
    }

    fn get_feed(&self, id: i64) -> Result<Option<Feed>> {
        let conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        let result = conn
            .query_row(
                "SELECT id, url, title, description, etag, last_modified, last_fetched_at, created_at
                 FROM feeds WHERE id = ?1",
                params![id],
                |row| {
                    Ok(Feed {
                        id: row.get(0)?,
                        url: row.get(1)?,
                        title: row.get(2)?,
                        description: row.get(3)?,
                        etag: row.get(4)?,
                        last_modified: row.get(5)?,
                        last_fetched_at: row.get::<_, Option<String>>(6)?
                            .and_then(|s| Self::parse_datetime(&s)),
                        created_at: row.get::<_, String>(7)
                            .ok()
                            .and_then(|s| Self::parse_datetime(&s))
                            .unwrap_or_else(Utc::now),
                    })
                },
            )
            .optional()?;

        Ok(result)
    }

    fn get_feed_by_url(&self, url: &str) -> Result<Option<Feed>> {
        let conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        let result = conn
            .query_row(
                "SELECT id, url, title, description, etag, last_modified, last_fetched_at, created_at
                 FROM feeds WHERE url = ?1",
                params![url],
                |row| {
                    Ok(Feed {
                        id: row.get(0)?,
                        url: row.get(1)?,
                        title: row.get(2)?,
                        description: row.get(3)?,
                        etag: row.get(4)?,
                        last_modified: row.get(5)?,
                        last_fetched_at: row.get::<_, Option<String>>(6)?
                            .and_then(|s| Self::parse_datetime(&s)),
                        created_at: row.get::<_, String>(7)
                            .ok()
                            .and_then(|s| Self::parse_datetime(&s))
                            .unwrap_or_else(Utc::now),
                    })
                },
            )
            .optional()?;

        Ok(result)
    }

    fn get_all_feeds(&self) -> Result<Vec<Feed>> {
        let conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        let mut stmt = conn.prepare(
            "SELECT id, url, title, description, etag, last_modified, last_fetched_at, created_at
             FROM feeds ORDER BY title, url",
        )?;

        let feeds = stmt
            .query_map([], |row| {
                Ok(Feed {
                    id: row.get(0)?,
                    url: row.get(1)?,
                    title: row.get(2)?,
                    description: row.get(3)?,
                    etag: row.get(4)?,
                    last_modified: row.get(5)?,
                    last_fetched_at: row
                        .get::<_, Option<String>>(6)?
                        .and_then(|s| Self::parse_datetime(&s)),
                    created_at: row
                        .get::<_, String>(7)
                        .ok()
                        .and_then(|s| Self::parse_datetime(&s))
                        .unwrap_or_else(Utc::now),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(feeds)
    }

    fn update_feed(&self, id: i64, update: &FeedUpdate) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        if let Some(ref title) = update.title {
            conn.execute(
                "UPDATE feeds SET title = ?1 WHERE id = ?2",
                params![title, id],
            )?;
        }
        if let Some(ref description) = update.description {
            conn.execute(
                "UPDATE feeds SET description = ?1 WHERE id = ?2",
                params![description, id],
            )?;
        }
        if let Some(ref etag) = update.etag {
            conn.execute(
                "UPDATE feeds SET etag = ?1 WHERE id = ?2",
                params![etag, id],
            )?;
        }
        if let Some(ref last_modified) = update.last_modified {
            conn.execute(
                "UPDATE feeds SET last_modified = ?1 WHERE id = ?2",
                params![last_modified, id],
            )?;
        }
        if let Some(ref last_fetched_at) = update.last_fetched_at {
            conn.execute(
                "UPDATE feeds SET last_fetched_at = ?1 WHERE id = ?2",
                params![last_fetched_at.to_rfc3339(), id],
            )?;
        }
        Self::refresh_search_index_for_feed_locked(&conn, id)?;

        Ok(())
    }

    fn delete_feed(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        conn.execute(
            "DELETE FROM item_search
             WHERE item_id IN (SELECT id FROM items WHERE feed_id = ?1)",
            params![id],
        )?;
        conn.execute("DELETE FROM feeds WHERE id = ?1", params![id])?;
        Ok(())
    }

    fn add_auth_profile(&self, profile: &AuthProfile) -> Result<i64> {
        let conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        conn.execute(
            "INSERT INTO auth_profiles (name, site_url, profile_dir, created_at, last_checked_at, last_status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(name) DO UPDATE SET
                site_url = excluded.site_url,
                profile_dir = excluded.profile_dir",
            params![
                profile.name,
                profile.site_url,
                profile.profile_dir,
                profile.created_at.to_rfc3339(),
                profile.last_checked_at.map(|dt| dt.to_rfc3339()),
                profile.last_status
            ],
        )?;

        let id = conn.query_row(
            "SELECT id FROM auth_profiles WHERE name = ?1",
            params![profile.name],
            |row| row.get(0),
        )?;

        Ok(id)
    }

    fn get_auth_profile_by_name(&self, name: &str) -> Result<Option<AuthProfile>> {
        let conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        let profile = conn
            .query_row(
                "SELECT id, name, site_url, profile_dir, created_at, last_checked_at, last_status
                 FROM auth_profiles WHERE name = ?1",
                params![name],
                Self::row_to_auth_profile,
            )
            .optional()?;

        Ok(profile)
    }

    fn get_all_auth_profiles(&self) -> Result<Vec<AuthProfile>> {
        let conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        let mut stmt = conn.prepare(
            "SELECT id, name, site_url, profile_dir, created_at, last_checked_at, last_status
             FROM auth_profiles ORDER BY name",
        )?;

        let profiles = stmt
            .query_map([], Self::row_to_auth_profile)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(profiles)
    }

    fn update_auth_profile_status(&self, id: i64, status: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        conn.execute(
            "UPDATE auth_profiles SET last_checked_at = ?1, last_status = ?2 WHERE id = ?3",
            params![Utc::now().to_rfc3339(), status, id],
        )?;

        Ok(())
    }

    fn add_item(&self, item: &Item) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        let inserted = conn.execute(
            "INSERT OR IGNORE INTO items (id, feed_id, title, link, content, summary, author, published_at, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                item.id,
                item.feed_id,
                item.title,
                item.link,
                item.content,
                item.summary,
                item.author,
                item.published_at.map(|dt| dt.to_rfc3339()),
                item.fetched_at.to_rfc3339()
            ],
        )?;
        if inserted > 0 {
            Self::refresh_search_index_for_item_locked(&conn, &item.id)?;
        }

        Ok(())
    }

    fn add_items_with_report(&self, items: &[Item]) -> Result<AddItemsResult> {
        let mut conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        let tx = conn.transaction()?;
        let mut count = 0;
        let mut inserted_ids = Vec::new();

        for item in items {
            let inserted = tx.execute(
                "INSERT OR IGNORE INTO items (id, feed_id, title, link, content, summary, author, published_at, fetched_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    item.id,
                    item.feed_id,
                    item.title,
                    item.link,
                    item.content,
                    item.summary,
                    item.author,
                    item.published_at.map(|dt| dt.to_rfc3339()),
                    item.fetched_at.to_rfc3339()
                ],
            )?;
            count += inserted;
            if inserted > 0 {
                inserted_ids.push(item.id.clone());
            }
        }

        tx.commit()?;
        for item_id in &inserted_ids {
            Self::refresh_search_index_for_item_locked(&conn, item_id)?;
        }

        Ok(AddItemsResult {
            count,
            inserted_ids,
        })
    }

    fn add_items(&self, items: &[Item]) -> Result<usize> {
        Ok(self.add_items_with_report(items)?.count)
    }

    fn get_item(&self, id: &str) -> Result<Option<Item>> {
        let conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        let result = conn
            .query_row(
                "SELECT id, feed_id, title, link, content, summary, author, published_at, fetched_at
                 FROM items WHERE id = ?1",
                params![id],
                Self::row_to_item,
            )
            .optional()?;

        Ok(result)
    }

    fn get_items_by_feed(&self, feed_id: i64) -> Result<Vec<Item>> {
        let conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        let mut stmt = conn.prepare(
            "SELECT id, feed_id, title, link, content, summary, author, published_at, fetched_at
             FROM items WHERE feed_id = ?1 ORDER BY published_at DESC, fetched_at DESC",
        )?;

        let items = stmt
            .query_map(params![feed_id], Self::row_to_item)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(items)
    }

    fn get_all_items(&self) -> Result<Vec<Item>> {
        self.get_items_where(None)
    }

    fn get_items_by_filter(&self, filter: ItemListFilter) -> Result<Vec<Item>> {
        let where_clause = Self::filter_clause(filter, "s");
        self.get_items_where(Some(&where_clause))
    }

    fn get_recent_items(
        &self,
        filter: ItemListFilter,
        days: u32,
        limit: usize,
        latest_run_id: Option<i64>,
    ) -> Result<Vec<RecentItem>> {
        let conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        let state_clause = Self::filter_clause(filter, "s");
        let latest_run_id = latest_run_id.unwrap_or(-1);
        let days_arg = format!("-{} days", days);
        let mut recent = Vec::new();

        let batch_sql = format!(
            "SELECT i.id, i.feed_id, i.title, i.link, i.content, i.summary, i.author,
                    i.published_at, i.fetched_at, COALESCE(f.title, f.url), 1,
                    COALESCE(ri.inserted_at, i.fetched_at)
             FROM refresh_run_items ri
             JOIN items i ON i.id = ri.item_id
             JOIN feeds f ON f.id = i.feed_id
             LEFT JOIN item_state s ON i.id = s.item_id
             WHERE ri.refresh_run_id = ?1 AND {state_clause}
             ORDER BY COALESCE(i.published_at, i.fetched_at) DESC, i.fetched_at DESC"
        );
        let mut stmt = conn.prepare(&batch_sql)?;
        recent.extend(
            stmt.query_map(params![latest_run_id], Self::row_to_recent_item)?
                .collect::<std::result::Result<Vec<_>, _>>()?,
        );

        let tail_sql = format!(
            "SELECT i.id, i.feed_id, i.title, i.link, i.content, i.summary, i.author,
                    i.published_at, i.fetched_at, COALESCE(f.title, f.url), 0,
                    i.fetched_at
             FROM items i
             JOIN feeds f ON f.id = i.feed_id
             LEFT JOIN item_state s ON i.id = s.item_id
             WHERE {state_clause}
               AND julianday(i.fetched_at) >= julianday('now', ?2)
               AND NOT EXISTS (
                   SELECT 1
                   FROM refresh_run_items ri
                   WHERE ri.refresh_run_id = ?1 AND ri.item_id = i.id
               )
             ORDER BY COALESCE(i.published_at, i.fetched_at) DESC, i.fetched_at DESC
             LIMIT ?3"
        );
        let mut stmt = conn.prepare(&tail_sql)?;
        recent.extend(
            stmt.query_map(
                params![latest_run_id, days_arg, limit as i64],
                Self::row_to_recent_item,
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?,
        );

        Ok(recent)
    }

    fn search_items(&self, query: &str, filter: ItemListFilter, limit: usize) -> Result<Vec<Item>> {
        if query.trim().is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        let state_clause = Self::filter_clause(filter, "st");
        let sql = format!(
            "SELECT i.id, i.feed_id, i.title, i.link, i.content, i.summary, i.author, i.published_at, i.fetched_at
             FROM item_search s
             JOIN items i ON i.id = s.item_id
             LEFT JOIN item_state st ON i.id = st.item_id
             WHERE item_search MATCH ?1 AND {state_clause}
             ORDER BY bm25(item_search), i.published_at DESC, i.fetched_at DESC
             LIMIT ?2"
        );

        let mut stmt = conn.prepare(&sql)?;
        let items = stmt
            .query_map(params![query.trim(), limit as i64], Self::row_to_item)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(items)
    }

    fn item_exists(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM items WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;

        Ok(count > 0)
    }

    fn begin_refresh_run(&self, source: RefreshSource, total_feeds: usize) -> Result<i64> {
        let conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        conn.execute(
            "INSERT INTO refresh_runs (started_at, total_feeds, source)
             VALUES (?1, ?2, ?3)",
            params![Utc::now().to_rfc3339(), total_feeds as i64, source.as_str()],
        )?;

        Ok(conn.last_insert_rowid())
    }

    fn complete_refresh_run(
        &self,
        run_id: i64,
        new_item_count: usize,
        error_count: usize,
    ) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        conn.execute(
            "UPDATE refresh_runs
             SET completed_at = ?1,
                 new_item_count = ?2,
                 error_count = ?3
             WHERE id = ?4",
            params![
                Utc::now().to_rfc3339(),
                new_item_count as i64,
                error_count as i64,
                run_id
            ],
        )?;

        Ok(())
    }

    fn record_refresh_run_items(
        &self,
        run_id: i64,
        feed_id: i64,
        item_ids: &[String],
    ) -> Result<()> {
        let mut conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        let tx = conn.transaction()?;
        for item_id in item_ids {
            tx.execute(
                "INSERT OR IGNORE INTO refresh_run_items
                    (refresh_run_id, item_id, feed_id, inserted_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![run_id, item_id, feed_id, Utc::now().to_rfc3339()],
            )?;
        }
        tx.commit()?;

        Ok(())
    }

    fn get_latest_refresh_run_id(&self) -> Result<Option<i64>> {
        let conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        let run_id = conn
            .query_row(
                "SELECT id
                 FROM refresh_runs
                 WHERE completed_at IS NOT NULL
                 ORDER BY completed_at DESC, id DESC
                 LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;

        Ok(run_id)
    }

    fn get_item_state(&self, item_id: &str) -> Result<Option<ItemState>> {
        let conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        let result = conn
            .query_row(
                "SELECT item_id, is_read, is_starred, is_queued, is_saved, is_archived,
                        read_at, starred_at, queued_at, saved_at, archived_at
                 FROM item_state WHERE item_id = ?1",
                params![item_id],
                |row| {
                    Ok(ItemState {
                        item_id: row.get(0)?,
                        is_read: row.get::<_, i32>(1)? != 0,
                        is_starred: row.get::<_, i32>(2)? != 0,
                        is_queued: row.get::<_, i32>(3)? != 0,
                        is_saved: row.get::<_, i32>(4)? != 0,
                        is_archived: row.get::<_, i32>(5)? != 0,
                        read_at: row
                            .get::<_, Option<String>>(6)?
                            .and_then(|s| Self::parse_datetime(&s)),
                        starred_at: row
                            .get::<_, Option<String>>(7)?
                            .and_then(|s| Self::parse_datetime(&s)),
                        queued_at: row
                            .get::<_, Option<String>>(8)?
                            .and_then(|s| Self::parse_datetime(&s)),
                        saved_at: row
                            .get::<_, Option<String>>(9)?
                            .and_then(|s| Self::parse_datetime(&s)),
                        archived_at: row
                            .get::<_, Option<String>>(10)?
                            .and_then(|s| Self::parse_datetime(&s)),
                    })
                },
            )
            .optional()?;

        Ok(result)
    }

    fn set_read(&self, item_id: &str, is_read: bool) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        let read_at = if is_read {
            Some(Utc::now().to_rfc3339())
        } else {
            None
        };

        conn.execute(
            "INSERT INTO item_state (item_id, is_read, read_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(item_id) DO UPDATE SET is_read = ?2, read_at = ?3",
            params![item_id, is_read as i32, read_at],
        )?;

        Ok(())
    }

    fn set_starred(&self, item_id: &str, is_starred: bool) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        let starred_at = if is_starred {
            Some(Utc::now().to_rfc3339())
        } else {
            None
        };

        conn.execute(
            "INSERT INTO item_state (item_id, is_starred, starred_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(item_id) DO UPDATE SET is_starred = ?2, starred_at = ?3",
            params![item_id, is_starred as i32, starred_at],
        )?;

        Ok(())
    }

    fn set_queued(&self, item_id: &str, is_queued: bool) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        let queued_at = if is_queued {
            Some(Utc::now().to_rfc3339())
        } else {
            None
        };

        conn.execute(
            "INSERT INTO item_state (item_id, is_queued, queued_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(item_id) DO UPDATE SET is_queued = ?2, queued_at = ?3",
            params![item_id, is_queued as i32, queued_at],
        )?;

        Ok(())
    }

    fn set_saved(&self, item_id: &str, is_saved: bool) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        let saved_at = if is_saved {
            Some(Utc::now().to_rfc3339())
        } else {
            None
        };

        conn.execute(
            "INSERT INTO item_state (item_id, is_saved, saved_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(item_id) DO UPDATE SET is_saved = ?2, saved_at = ?3",
            params![item_id, is_saved as i32, saved_at],
        )?;

        Ok(())
    }

    fn set_archived(&self, item_id: &str, is_archived: bool) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        let archived_at = if is_archived {
            Some(Utc::now().to_rfc3339())
        } else {
            None
        };

        conn.execute(
            "INSERT INTO item_state (item_id, is_archived, archived_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(item_id) DO UPDATE SET is_archived = ?2, archived_at = ?3",
            params![item_id, is_archived as i32, archived_at],
        )?;

        Ok(())
    }

    fn get_unread_count(&self, feed_id: i64) -> Result<i64> {
        let conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM items i
             LEFT JOIN item_state s ON i.id = s.item_id
             WHERE i.feed_id = ?1
               AND (s.is_archived IS NULL OR s.is_archived = 0)
               AND (s.is_read IS NULL OR s.is_read = 0)",
            params![feed_id],
            |row| row.get(0),
        )?;

        Ok(count)
    }

    fn update_item_content(&self, id: &str, content: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| {
            RivuletError::Database(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(e.to_string()),
            ))
        })?;

        conn.execute(
            "UPDATE items SET content = ?1 WHERE id = ?2",
            params![content, id],
        )?;
        Self::refresh_search_index_for_item_locked(&conn, id)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_get_feed() {
        let store = SqliteStore::in_memory().unwrap();
        let feed = Feed::new("https://example.com/feed.xml".into());
        let id = store.add_feed(&feed).unwrap();

        let retrieved = store.get_feed(id).unwrap().unwrap();
        assert_eq!(retrieved.url, "https://example.com/feed.xml");
    }

    #[test]
    fn test_add_and_get_item() {
        let store = SqliteStore::in_memory().unwrap();
        let feed = Feed::new("https://example.com/feed.xml".into());
        let feed_id = store.add_feed(&feed).unwrap();

        let mut item = Item::new(feed_id, "https://example.com/feed.xml", "entry-1");
        item.title = Some("Test Item".into());
        store.add_item(&item).unwrap();

        let retrieved = store.get_item(&item.id).unwrap().unwrap();
        assert_eq!(retrieved.title, Some("Test Item".into()));
    }

    #[test]
    fn test_set_read_state() {
        let store = SqliteStore::in_memory().unwrap();
        let feed = Feed::new("https://example.com/feed.xml".into());
        let feed_id = store.add_feed(&feed).unwrap();

        let item = Item::new(feed_id, "https://example.com/feed.xml", "entry-1");
        store.add_item(&item).unwrap();

        store.set_read(&item.id, true).unwrap();
        let state = store.get_item_state(&item.id).unwrap().unwrap();
        assert!(state.is_read);

        store.set_read(&item.id, false).unwrap();
        let state = store.get_item_state(&item.id).unwrap().unwrap();
        assert!(!state.is_read);
    }

    #[test]
    fn test_unread_count() {
        let store = SqliteStore::in_memory().unwrap();
        let feed = Feed::new("https://example.com/feed.xml".into());
        let feed_id = store.add_feed(&feed).unwrap();

        for i in 0..5 {
            let item = Item::new(
                feed_id,
                "https://example.com/feed.xml",
                &format!("entry-{}", i),
            );
            store.add_item(&item).unwrap();
        }

        assert_eq!(store.get_unread_count(feed_id).unwrap(), 5);

        let items = store.get_items_by_feed(feed_id).unwrap();
        store.set_read(&items[0].id, true).unwrap();
        store.set_read(&items[1].id, true).unwrap();

        assert_eq!(store.get_unread_count(feed_id).unwrap(), 3);

        store.set_archived(&items[2].id, true).unwrap();
        assert_eq!(store.get_unread_count(feed_id).unwrap(), 2);
    }

    #[test]
    fn test_get_feed_by_url() {
        let store = SqliteStore::in_memory().unwrap();
        let feed = Feed::new("https://example.com/feed.xml".into());
        store.add_feed(&feed).unwrap();

        let found = store
            .get_feed_by_url("https://example.com/feed.xml")
            .unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().url, "https://example.com/feed.xml");

        let missing = store
            .get_feed_by_url("https://example.com/nonexistent.xml")
            .unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn test_get_all_feeds_ordering() {
        let store = SqliteStore::in_memory().unwrap();

        let feed_c = Feed::new("https://example.com/c.xml".into());
        let feed_a = Feed::new("https://example.com/a.xml".into());
        let feed_b = Feed::new("https://example.com/b.xml".into());

        store.add_feed(&feed_c).unwrap();
        store.add_feed(&feed_a).unwrap();
        store.add_feed(&feed_b).unwrap();

        // With no titles set, should be ordered by URL
        let feeds = store.get_all_feeds().unwrap();
        assert_eq!(feeds.len(), 3);
        assert_eq!(feeds[0].url, "https://example.com/a.xml");
        assert_eq!(feeds[1].url, "https://example.com/b.xml");
        assert_eq!(feeds[2].url, "https://example.com/c.xml");
    }

    #[test]
    fn test_update_feed() {
        let store = SqliteStore::in_memory().unwrap();
        let feed = Feed::new("https://example.com/feed.xml".into());
        let id = store.add_feed(&feed).unwrap();

        let update = FeedUpdate {
            title: Some("Updated Title".into()),
            description: Some("Updated Description".into()),
            etag: Some("\"abc123\"".into()),
            last_modified: Some("Mon, 01 Jan 2024 00:00:00 GMT".into()),
            last_fetched_at: None,
        };
        store.update_feed(id, &update).unwrap();

        let retrieved = store.get_feed(id).unwrap().unwrap();
        assert_eq!(retrieved.title, Some("Updated Title".into()));
        assert_eq!(retrieved.description, Some("Updated Description".into()));
        assert_eq!(retrieved.etag, Some("\"abc123\"".into()));
        assert_eq!(
            retrieved.last_modified,
            Some("Mon, 01 Jan 2024 00:00:00 GMT".into())
        );
    }

    #[test]
    fn test_update_feed_partial() {
        let store = SqliteStore::in_memory().unwrap();
        let feed = Feed::new("https://example.com/feed.xml".into());
        let id = store.add_feed(&feed).unwrap();

        // Only update title, leave everything else as None
        let update = FeedUpdate {
            title: Some("New Title".into()),
            ..Default::default()
        };
        store.update_feed(id, &update).unwrap();

        let retrieved = store.get_feed(id).unwrap().unwrap();
        assert_eq!(retrieved.title, Some("New Title".into()));
        assert_eq!(retrieved.description, None);
        assert_eq!(retrieved.etag, None);
    }

    #[test]
    fn test_delete_feed_cascades_items() {
        let store = SqliteStore::in_memory().unwrap();
        let feed = Feed::new("https://example.com/feed.xml".into());
        let feed_id = store.add_feed(&feed).unwrap();

        let item = Item::new(feed_id, "https://example.com/feed.xml", "entry-1");
        let item_id = item.id.clone();
        store.add_item(&item).unwrap();

        assert!(store.item_exists(&item_id).unwrap());

        store.delete_feed(feed_id).unwrap();

        assert!(store.get_feed(feed_id).unwrap().is_none());
        assert!(!store.item_exists(&item_id).unwrap());
    }

    #[test]
    fn test_add_items_batch_and_dedup() {
        let store = SqliteStore::in_memory().unwrap();
        let feed = Feed::new("https://example.com/feed.xml".into());
        let feed_id = store.add_feed(&feed).unwrap();

        let items: Vec<Item> = (0..3)
            .map(|i| {
                Item::new(
                    feed_id,
                    "https://example.com/feed.xml",
                    &format!("entry-{}", i),
                )
            })
            .collect();

        let count = store.add_items(&items).unwrap();
        assert_eq!(count, 3);

        // Duplicate batch: INSERT OR IGNORE means 0 new rows
        let count = store.add_items(&items).unwrap();
        assert_eq!(count, 0);

        let stored = store.get_items_by_feed(feed_id).unwrap();
        assert_eq!(stored.len(), 3);
    }

    #[test]
    fn test_add_items_with_report_returns_only_inserted_ids() {
        let store = SqliteStore::in_memory().unwrap();
        let feed = Feed::new("https://example.com/feed.xml".into());
        let feed_id = store.add_feed(&feed).unwrap();

        let items: Vec<Item> = (0..2)
            .map(|i| {
                Item::new(
                    feed_id,
                    "https://example.com/feed.xml",
                    &format!("entry-{}", i),
                )
            })
            .collect();

        let first = store.add_items_with_report(&items).unwrap();
        assert_eq!(first.count, 2);
        assert_eq!(
            first.inserted_ids,
            vec![items[0].id.clone(), items[1].id.clone()]
        );

        let second = store.add_items_with_report(&items).unwrap();
        assert_eq!(second.count, 0);
        assert!(second.inserted_ids.is_empty());
    }

    #[test]
    fn test_refresh_run_round_trip_and_recent_items_ordering() {
        let store = SqliteStore::in_memory().unwrap();
        let feed = Feed::new("https://example.com/feed.xml".into());
        let feed_id = store.add_feed(&feed).unwrap();

        let mut latest_a = Item::new(feed_id, "https://example.com/feed.xml", "latest-a");
        latest_a.title = Some("Latest A".into());
        latest_a.published_at = Some(Utc::now() - chrono::Duration::days(20));
        let mut latest_b = Item::new(feed_id, "https://example.com/feed.xml", "latest-b");
        latest_b.title = Some("Latest B".into());
        latest_b.published_at = Some(Utc::now() - chrono::Duration::days(19));
        let mut tail = Item::new(feed_id, "https://example.com/feed.xml", "tail");
        tail.title = Some("Tail".into());
        tail.published_at = Some(Utc::now());

        let report = store
            .add_items_with_report(&[latest_a.clone(), latest_b.clone(), tail.clone()])
            .unwrap();
        assert_eq!(report.count, 3);

        let run_id = store.begin_refresh_run(RefreshSource::Tui, 1).unwrap();
        store
            .record_refresh_run_items(run_id, feed_id, &[latest_a.id.clone(), latest_b.id.clone()])
            .unwrap();
        store.complete_refresh_run(run_id, 2, 0).unwrap();

        assert_eq!(store.get_latest_refresh_run_id().unwrap(), Some(run_id));

        let recent = store
            .get_recent_items(ItemListFilter::All, 7, 1, Some(run_id))
            .unwrap();
        assert_eq!(recent.len(), 3);
        assert!(recent[0].is_latest_refresh_item);
        assert!(recent[1].is_latest_refresh_item);
        assert!(!recent[2].is_latest_refresh_item);
        assert_eq!(recent[2].item.id, tail.id);
    }

    #[test]
    fn test_recent_items_respects_filters() {
        let store = SqliteStore::in_memory().unwrap();
        let feed = Feed::new("https://example.com/feed.xml".into());
        let feed_id = store.add_feed(&feed).unwrap();

        let starred = Item::new(feed_id, "https://example.com/feed.xml", "starred");
        let plain = Item::new(feed_id, "https://example.com/feed.xml", "plain");
        store
            .add_items_with_report(&[starred.clone(), plain.clone()])
            .unwrap();
        store.set_starred(&starred.id, true).unwrap();

        let run_id = store.begin_refresh_run(RefreshSource::Tui, 1).unwrap();
        store
            .record_refresh_run_items(run_id, feed_id, &[starred.id.clone(), plain.id.clone()])
            .unwrap();
        store.complete_refresh_run(run_id, 2, 0).unwrap();

        let recent = store
            .get_recent_items(ItemListFilter::Starred, 7, 10, Some(run_id))
            .unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].item.id, starred.id);
        assert!(recent[0].is_latest_refresh_item);
    }

    #[test]
    fn test_sweep_orphaned_refresh_runs_marks_stale_runs_failed() {
        let store = SqliteStore::in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO refresh_runs (started_at, total_feeds, source)
             VALUES (?1, ?2, ?3)",
            params![
                (Utc::now() - chrono::Duration::hours(2)).to_rfc3339(),
                3_i64,
                RefreshSource::Cli.as_str()
            ],
        )
        .unwrap();
        let run_id = conn.last_insert_rowid();
        SqliteStore::sweep_orphaned_refresh_runs_locked(&conn).unwrap();

        let (completed_at, error_count): (Option<String>, i64) = conn
            .query_row(
                "SELECT completed_at, error_count FROM refresh_runs WHERE id = ?1",
                params![run_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(completed_at.is_some());
        assert_eq!(error_count, 3);
    }

    #[test]
    fn test_add_duplicate_item_ignored() {
        let store = SqliteStore::in_memory().unwrap();
        let feed = Feed::new("https://example.com/feed.xml".into());
        let feed_id = store.add_feed(&feed).unwrap();

        let mut item = Item::new(feed_id, "https://example.com/feed.xml", "entry-1");
        item.title = Some("Original Title".into());
        store.add_item(&item).unwrap();

        // Same ID with different title — should be ignored
        let mut dup = Item::new(feed_id, "https://example.com/feed.xml", "entry-1");
        dup.title = Some("Different Title".into());
        store.add_item(&dup).unwrap();

        let retrieved = store.get_item(&item.id).unwrap().unwrap();
        assert_eq!(retrieved.title, Some("Original Title".into()));
    }

    #[test]
    fn test_item_exists() {
        let store = SqliteStore::in_memory().unwrap();
        let feed = Feed::new("https://example.com/feed.xml".into());
        let feed_id = store.add_feed(&feed).unwrap();

        let item = Item::new(feed_id, "https://example.com/feed.xml", "entry-1");
        let item_id = item.id.clone();

        assert!(!store.item_exists(&item_id).unwrap());
        store.add_item(&item).unwrap();
        assert!(store.item_exists(&item_id).unwrap());
    }

    #[test]
    fn test_set_starred() {
        let store = SqliteStore::in_memory().unwrap();
        let feed = Feed::new("https://example.com/feed.xml".into());
        let feed_id = store.add_feed(&feed).unwrap();

        let item = Item::new(feed_id, "https://example.com/feed.xml", "entry-1");
        store.add_item(&item).unwrap();

        store.set_starred(&item.id, true).unwrap();
        let state = store.get_item_state(&item.id).unwrap().unwrap();
        assert!(state.is_starred);
        assert!(state.starred_at.is_some());

        store.set_starred(&item.id, false).unwrap();
        let state = store.get_item_state(&item.id).unwrap().unwrap();
        assert!(!state.is_starred);
        assert!(state.starred_at.is_none());
    }

    #[test]
    fn test_set_reading_workflow_states() {
        let store = SqliteStore::in_memory().unwrap();
        let feed = Feed::new("https://example.com/feed.xml".into());
        let feed_id = store.add_feed(&feed).unwrap();

        let item = Item::new(feed_id, "https://example.com/feed.xml", "entry-1");
        store.add_item(&item).unwrap();

        store.set_queued(&item.id, true).unwrap();
        store.set_saved(&item.id, true).unwrap();
        store.set_archived(&item.id, true).unwrap();

        let state = store.get_item_state(&item.id).unwrap().unwrap();
        assert!(state.is_queued);
        assert!(state.is_saved);
        assert!(state.is_archived);
        assert!(state.queued_at.is_some());
        assert!(state.saved_at.is_some());
        assert!(state.archived_at.is_some());

        store.set_queued(&item.id, false).unwrap();
        store.set_saved(&item.id, false).unwrap();
        store.set_archived(&item.id, false).unwrap();

        let state = store.get_item_state(&item.id).unwrap().unwrap();
        assert!(!state.is_queued);
        assert!(!state.is_saved);
        assert!(!state.is_archived);
        assert!(state.queued_at.is_none());
        assert!(state.saved_at.is_none());
        assert!(state.archived_at.is_none());
    }

    #[test]
    fn test_get_items_by_filter() {
        let store = SqliteStore::in_memory().unwrap();
        let feed = Feed::new("https://example.com/feed.xml".into());
        let feed_id = store.add_feed(&feed).unwrap();

        let unread = Item::new(feed_id, "https://example.com/feed.xml", "unread");
        let read = Item::new(feed_id, "https://example.com/feed.xml", "read");
        let starred = Item::new(feed_id, "https://example.com/feed.xml", "starred");
        let queued = Item::new(feed_id, "https://example.com/feed.xml", "queued");
        let saved = Item::new(feed_id, "https://example.com/feed.xml", "saved");
        let archived = Item::new(feed_id, "https://example.com/feed.xml", "archived");

        store
            .add_items(&[
                unread.clone(),
                read.clone(),
                starred.clone(),
                queued.clone(),
                saved.clone(),
                archived.clone(),
            ])
            .unwrap();

        store.set_read(&read.id, true).unwrap();
        store.set_starred(&starred.id, true).unwrap();
        store.set_queued(&queued.id, true).unwrap();
        store.set_saved(&saved.id, true).unwrap();
        store.set_archived(&archived.id, true).unwrap();

        let unread_ids: Vec<String> = store
            .get_items_by_filter(ItemListFilter::Unread)
            .unwrap()
            .into_iter()
            .map(|i| i.id)
            .collect();
        assert!(unread_ids.contains(&unread.id));
        assert!(!unread_ids.contains(&read.id));
        assert!(!unread_ids.contains(&archived.id));

        let all_ids: Vec<String> = store
            .get_items_by_filter(ItemListFilter::All)
            .unwrap()
            .into_iter()
            .map(|i| i.id)
            .collect();
        assert!(all_ids.contains(&unread.id));
        assert!(!all_ids.contains(&archived.id));

        let starred_ids: Vec<String> = store
            .get_items_by_filter(ItemListFilter::Starred)
            .unwrap()
            .into_iter()
            .map(|i| i.id)
            .collect();
        assert_eq!(starred_ids, vec![starred.id.clone()]);

        let queued_ids: Vec<String> = store
            .get_items_by_filter(ItemListFilter::Queued)
            .unwrap()
            .into_iter()
            .map(|i| i.id)
            .collect();
        assert_eq!(queued_ids, vec![queued.id.clone()]);

        let saved_ids: Vec<String> = store
            .get_items_by_filter(ItemListFilter::Saved)
            .unwrap()
            .into_iter()
            .map(|i| i.id)
            .collect();
        assert_eq!(saved_ids, vec![saved.id.clone()]);

        let archived_ids: Vec<String> = store
            .get_items_by_filter(ItemListFilter::Archived)
            .unwrap()
            .into_iter()
            .map(|i| i.id)
            .collect();
        assert_eq!(archived_ids, vec![archived.id.clone()]);
    }

    #[test]
    fn test_search_items_indexes_inserted_and_scraped_content() {
        let store = SqliteStore::in_memory().unwrap();
        let feed = Feed::new("https://example.com/feed.xml".into());
        let feed_id = store.add_feed(&feed).unwrap();

        let mut item = Item::new(feed_id, "https://example.com/feed.xml", "entry-1");
        item.title = Some("Rust release notes".into());
        item.summary = Some("Compiler improvements".into());
        store.add_item(&item).unwrap();

        let title_results = store
            .search_items("release", ItemListFilter::All, 10)
            .unwrap();
        assert_eq!(title_results.len(), 1);
        assert_eq!(title_results[0].id, item.id);

        store
            .update_item_content(&item.id, "Ownership and borrow checker deep dive")
            .unwrap();

        let content_results = store
            .search_items("ownership", ItemListFilter::All, 10)
            .unwrap();
        assert_eq!(content_results.len(), 1);
        assert_eq!(content_results[0].id, item.id);
    }

    #[test]
    fn test_search_items_respects_filters() {
        let store = SqliteStore::in_memory().unwrap();
        let feed = Feed::new("https://example.com/feed.xml".into());
        let feed_id = store.add_feed(&feed).unwrap();

        let mut active = Item::new(feed_id, "https://example.com/feed.xml", "active");
        active.title = Some("Searchable active item".into());
        let mut archived = Item::new(feed_id, "https://example.com/feed.xml", "archived");
        archived.title = Some("Searchable archived item".into());
        store
            .add_items(&[active.clone(), archived.clone()])
            .unwrap();
        store.set_archived(&archived.id, true).unwrap();

        let active_results = store
            .search_items("searchable", ItemListFilter::All, 10)
            .unwrap();
        assert_eq!(active_results.len(), 1);
        assert_eq!(active_results[0].id, active.id);

        let archived_results = store
            .search_items("searchable", ItemListFilter::Archived, 10)
            .unwrap();
        assert_eq!(archived_results.len(), 1);
        assert_eq!(archived_results[0].id, archived.id);
    }

    #[test]
    fn test_update_item_content() {
        let store = SqliteStore::in_memory().unwrap();
        let feed = Feed::new("https://example.com/feed.xml".into());
        let feed_id = store.add_feed(&feed).unwrap();

        let item = Item::new(feed_id, "https://example.com/feed.xml", "entry-1");
        let item_id = item.id.clone();
        store.add_item(&item).unwrap();

        let retrieved = store.get_item(&item_id).unwrap().unwrap();
        assert_eq!(retrieved.content, None);

        store
            .update_item_content(&item_id, "<p>Full article content</p>")
            .unwrap();

        let retrieved = store.get_item(&item_id).unwrap().unwrap();
        assert_eq!(
            retrieved.content,
            Some("<p>Full article content</p>".into())
        );
    }

    #[test]
    fn test_get_all_items_across_feeds() {
        let store = SqliteStore::in_memory().unwrap();

        let feed1 = Feed::new("https://example.com/feed1.xml".into());
        let feed1_id = store.add_feed(&feed1).unwrap();
        let feed2 = Feed::new("https://example.com/feed2.xml".into());
        let feed2_id = store.add_feed(&feed2).unwrap();

        store
            .add_item(&Item::new(feed1_id, "https://example.com/feed1.xml", "e1"))
            .unwrap();
        store
            .add_item(&Item::new(feed2_id, "https://example.com/feed2.xml", "e2"))
            .unwrap();

        let all = store.get_all_items().unwrap();
        assert_eq!(all.len(), 2);

        let feed_ids: Vec<i64> = all.iter().map(|i| i.feed_id).collect();
        assert!(feed_ids.contains(&feed1_id));
        assert!(feed_ids.contains(&feed2_id));
    }

    #[test]
    fn test_get_feed_nonexistent() {
        let store = SqliteStore::in_memory().unwrap();
        assert!(store.get_feed(999).unwrap().is_none());
    }

    #[test]
    fn test_auth_profile_crud_and_status() {
        let store = SqliteStore::in_memory().unwrap();
        let profile = AuthProfile::new(
            "nyt".into(),
            "https://www.nytimes.com".into(),
            "/tmp/rivulet-auth/nyt".into(),
        );

        let id = store.add_auth_profile(&profile).unwrap();
        let stored = store.get_auth_profile_by_name("nyt").unwrap().unwrap();
        assert_eq!(stored.id, id);
        assert_eq!(stored.site_url, "https://www.nytimes.com");
        assert_eq!(stored.profile_dir, "/tmp/rivulet-auth/nyt");
        assert!(stored.last_status.is_none());

        store.update_auth_profile_status(id, "ok").unwrap();
        let stored = store.get_auth_profile_by_name("nyt").unwrap().unwrap();
        assert_eq!(stored.last_status, Some("ok".into()));
        assert!(stored.last_checked_at.is_some());

        let profiles = store.get_all_auth_profiles().unwrap();
        assert_eq!(profiles.len(), 1);
    }

    #[test]
    fn test_get_item_nonexistent() {
        let store = SqliteStore::in_memory().unwrap();
        assert!(store.get_item("nonexistent-id").unwrap().is_none());
    }

    #[test]
    fn test_get_item_state_nonexistent() {
        let store = SqliteStore::in_memory().unwrap();
        assert!(store.get_item_state("nonexistent-id").unwrap().is_none());
    }
}
