use std::path::Path;
use std::path::PathBuf;

use chrono::Utc;

use crate::app::{AppContext, Result, RivuletError};
use crate::domain::{AuthProfile, Feed, FeedUpdate};
use crate::fetcher::FetchResult;
use crate::scraper::{ChromeScraper, Scraper, ScraperConfig};
use crate::store::{ItemListFilter, RefreshSource, Store};
use crate::url_policy::parse_http_url;

/// Initialize config file with all options
pub fn init_config(force: bool) -> Result<()> {
    let config_path = crate::config::Config::default_config_path()
        .map_err(|e| RivuletError::Config(e.to_string()))?;

    if config_path.exists() && !force {
        println!("Config already exists: {}", config_path.display());
        println!("Use --force to overwrite");
        return Ok(());
    }

    // Ensure parent directory exists
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Write the full sample config
    let sample_config = include_str!("../../config.sample.toml");
    std::fs::write(&config_path, sample_config)?;

    println!("Created config: {}", config_path.display());
    println!("\nEdit to customize colors, keybindings, and scraper settings.");

    Ok(())
}

pub async fn add_feed(ctx: &AppContext, url: &str) -> Result<()> {
    parse_http_url(url)?;

    // Check if feed already exists
    if ctx.store.get_feed_by_url(url)?.is_some() {
        println!("Feed already exists: {}", url);
        return Ok(());
    }

    // Create the feed entry
    let feed = Feed::new(url.to_string());
    let feed_id = ctx.store.add_feed(&feed)?;
    println!("Added feed: {}", url);

    // Fetch and store items
    let run_id = ctx.store.begin_refresh_run(RefreshSource::Cli, 1)?;
    let result = ctx.fetcher.fetch(url, None, None).await?;
    let mut total_new = 0;

    match result {
        FetchResult::Content {
            body,
            etag,
            last_modified,
        } => {
            let (meta, items) = ctx.normalizer.normalize(feed_id, url, &body)?;

            // Update feed with metadata
            let update = FeedUpdate {
                title: meta.title.clone(),
                description: meta.description,
                etag,
                last_modified,
                last_fetched_at: Some(Utc::now()),
            };
            ctx.store.update_feed(feed_id, &update)?;

            // Add items
            let add_result = ctx.store.add_items_with_report(&items)?;
            total_new = add_result.count;
            ctx.store
                .record_refresh_run_items(run_id, feed_id, &add_result.inserted_ids)?;

            // Queue new items for background scraping
            ctx.queue_for_scraping(items).await;

            if let Some(title) = meta.title {
                println!("Feed title: {}", title);
            }
            println!("Fetched {} items", add_result.count);
        }
        FetchResult::NotModified => {
            println!("Feed not modified");
        }
    }
    ctx.store.complete_refresh_run(run_id, total_new, 0)?;

    Ok(())
}

pub async fn remove_feed(ctx: &AppContext, url: &str) -> Result<()> {
    parse_http_url(url)?;

    let feed = ctx
        .store
        .get_feed_by_url(url)?
        .ok_or_else(|| RivuletError::FeedNotFound(url.to_string()))?;

    ctx.store.delete_feed(feed.id)?;
    println!("Removed feed: {}", url);
    Ok(())
}

pub async fn update_feeds(ctx: &AppContext) -> Result<()> {
    let feeds = ctx.store.get_all_feeds()?;

    if feeds.is_empty() {
        println!("No feeds to update");
        return Ok(());
    }

    println!("Updating {} feeds...", feeds.len());
    let run_id = ctx
        .store
        .begin_refresh_run(RefreshSource::Cli, feeds.len())?;

    let results = ctx
        .parallel_fetcher
        .fetch_all(feeds.clone(), ctx.store.clone(), &ctx.normalizer, None)
        .await;

    let mut total_new = 0;
    let mut errors = 0;
    let mut updated_feed_ids = Vec::new();

    for (feed_id, result) in results {
        match result {
            Ok(refresh) => {
                total_new += refresh.new_count;
                ctx.store
                    .record_refresh_run_items(run_id, feed_id, &refresh.inserted_item_ids)?;
                if refresh.new_count > 0 {
                    updated_feed_ids.push(feed_id);
                    if let Ok(Some(feed)) = ctx.store.get_feed(feed_id) {
                        println!(
                            "  {} new items from {}",
                            refresh.new_count,
                            feed.display_title()
                        );
                    }
                }
            }
            Err(e) => {
                errors += 1;
                if let Ok(Some(feed)) = ctx.store.get_feed(feed_id) {
                    eprintln!("  Error updating {}: {}", feed.display_title(), e);
                }
            }
        }
    }
    ctx.store.complete_refresh_run(run_id, total_new, errors)?;

    // Queue items from updated feeds for background scraping
    // Only queue items that actually need scraping (no content and no summary)
    if ctx.scraper_handle.is_some() && !updated_feed_ids.is_empty() {
        let mut items_to_scrape = Vec::new();
        for feed_id in updated_feed_ids {
            if let Ok(items) = ctx.store.get_items_by_feed(feed_id) {
                items_to_scrape.extend(items.into_iter().filter(ChromeScraper::needs_scraping));
            }
        }
        if !items_to_scrape.is_empty() {
            println!(
                "Queuing {} items for background content scraping...",
                items_to_scrape.len()
            );
            ctx.queue_for_scraping(items_to_scrape).await;
        }
    }

    println!(
        "Update complete: {} new items, {} errors",
        total_new, errors
    );
    Ok(())
}

