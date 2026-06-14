pub mod commands;

use clap::{Parser, Subcommand};

use crate::fetcher::parallel::DEFAULT_WORKERS;

#[derive(Parser)]
#[command(name = "rivulet")]
#[command(version)]
#[command(about = "A terminal RSS/Atom reader", long_about = None)]
pub struct Cli {
    /// Number of parallel workers for fetching feeds
    #[arg(
        short,
        long,
        default_value_t = DEFAULT_WORKERS,
        global = true,
        value_parser = parse_positive_usize
    )]
    pub workers: usize,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize config file with all options
    Init {
        /// Overwrite existing config
        #[arg(long)]
        force: bool,
    },
    /// Add a new feed
    Add {
        /// URL of the feed to add
        url: String,
    },
    /// Remove a feed
    Remove {
        /// URL of the feed to remove
        url: String,
    },
    /// Import feeds from an OPML file
    Import {
        /// Path to the OPML file
        path: std::path::PathBuf,
    },
    /// Update all feeds
    Update,
    /// List feeds or items
    List {
        /// Show items instead of feeds
        #[arg(long)]
        items: bool,

        /// Show unread items
        #[arg(long)]
        unread: bool,

        /// Show starred items
        #[arg(long)]
        starred: bool,

        /// Show queued/read-later items
        #[arg(long)]
        queued: bool,

        /// Show saved items
        #[arg(long)]
        saved: bool,

        /// Show archived items
        #[arg(long)]
        archived: bool,
    },
    /// Search locally indexed article titles, summaries, and scraped content
    Search {
        /// Search query
        query: String,

        /// Maximum number of results
        #[arg(short, long, default_value = "20")]
        limit: usize,

        /// Search unread items
        #[arg(long)]
        unread: bool,

        /// Search starred items
        #[arg(long)]
        starred: bool,

        /// Search queued/read-later items
        #[arg(long)]
        queued: bool,

        /// Search saved items
        #[arg(long)]
        saved: bool,

        /// Search archived items
        #[arg(long)]
        archived: bool,
    },
    /// Launch the TUI
    Tui,
    /// Background daemon for automatic updates
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
    /// Scrape full content for items using headless Chrome
    Scrape {
        /// Only scrape items from a specific feed URL
        #[arg(long)]
        feed: Option<String>,

        /// Maximum number of items to scrape
        #[arg(short, long, default_value = "10")]
        limit: usize,

        /// Number of concurrent browser pages
        #[arg(
            short,
            long,
            default_value = "3",
            value_parser = parse_positive_usize
        )]
        concurrency: usize,

        /// Run in non-headless mode (show browser)
        #[arg(long)]
        visible: bool,

        /// Use a stored authenticated Chrome profile
        #[arg(long)]
        auth_profile: Option<String>,
    },
    /// Manage authenticated Chrome profiles for paid/private sites
    Auth {
        #[command(subcommand)]
        action: AuthAction,
    },
}

#[derive(Subcommand)]
pub enum DaemonAction {
    /// Start the background daemon
    Start {
        /// Update interval (e.g., "1h", "30m", "6h", "1d")
        #[arg(short, long, default_value = "1h")]
        interval: String,

        /// Skip initial update on start
        #[arg(long)]
        no_initial_update: bool,

        /// Log file path (default: stdout)
        #[arg(short, long)]
        log: Option<std::path::PathBuf>,

        /// Run in foreground (don't detach)
        #[arg(short, long)]
        foreground: bool,

        /// Internal token used to identify a detached daemon process
        #[arg(long, hide = true)]
        instance_token: Option<String>,
    },
    /// Stop the running daemon
    Stop,
    /// Check daemon status
    Status,
}

#[derive(Subcommand)]
pub enum AuthAction {
    /// Create/update a profile and open a visible Chrome login session
    Add {
        /// Profile name
        name: String,

        /// Site URL to open for login
        #[arg(long)]
        site: String,

        /// Override profile directory; defaults under Rivulet's data dir
        #[arg(long)]
        profile_dir: Option<std::path::PathBuf>,
    },
    /// Open a URL with a stored profile and report whether content can be extracted
    Check {
        /// Profile name
        name: String,

        /// URL to check; defaults to the profile site URL
        #[arg(long)]
        url: Option<String>,

        /// Show browser while checking
        #[arg(long)]
        visible: bool,
    },
    /// List configured auth profiles
    List,
}

fn parse_positive_usize(value: &str) -> std::result::Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| format!("{value:?} is not a valid positive integer"))?;
    if parsed == 0 {
        return Err("value must be greater than zero".to_string());
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::Cli;

    #[test]
    fn rejects_zero_workers() {
        assert!(Cli::try_parse_from(["rivulet", "--workers", "0", "list"]).is_err());
    }

    #[test]
    fn rejects_zero_scrape_concurrency() {
        assert!(Cli::try_parse_from(["rivulet", "scrape", "--concurrency", "0"]).is_err());
    }
}
