use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub feed_id: i64,
    pub title: Option<String>,
    pub link: Option<String>,
    pub content: Option<String>,
    pub summary: Option<String>,
    pub author: Option<String>,
    pub published_at: Option<DateTime<Utc>>,
    pub fetched_at: DateTime<Utc>,
}

impl Item {
    pub fn new(feed_id: i64, feed_url: &str, entry_id: &str) -> Self {
        let id = Self::generate_id(feed_url, entry_id);
        Self {
            id,
            feed_id,
            title: None,
            link: None,
            content: None,
            summary: None,
            author: None,
            published_at: None,
            fetched_at: Utc::now(),
        }
    }

    /// Generate a deterministic ID from feed URL and entry ID
    pub fn generate_id(feed_url: &str, entry_id: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(feed_url.as_bytes());
        hasher.update(entry_id.as_bytes());
        hex::encode(hasher.finalize())
    }

    pub fn display_title(&self) -> &str {
        self.title.as_deref().unwrap_or("(Untitled)")
    }

    /// Get the best available content for display
    pub fn display_content(&self) -> &str {
        self.content
            .as_deref()
            .or(self.summary.as_deref())
            .unwrap_or("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_id_generation_deterministic() {
        let id1 = Item::generate_id("https://example.com/feed.xml", "entry-123");
        let id2 = Item::generate_id("https://example.com/feed.xml", "entry-123");
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_id_generation_different_inputs() {
        let id1 = Item::generate_id("https://example.com/feed.xml", "entry-123");
        let id2 = Item::generate_id("https://example.com/feed.xml", "entry-456");
        let id3 = Item::generate_id("https://other.com/feed.xml", "entry-123");
        assert_ne!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_id_is_hex_sha256() {
        let id = Item::generate_id("https://example.com/feed.xml", "entry-123");
        assert_eq!(id.len(), 64); // SHA256 produces 32 bytes = 64 hex chars
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_display_title_with_title() {
        let mut item = Item::new(1, "https://example.com/feed.xml", "e1");
        item.title = Some("My Article".into());
        assert_eq!(item.display_title(), "My Article");
    }

    #[test]
    fn test_display_title_without_title() {
        let item = Item::new(1, "https://example.com/feed.xml", "e1");
        assert_eq!(item.display_title(), "(Untitled)");
    }

    #[test]
    fn test_display_content_prefers_content() {
        let mut item = Item::new(1, "https://example.com/feed.xml", "e1");
        item.content = Some("Full content".into());
        item.summary = Some("Short summary".into());
        assert_eq!(item.display_content(), "Full content");
    }

    #[test]
    fn test_display_content_falls_back_to_summary() {
        let mut item = Item::new(1, "https://example.com/feed.xml", "e1");
        item.summary = Some("Short summary".into());
        assert_eq!(item.display_content(), "Short summary");
    }

    #[test]
    fn test_display_content_empty_when_neither() {
        let item = Item::new(1, "https://example.com/feed.xml", "e1");
        assert_eq!(item.display_content(), "");
    }
}