pub fn list_feeds(ctx: &AppContext) -> Result<()> {
    let feeds = ctx.store.get_all_feeds()?;

    if feeds.is_empty() {
        println!("No feeds");
        return Ok(());
    }

    for feed in feeds {
        let unread = ctx.store.get_unread_count(feed.id)?;
        println!(
            "{} ({} unread)\n  {}",
            feed.display_title(),
            unread,
            feed.url
        );
    }

    Ok(())
}

pub fn list_filter_from_flags(
    unread: bool,
    starred: bool,
    queued: bool,
    saved: bool,
    archived: bool,
) -> Result<Option<ItemListFilter>> {
    let filters = [
        (unread, ItemListFilter::Unread),
        (starred, ItemListFilter::Starred),
        (queued, ItemListFilter::Queued),
        (saved, ItemListFilter::Saved),
        (archived, ItemListFilter::Archived),
    ];

    let selected: Vec<ItemListFilter> = filters
        .into_iter()
        .filter_map(|(enabled, filter)| enabled.then_some(filter))
        .collect();

    match selected.as_slice() {
        [] => Ok(None),
        [filter] => Ok(Some(*filter)),
        _ => Err(RivuletError::Config(
            "Use only one item filter at a time".to_string(),
        )),
    }
}

pub fn list_items(ctx: &AppContext, filter: Option<ItemListFilter>) -> Result<()> {
    let items = ctx
        .store
        .get_items_by_filter(filter.unwrap_or(ItemListFilter::All))?;

    if items.is_empty() {
        println!("No items");
        return Ok(());
    }

    for item in items {
        print_item_line(ctx, &item)?;
    }

    Ok(())
}

pub fn search_items(
    ctx: &AppContext,
    query: &str,
    filter: ItemListFilter,
    limit: usize,
) -> Result<()> {
    let items = ctx.store.search_items(query, filter, limit)?;

    if items.is_empty() {
        println!("No search results");
        return Ok(());
    }

    for item in items {
        print_item_line(ctx, &item)?;
    }

    Ok(())
}

fn print_item_line(ctx: &AppContext, item: &crate::domain::Item) -> Result<()> {
    let state = ctx.store.get_item_state(&item.id)?;
    let marker = if let Some(state) = state {
        if state.is_archived {
            "x"
        } else if state.is_saved {
            "S"
        } else if state.is_queued {
            "Q"
        } else if state.is_starred {
            "*"
        } else if state.is_read {
            " "
        } else {
            "u"
        }
    } else {
        "u"
    };

    let date = item
        .published_at
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "          ".to_string());

    println!("{} {} {}", marker, date, item.display_title());
    Ok(())
}

