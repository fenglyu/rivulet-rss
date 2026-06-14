use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthProfile {
    pub id: i64,
    pub name: String,
    pub site_url: String,
    pub profile_dir: String,
    pub created_at: DateTime<Utc>,
    pub last_checked_at: Option<DateTime<Utc>>,
    pub last_status: Option<String>,
}

impl AuthProfile {
    pub fn new(name: String, site_url: String, profile_dir: String) -> Self {
        Self {
            id: 0,
            name,
            site_url,
            profile_dir,
            created_at: Utc::now(),
            last_checked_at: None,
            last_status: None,
        }
    }
}
