use chrono::Utc;
use feed_rs::parser;
use html_escape::decode_html_entities;

use crate::app::{Result, RivuletError};
use crate::domain::Item;

#[derive(Debug, Clone)]
pub struct FeedMeta {
    pub title: Option<String>,
    pub description: Option<String>,
}

#[derive(Clone)]
pub struct Normalizer;

impl Default for Normalizer {
    fn default() -> Self {
        Self::new()
    }
}

impl Normalizer {
    pub fn new() -> Self {
        Self
    }

    pub fn normalize(
        &self,
        feed_id: i64,
        feed_url: &str,
        body: &[u8],
    ) -> Result<(FeedMeta, Vec<Item>)> {
        let feed = parser::parse(body).map_err(|e| RivuletError::FeedParse(e.to_string()))?;

        let meta = FeedMeta {
            title: feed
                .title
                .map(|t| decode_html_entities(&t.content).to_string()),
            description: feed
                .description
                .map(|d| decode_html_entities(&d.content).to_string()),
        };

        let items: Vec<Item> = feed
            .entries
            .into_iter()
            .map(|entry| {
                let entry_id = entry.id.clone();
                let link = entry.links.first().map(|l| l.href.clone());
                let entry_id_for_hash = if entry_id.is_empty() {
                    link.clone().unwrap_or_default()
                } else {
                    entry_id
                };

                let mut item = Item::new(feed_id, feed_url, &entry_id_for_hash);

                item.title = entry
                    .title
                    .map(|t| decode_html_entities(&t.content).to_string());
                item.link = link;
                item.content = entry
                    .content
                    .and_then(|c| c.body)
                    .map(|b| decode_html_entities(&b).to_string());
                item.summary = entry
                    .summary
                    .map(|s| decode_html_entities(&s.content).to_string());
                item.author = entry.authors.first().map(|a| a.name.clone());
                item.published_at = entry
                    .published
                    .or(entry.updated)
                    .map(|dt| dt.with_timezone(&Utc));

                item
            })
            .collect();

        Ok((meta, items))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS_SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Test Feed</title>
    <description>A test feed</description>
    <item>
      <title>Test Item 1</title>
      <link>https://example.com/item1</link>
      <guid>item-1</guid>
      <pubDate>Mon, 01 Jan 2024 00:00:00 GMT</pubDate>
      <description>This is item 1</description>
    </item>
    <item>
      <title>Test Item 2</title>
      <link>https://example.com/item2</link>
      <guid>item-2</guid>
      <description>This is item 2</description>
    </item>
  </channel>
</rss>"#;

    const ATOM_SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Atom Test Feed</title>
  <subtitle>An Atom test feed</subtitle>
  <entry>
    <title>Atom Entry 1</title>
    <link href="https://example.com/atom1"/>
    <id>atom-entry-1</id>
    <updated>2024-01-01T00:00:00Z</updated>
    <summary>This is Atom entry 1</summary>
  </entry>
</feed>"#;

    #[test]
    fn test_parse_rss() {
        let normalizer = Normalizer::new();
        let (meta, items) = normalizer
            .normalize(1, "https://example.com/feed.xml", RSS_SAMPLE.as_bytes())
            .unwrap();

        assert_eq!(meta.title, Some("Test Feed".into()));
        assert_eq!(meta.description, Some("A test feed".into()));
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, Some("Test Item 1".into()));
        assert_eq!(items[0].link, Some("https://example.com/item1".into()));
    }

    #[test]
    fn test_parse_atom() {
        let normalizer = Normalizer::new();
        let (meta, items) = normalizer
            .normalize(1, "https://example.com/feed.atom", ATOM_SAMPLE.as_bytes())
            .unwrap();

        assert_eq!(meta.title, Some("Atom Test Feed".into()));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, Some("Atom Entry 1".into()));
        assert_eq!(items[0].link, Some("https://example.com/atom1".into()));
    }

    #[test]
    fn test_item_id_determinism() {
        let normalizer = Normalizer::new();
        let (_, items1) = normalizer
            .normalize(1, "https://example.com/feed.xml", RSS_SAMPLE.as_bytes())
            .unwrap();
        let (_, items2) = normalizer
            .normalize(1, "https://example.com/feed.xml", RSS_SAMPLE.as_bytes())
            .unwrap();

        assert_eq!(items1[0].id, items2[0].id);
        assert_eq!(items1[1].id, items2[1].id);
    }

    #[test]
    fn test_parse_invalid_feed() {
        let normalizer = Normalizer::new();
        let result = normalizer.normalize(1, "https://example.com/feed.xml", b"not a feed");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_rss_missing_fields() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <item>
      <guid>item-no-title</guid>
    </item>
  </channel>
</rss>"#;
        let normalizer = Normalizer::new();
        let (meta, items) = normalizer
            .normalize(1, "https://example.com/feed.xml", xml.as_bytes())
            .unwrap();

        assert_eq!(meta.title, None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, None);
        assert_eq!(items[0].link, None);
    }

    #[test]
    fn test_parse_rss_html_entities_in_title() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Tom &amp; Jerry</title>
    <item>
      <title>Episode &lt;1&gt;</title>
      <guid>ep-1</guid>
    </item>
  </channel>
</rss>"#;
        let normalizer = Normalizer::new();
        let (meta, items) = normalizer
            .normalize(1, "https://example.com/feed.xml", xml.as_bytes())
            .unwrap();

        assert_eq!(meta.title, Some("Tom & Jerry".into()));
        assert_eq!(items[0].title, Some("Episode <1>".into()));
    }

    #[test]
    fn test_parse_entry_empty_id_uses_link() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <item>
      <title>No GUID Item</title>
      <link>https://example.com/article</link>
    </item>
  </channel>
</rss>"#;
        let normalizer = Normalizer::new();
        let (_, items) = normalizer
            .normalize(1, "https://example.com/feed.xml", xml.as_bytes())
            .unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id.len(), 64);
        assert!(items[0].id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_normalize_preserves_author() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Author Test</title>
  <entry>
    <title>Post by Alice</title>
    <id>post-1</id>
    <author><name>Alice</name></author>
    <updated>2024-01-01T00:00:00Z</updated>
  </entry>
</feed>"#;
        let normalizer = Normalizer::new();
        let (_, items) = normalizer
            .normalize(1, "https://example.com/feed.atom", xml.as_bytes())
            .unwrap();

        assert_eq!(items[0].author, Some("Alice".into()));
    }

    #[test]
    fn test_normalize_preserves_summary() {
        let (_, items) = Normalizer::new()
            .normalize(1, "https://example.com/feed.xml", RSS_SAMPLE.as_bytes())
            .unwrap();

        assert_eq!(items[0].summary, Some("This is item 1".into()));
        assert_eq!(items[1].summary, Some("This is item 2".into()));
    }
}