/// Import feeds from an OPML file
pub async fn import_opml(ctx: &AppContext, path: &Path) -> Result<()> {
    let content = std::fs::read_to_string(path)?;
    let feed_urls = parse_opml(&content)?;

    if feed_urls.is_empty() {
        println!("No feeds found in OPML file");
        return Ok(());
    }

    println!("Found {} feeds in OPML file", feed_urls.len());

    // First pass: filter out existing feeds and create new feed entries
    let mut feeds_to_fetch = Vec::new();
    let mut skipped = 0;

    for (title, url) in feed_urls {
        if ctx.store.get_feed_by_url(&url)?.is_some() {
            skipped += 1;
            continue;
        }

        // Create the feed entry
        let feed = Feed::new(url.clone());
        let feed_id = ctx.store.add_feed(&feed)?;

        // Store with OPML title as fallback
        let mut feed_with_id = feed;
        feed_with_id.id = feed_id;
        feed_with_id.title = Some(title);
        feeds_to_fetch.push(feed_with_id);
    }

    if feeds_to_fetch.is_empty() {
        println!("All feeds already exist ({} skipped)", skipped);
        return Ok(());
    }

    println!("Fetching {} new feeds in parallel...", feeds_to_fetch.len());
    let run_id = ctx
        .store
        .begin_refresh_run(RefreshSource::Import, feeds_to_fetch.len())?;

    // Parallel fetch all new feeds
    let results = ctx
        .parallel_fetcher
        .fetch_all(
            feeds_to_fetch.clone(),
            ctx.store.clone(),
            &ctx.normalizer,
            None,
        )
        .await;

    let mut added = 0;
    let mut errors = 0;
    let mut total_new = 0;

    let mut imported_feed_ids = Vec::new();

    for (feed_id, result) in results {
        let feed = feeds_to_fetch.iter().find(|f| f.id == feed_id);
        let title = feed
            .and_then(|f| f.title.as_ref())
            .map(|s| s.as_str())
            .unwrap_or("Unknown");

        match result {
            Ok(refresh) => {
                ctx.store
                    .record_refresh_run_items(run_id, feed_id, &refresh.inserted_item_ids)?;
                total_new += refresh.new_count;
                println!("  + {} ({} items)", title, refresh.new_count);
                added += 1;
                imported_feed_ids.push(feed_id);
            }
            Err(e) => {
                eprintln!("  ! {} - error: {}", title, e);
                // Delete the feed entry on error
                let _ = ctx.store.delete_feed(feed_id);
                errors += 1;
            }
        }
    }
    ctx.store.complete_refresh_run(run_id, total_new, errors)?;

    // Queue items from imported feeds for background scraping
    // Only queue items that actually need scraping (no content and no summary)
    if ctx.scraper_handle.is_some() && !imported_feed_ids.is_empty() {
        let mut items_to_scrape = Vec::new();
        for feed_id in imported_feed_ids {
            if let Ok(items) = ctx.store.get_items_by_feed(feed_id) {
                items_to_scrape.extend(items.into_iter().filter(ChromeScraper::needs_scraping));
            }
        }
        if !items_to_scrape.is_empty() {
            println!(
                "Queuing {} items for background content scraping...",
                items_to_scrape.len()
            );
            ctx.queue_for_scraping(items_to_scrape).await;
        }
    }

    println!(
        "\nImport complete: {} added, {} skipped (already exist), {} errors",
        added, skipped, errors
    );

    Ok(())
}

/// Parse OPML content and extract feed URLs with titles
fn parse_opml(content: &str) -> Result<Vec<(String, String)>> {
    let mut feeds = Vec::new();

    // Simple regex-free parsing: find all outline elements with xmlUrl
    for line in content.lines() {
        if line.contains("xmlUrl") {
            if let (Some(title), Some(url)) = (
                extract_attr(line, "title").or_else(|| extract_attr(line, "text")),
                extract_attr(line, "xmlUrl"),
            ) {
                parse_http_url(&url)?;
                feeds.push((title, url));
            }
        }
    }

    Ok(feeds)
}

/// Extract an attribute value from an XML element string
fn extract_attr(line: &str, attr: &str) -> Option<String> {
    let pattern = format!("{}=\"", attr);
    let start = line.find(&pattern)? + pattern.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    let value = &rest[..end];
    Some(html_escape::decode_html_entities(value).to_string())
}

/// Scrape full content for items that only have summaries
pub async fn scrape_content(
    ctx: &AppContext,
    feed_url: Option<&str>,
    limit: usize,
    concurrency: usize,
    visible: bool,
    auth_profile: Option<&str>,
) -> Result<()> {
    // Get items to scrape
    let items = if let Some(url) = feed_url {
        parse_http_url(url)?;
        let feed = ctx
            .store
            .get_feed_by_url(url)?
            .ok_or_else(|| RivuletError::FeedNotFound(url.to_string()))?;
        ctx.store.get_items_by_feed(feed.id)?
    } else {
        ctx.store.get_all_items()?
    };

    // Filter items that need scraping (have link but no/short content)
    let items_to_scrape: Vec<_> = items
        .into_iter()
        .filter(ChromeScraper::needs_scraping)
        .take(limit)
        .collect();

    if items_to_scrape.is_empty() {
        println!("No items need scraping");
        return Ok(());
    }

    println!(
        "Scraping {} items with {} concurrent pages...",
        items_to_scrape.len(),
        concurrency
    );

    // Create scraper config
    let user_data_dir = if let Some(name) = auth_profile {
        let profile = ctx
            .store
            .get_auth_profile_by_name(name)?
            .ok_or_else(|| RivuletError::Config(format!("Auth profile not found: {}", name)))?;
        Some(PathBuf::from(profile.profile_dir))
    } else {
        None
    };

    let config = ScraperConfig {
        headless: !visible,
        max_concurrency: concurrency,
        user_data_dir,
        ..Default::default()
    };

    // Initialize the scraper
    let scraper = ChromeScraper::new(config).await?;

    // Scrape all items
    let results = scraper.scrape_items(&items_to_scrape, concurrency).await;

    let mut success = 0;
    let mut errors = 0;

    for (item_id, result) in results {
        let item = items_to_scrape.iter().find(|i| i.id == item_id);
        let title = item.map(|i| i.display_title()).unwrap_or("Unknown");

        match result {
            Ok(scrape_result) => {
                // Update item content in store
                ctx.store
                    .update_item_content(&item_id, &scrape_result.content)?;
                let content_type = if scrape_result.is_html {
                    "HTML"
                } else {
                    "text"
                };
                println!(
                    "  + {} ({}, {} chars)",
                    title,
                    content_type,
                    scrape_result.content.len()
                );
                success += 1;
            }
            Err(e) => {
                eprintln!("  ! {} - error: {}", title, e);
                errors += 1;
            }
        }
    }

    println!(
        "\nScraping complete: {} succeeded, {} failed",
        success, errors
    );

    Ok(())
}

