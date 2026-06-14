//! # Rivulet
//!
//! A terminal-first, offline-first RSS/Atom feed reader.
//!
//! ## Architecture
//!
//! Rivulet follows a modular pipeline architecture:
//!
//! ```text
//! Fetcher → Normalizer → Store → Index → UI
//! ```
//!
//! - [`fetcher`]: HTTP client with ETag/conditional request support
//! - [`normalizer`]: Converts RSS/Atom feeds to unified domain models
//! - [`store`]: SQLite persistence layer
//! - [`tui`]: Terminal user interface built with ratatui
//!
//! ## Quick Start
//!
//! ```bash
//! # Add a feed
//! rivulet add https://blog.rust-lang.org/feed.xml
//!
//! # List feeds
//! rivulet list
//!
//! # Update all feeds
//! rivulet update
//!
//! # Launch TUI
//! rivulet tui
//! ```
//!
//! ## Modules
//!
//! - [`app`]: Application context and error types
//! - [`cli`]: Command-line interface definitions
//! - [`domain`]: Core domain models (Feed, Item, ItemState)
//! - [`fetcher`]: HTTP fetching with conditional requests
//! - [`normalizer`]: Feed parsing and normalization
//! - [`store`]: Database persistence
//! - [`tui`]: Terminal user interface

/// Application context and error handling.
///
/// The [`AppContext`](app::AppContext) struct wires together all components:
/// store, fetcher, normalizer.
pub mod app;

/// Configuration management for the TUI.
///
/// Loads from the platform configuration directory, supporting:
/// - Custom colors (named or hex)
/// - Custom keybindings
pub mod config;

/// Background daemon for automatic feed updates.
///
/// Provides Chrome-updater-style background updates:
/// - `rivulet daemon start` - Start the background updater
/// - `rivulet daemon stop` - Stop the daemon
/// - `rivulet daemon status` - Check if daemon is running
pub mod daemon;

/// Command-line interface using clap.
///
/// Defines the CLI structure and subcommands:
/// - `add <url>` - Add a new feed
/// - `remove <url>` - Remove a feed
/// - `update` - Update all feeds
/// - `list [--items]` - List feeds or items
/// - `tui` - Launch the TUI
pub mod cli;

/// Core domain models.
///
/// - [`Feed`](domain::Feed): RSS/Atom feed metadata
/// - [`Item`](domain::Item): Individual feed entries with SHA256 IDs
/// - [`ItemState`](domain::ItemState): Read/starred state
pub mod domain;

/// HTTP fetching with conditional request support.
///
/// - [`Fetcher`](fetcher::Fetcher): Async trait for feed fetching
/// - [`HttpFetcher`](fetcher::http_fetcher::HttpFetcher): reqwest-based implementation
/// - [`ParallelFetcher`](fetcher::parallel::ParallelFetcher): Concurrent fetching with semaphore
pub mod fetcher;

/// Feed parsing and normalization.
///
/// Converts RSS 0.9x/1.0/2.0, Atom 0.3/1.0, and JSON Feed 1.0
/// into unified [`Item`](domain::Item) structs.
pub mod normalizer;

/// SQLite persistence layer.
///
/// - [`Store`](store::Store): Trait defining storage operations
/// - [`SqliteStore`](store::SqliteStore): SQLite implementation
pub mod store;

/// Terminal user interface.
///
/// Three-pane layout built with ratatui:
/// - Feeds pane (compact)
/// - Items pane (40% height)
/// - Preview pane (remaining)
///
/// Keybindings: j/k navigate, Tab cycles panes, r toggles read,
/// o opens in browser, R refreshes, q quits.
pub mod tui;

/// Web scraping module for full article content extraction.
///
/// Uses headless Chrome via chromiumoxide to fetch full article content
/// from web pages when RSS feeds only provide summaries.
///
/// - [`ChromeScraper`](scraper::ChromeScraper): Chrome-based scraper
/// - [`ScraperConfig`](scraper::ScraperConfig): Configuration options
/// - [`Scraper`](scraper::Scraper): Async trait for scraping implementations
pub mod scraper;

/// URL validation shared by feed fetching, scraping, and browser opening.
pub mod url_policy;
