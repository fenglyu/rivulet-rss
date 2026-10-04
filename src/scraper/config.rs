use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

/// Configuration for the web scraper
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScraperConfig {
    /// Enable automatic background scraping (default: true)
    pub enabled: bool,

    /// Whether to run the browser in headless mode (default: true)
    pub headless: bool,

    /// Minimum content length to consider item as having content (default: 200)
    pub min_content_length: usize,

    /// Page load timeout in seconds (default: 30)
    pub timeout_secs: u64,

    /// Wait time after page load for dynamic content in milliseconds (default: 1000)
    pub wait_after_load_ms: u64,

    /// CSS selectors to try for article content extraction, in priority order
    pub content_selectors: Vec<String>,

    /// CSS selectors for elements to remove (ads, navigation, etc.)
    pub remove_selectors: Vec<String>,

    /// Maximum concurrent browser pages (default: 5)
    pub max_concurrency: usize,

    /// Maximum extracted article size in bytes (default: 2 MiB)
    pub max_content_bytes: usize,

    /// Whether to block images for faster loading (default: true)
    pub block_images: bool,

    /// Whether to block stylesheets for faster loading (default: true)
    pub block_stylesheets: bool,

    /// User agent string to use
    pub user_agent: Option<String>,

    /// Optional persistent Chrome user data directory for authenticated sessions
    pub user_data_dir: Option<PathBuf>,

    /// Restore cookies without an expiry when reopening a persistent profile.
    /// Only useful for a profile dedicated to this application (default: false).
    pub restore_session: bool,
}

impl Default for ScraperConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            headless: true,
            min_content_length: 200,
            timeout_secs: 30,
            wait_after_load_ms: 1000,
            content_selectors: vec![
                // Common article content selectors in priority order
                "article".to_string(),
                "[role=\"main\"]".to_string(),
                "main".to_string(),
                ".post-content".to_string(),
                ".article-content".to_string(),
                ".entry-content".to_string(),
                ".content".to_string(),
                "#content".to_string(),
                ".post".to_string(),
                ".article".to_string(),
                ".blog-post".to_string(),
            ],
            remove_selectors: vec![
                // Common elements to remove
                "nav".to_string(),
                "header".to_string(),
                "footer".to_string(),
                "aside".to_string(),
                ".sidebar".to_string(),
                ".advertisement".to_string(),
                ".ad".to_string(),
                ".ads".to_string(),
                ".social-share".to_string(),
                ".comments".to_string(),
                ".related-posts".to_string(),
                "script".to_string(),
                "style".to_string(),
                "noscript".to_string(),
            ],
            max_concurrency: 5,
            max_content_bytes: 2 * 1024 * 1024,
            block_images: true,
            block_stylesheets: true,
            user_agent: Some(
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
                 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
                    .to_string(),
            ),
            user_data_dir: None,
            restore_session: false,
        }
    }
}

impl ScraperConfig {
    /// Validate values that would otherwise disable timeouts or deadlock.
    pub fn validate(&self) -> Result<(), String> {
        if self.timeout_secs == 0 {
            return Err("scraper.timeout_secs must be greater than zero".to_string());
        }
        if self.max_concurrency == 0 {
            return Err("scraper.max_concurrency must be greater than zero".to_string());
        }
        if self.max_content_bytes == 0 {
            return Err("scraper.max_content_bytes must be greater than zero".to_string());
        }
        Ok(())
    }

    /// Get the page load timeout as a Duration
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs)
    }

    /// Get the wait time after load as a Duration
    pub fn wait_after_load(&self) -> Duration {
        Duration::from_millis(self.wait_after_load_ms)
    }

    /// Create a config optimized for speed (less accurate)
    pub fn fast() -> Self {
        Self {
            timeout_secs: 15,
            wait_after_load_ms: 500,
            max_concurrency: 10,
            ..Default::default()
        }
    }

    /// Create a config optimized for accuracy (slower)
    pub fn thorough() -> Self {
        Self {
            timeout_secs: 60,
            wait_after_load_ms: 2000,
            max_concurrency: 3,
            block_images: false,
            block_stylesheets: false,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_values() {
        let config = ScraperConfig::default();
        assert!(config.enabled);
        assert!(config.headless);
        assert_eq!(config.min_content_length, 200);
        assert_eq!(config.timeout_secs, 30);
        assert_eq!(config.wait_after_load_ms, 1000);
        assert_eq!(config.max_concurrency, 5);
        assert_eq!(config.max_content_bytes, 2 * 1024 * 1024);
        assert!(config.block_images);
        assert!(config.block_stylesheets);
        assert!(!config.content_selectors.is_empty());
        assert!(!config.remove_selectors.is_empty());
    }

    #[test]
    fn test_fast_config() {
        let config = ScraperConfig::fast();
        assert_eq!(config.timeout_secs, 15);
        assert_eq!(config.wait_after_load_ms, 500);
        assert_eq!(config.max_concurrency, 10);
        // Inherits defaults for the rest
        assert!(config.block_images);
    }

    #[test]
    fn test_thorough_config() {
        let config = ScraperConfig::thorough();
        assert_eq!(config.timeout_secs, 60);
        assert_eq!(config.wait_after_load_ms, 2000);
        assert_eq!(config.max_concurrency, 3);
        assert!(!config.block_images);
        assert!(!config.block_stylesheets);
    }

    #[test]
    fn test_timeout_duration() {
        let config = ScraperConfig::default();
        assert_eq!(config.timeout(), Duration::from_secs(30));

        let fast = ScraperConfig::fast();
        assert_eq!(fast.timeout(), Duration::from_secs(15));
    }

    #[test]
    fn test_wait_after_load_duration() {
        let config = ScraperConfig::default();
        assert_eq!(config.wait_after_load(), Duration::from_millis(1000));

        let thorough = ScraperConfig::thorough();
        assert_eq!(thorough.wait_after_load(), Duration::from_millis(2000));
    }

    #[test]
    fn test_rejects_zero_security_limits() {
        let config = ScraperConfig {
            timeout_secs: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());

        let config = ScraperConfig {
            max_concurrency: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());

        let config = ScraperConfig {
            max_content_bytes: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }
}