pub async fn auth_add(
    ctx: &AppContext,
    name: &str,
    site_url: &str,
    profile_dir: Option<PathBuf>,
) -> Result<()> {
    parse_http_url(site_url)?;
    let profile_dir = profile_dir.unwrap_or(default_auth_profile_dir(name)?);
    std::fs::create_dir_all(&profile_dir)?;

    let profile = AuthProfile::new(
        name.to_string(),
        site_url.to_string(),
        profile_dir.to_string_lossy().to_string(),
    );
    let profile_id = ctx.store.add_auth_profile(&profile)?;

    println!("Auth profile: {}", name);
    println!("Site: {}", site_url);
    println!("Chrome profile directory: {}", profile_dir.display());
    println!(
        "A visible Chrome window will open. Log in normally, then return here and press Enter."
    );
    println!(
        "Rivulet stores profile metadata only; cookies remain inside Chrome's profile directory."
    );

    let mut config = ScraperConfig {
        headless: false,
        user_data_dir: Some(profile_dir),
        block_images: false,
        block_stylesheets: false,
        ..Default::default()
    };
    config.enabled = false;

    let scraper = ChromeScraper::new(config).await?;
    scraper.open_interactive_page(site_url).await?;

    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;

    ctx.store
        .update_auth_profile_status(profile_id, "login captured")?;
    println!("Saved auth profile metadata for '{}'", name);
    Ok(())
}

pub async fn auth_check(
    ctx: &AppContext,
    name: &str,
    url: Option<&str>,
    visible: bool,
) -> Result<()> {
    let profile = ctx
        .store
        .get_auth_profile_by_name(name)?
        .ok_or_else(|| RivuletError::Config(format!("Auth profile not found: {}", name)))?;
    let check_url = url.unwrap_or(&profile.site_url);
    parse_http_url(check_url)?;

    let config = ScraperConfig {
        headless: !visible,
        user_data_dir: Some(PathBuf::from(&profile.profile_dir)),
        block_images: false,
        block_stylesheets: false,
        ..Default::default()
    };

    let scraper = ChromeScraper::new(config).await?;
    match scraper.scrape(check_url).await {
        Ok(result) => {
            let status = format!("ok: extracted {} chars", result.content.len());
            ctx.store.update_auth_profile_status(profile.id, &status)?;
            println!("{} ({})", status, check_url);
            Ok(())
        }
        Err(e) => {
            let status = format!("failed: {}", e);
            ctx.store.update_auth_profile_status(profile.id, &status)?;
            Err(e)
        }
    }
}

pub fn auth_list(ctx: &AppContext) -> Result<()> {
    let profiles = ctx.store.get_all_auth_profiles()?;
    if profiles.is_empty() {
        println!("No auth profiles");
        return Ok(());
    }

    for profile in profiles {
        let checked = profile
            .last_checked_at
            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "never".to_string());
        let status = profile.last_status.unwrap_or_else(|| "unknown".to_string());
        println!(
            "{}\n  site: {}\n  profile_dir: {}\n  last_check: {} ({})",
            profile.name, profile.site_url, profile.profile_dir, checked, status
        );
    }

    Ok(())
}

fn default_auth_profile_dir(name: &str) -> Result<PathBuf> {
    let data_dir = dirs::data_dir()
        .ok_or_else(|| RivuletError::Config("Could not find data directory".into()))?;
    Ok(data_dir
        .join("rivulet")
        .join("auth-profiles")
        .join(sanitize_profile_name(name)))
}

