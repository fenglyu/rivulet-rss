use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ItemState {
    pub item_id: String,
    pub is_read: bool,
    pub is_starred: bool,
    pub is_queued: bool,
    pub is_saved: bool,
    pub is_archived: bool,
    pub read_at: Option<DateTime<Utc>>,
    pub starred_at: Option<DateTime<Utc>>,
    pub queued_at: Option<DateTime<Utc>>,
    pub saved_at: Option<DateTime<Utc>>,
    pub archived_at: Option<DateTime<Utc>>,
}

impl ItemState {
    pub fn new(item_id: String) -> Self {
        Self {
            item_id,
            is_read: false,
            is_starred: false,
            is_queued: false,
            is_saved: false,
            is_archived: false,
            read_at: None,
            starred_at: None,
            queued_at: None,
            saved_at: None,
            archived_at: None,
        }
    }
}