fn sanitize_profile_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();

    let trimmed = sanitized.trim_matches('-');
    if trimmed.is_empty() {
        "default".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_opml_basic() {
        let opml = r#"<?xml version="1.0" encoding="UTF-8"?>
<opml version="2.0">
  <body>
    <outline text="Tech" title="Tech">
      <outline title="Hacker News" text="HN" xmlUrl="https://news.ycombinator.com/rss" htmlUrl="https://news.ycombinator.com"/>
      <outline title="Lobsters" text="Lobsters" xmlUrl="https://lobste.rs/rss" htmlUrl="https://lobste.rs"/>
    </outline>
  </body>
</opml>"#;
        let feeds = parse_opml(opml).unwrap();
        assert_eq!(feeds.len(), 2);
        assert_eq!(feeds[0].0, "Hacker News");
        assert_eq!(feeds[0].1, "https://news.ycombinator.com/rss");
        assert_eq!(feeds[1].0, "Lobsters");
        assert_eq!(feeds[1].1, "https://lobste.rs/rss");
    }

    #[test]
    fn test_parse_opml_text_fallback() {
        let opml = r#"<opml version="2.0">
  <body>
    <outline text="My Blog" xmlUrl="https://example.com/feed.xml"/>
  </body>
</opml>"#;
        let feeds = parse_opml(opml).unwrap();
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0].0, "My Blog");
    }

    #[test]
    fn test_parse_opml_html_entities() {
        let opml = r#"<opml version="2.0">
  <body>
    <outline title="Tom &amp; Jerry&apos;s Blog" xmlUrl="https://example.com/feed.xml"/>
  </body>
</opml>"#;
        let feeds = parse_opml(opml).unwrap();
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0].0, "Tom & Jerry's Blog");
    }

    #[test]
    fn test_parse_opml_empty() {
        let opml = r#"<?xml version="1.0" encoding="UTF-8"?>
<opml version="2.0">
  <body>
    <outline text="Category" title="Category"/>
  </body>
</opml>"#;
        let feeds = parse_opml(opml).unwrap();
        assert!(feeds.is_empty());
    }

    #[test]
    fn test_parse_opml_no_title_no_text() {
        let opml = r#"<opml version="2.0">
  <body>
    <outline xmlUrl="https://example.com/feed.xml"/>
  </body>
</opml>"#;
        let feeds = parse_opml(opml).unwrap();
        assert!(feeds.is_empty());
    }

    #[test]
    fn test_extract_attr_basic() {
        let line = r#"<outline title="My Feed" xmlUrl="https://example.com/rss"/>"#;
        assert_eq!(extract_attr(line, "title"), Some("My Feed".to_string()));
        assert_eq!(
            extract_attr(line, "xmlUrl"),
            Some("https://example.com/rss".to_string())
        );
    }

    #[test]
    fn test_extract_attr_missing() {
        let line = r#"<outline title="My Feed"/>"#;
        assert_eq!(extract_attr(line, "xmlUrl"), None);
    }

    #[test]
    fn test_extract_attr_html_entities() {
        let line = r#"<outline title="A &amp; B"/>"#;
        assert_eq!(extract_attr(line, "title"), Some("A & B".to_string()));
    }

    #[test]
    fn test_list_filter_from_flags() {
        assert_eq!(
            list_filter_from_flags(true, false, false, false, false).unwrap(),
            Some(ItemListFilter::Unread)
        );
        assert_eq!(
            list_filter_from_flags(false, true, false, false, false).unwrap(),
            Some(ItemListFilter::Starred)
        );
        assert_eq!(
            list_filter_from_flags(false, false, true, false, false).unwrap(),
            Some(ItemListFilter::Queued)
        );
        assert_eq!(
            list_filter_from_flags(false, false, false, true, false).unwrap(),
            Some(ItemListFilter::Saved)
        );
        assert_eq!(
            list_filter_from_flags(false, false, false, false, true).unwrap(),
            Some(ItemListFilter::Archived)
        );
        assert_eq!(
            list_filter_from_flags(false, false, false, false, false).unwrap(),
            None
        );
        assert!(list_filter_from_flags(true, true, false, false, false).is_err());
    }

    #[test]
    fn test_sanitize_profile_name() {
        assert_eq!(sanitize_profile_name("New York Times"), "New-York-Times");
        assert_eq!(sanitize_profile_name("paid_site-1"), "paid_site-1");
        assert_eq!(sanitize_profile_name("***"), "default");
    }
}
