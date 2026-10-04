use std::{
    ffi::OsString,
    path::Path,
    process::{Child, Command, Stdio},
    sync::Arc,
};

use async_trait::async_trait;
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::detection::{default_executable, DetectionOptions};
use chromiumoxide::Page;
use futures::StreamExt;
use tokio::sync::Semaphore;
use tokio::time::timeout;

use crate::app::{Result, RivuletError};
use crate::domain::Item;
use crate::scraper::config::ScraperConfig;
use crate::scraper::extractor::ContentExtractor;
use crate::scraper::{ScrapeResult, Scraper};
use crate::url_policy::parse_http_url;

/// Chrome-based web scraper using chromiumoxide
pub struct ChromeScraper {
    browser: Arc<Browser>,
    config: ScraperConfig,
    extractor: ContentExtractor,
    semaphore: Arc<Semaphore>,
}

impl ChromeScraper {
    /// Open a user-controlled browser without CDP or automation flags.
    /// The caller must wait for the user to exit it before reusing the profile.
    pub fn open_user_browser(profile_dir: &Path, url: &str) -> Result<Child> {
        let executable =
            default_executable(DetectionOptions::default()).map_err(RivuletError::Scraper)?;
        user_browser_command(&executable, profile_dir, url)?
            .spawn()
            .map_err(|error| {
                RivuletError::Scraper(format!("Failed to open login browser: {error}"))
            })
    }

    /// Create a new Chrome scraper with the given configuration
    pub async fn new(config: ScraperConfig) -> Result<Self> {
        config.validate().map_err(RivuletError::Config)?;

        let mut builder = BrowserConfig::builder()
            .respect_https_errors()
            .arg("disable-gpu")
            .arg("disable-dev-shm-usage")
            .arg("disable-software-rasterizer");

        if config.preserve_browser_defaults {
            builder = builder.disable_default_args().arg("no-first-run");
        }

        if !config.headless {
            builder = builder.with_head();
        }

        if let Some(ref user_data_dir) = config.user_data_dir {
            builder = builder.user_data_dir(user_data_dir);
            if config.restore_session {
                builder = builder.arg("restore-last-session");
            }
        }

        let browser_config = builder
            .build()
            .map_err(|e| RivuletError::Scraper(format!("Failed to build browser config: {}", e)))?;

        let (browser, mut handler) = Browser::launch(browser_config).await.map_err(|e| {
            RivuletError::Scraper(format!(
                "Failed to launch browser: {}. Is Chrome or Chromium installed and in PATH?",
                e
            ))
        })?;

        // Spawn the browser handler
        tokio::spawn(async move {
            while let Some(_event) = handler.next().await {
                // Handle browser events
            }
        });

        let semaphore = Arc::new(Semaphore::new(config.max_concurrency));
        let extractor = ContentExtractor::new(config.clone());

        Ok(Self {
            browser: Arc::new(browser),
            config,
            extractor,
            semaphore,
        })
    }

    /// Create a new Chrome scraper with default configuration
    pub async fn with_defaults() -> Result<Self> {
        Self::new(ScraperConfig::default()).await
    }

    /// Close our browser and wait for its profile lock to be released.
    pub async fn close(self) -> Result<()> {
        let mut browser = Arc::try_unwrap(self.browser)
            .map_err(|_| RivuletError::Scraper("Browser still has active operations".into()))?;
        timeout(self.config.timeout(), browser.close())
            .await
            .map_err(|_| scraper_timeout("closing the browser"))?
            .map_err(|error| RivuletError::Scraper(format!("Browser close failed: {error}")))?;
        timeout(self.config.timeout(), browser.wait())
            .await
            .map_err(|_| scraper_timeout("waiting for the browser to close"))?
            .map_err(|error| RivuletError::Scraper(format!("Browser wait failed: {error}")))?;
        Ok(())
    }

    /// Open a page and leave it open for interactive login or inspection.
    pub async fn open_interactive_page(&self, url: &str) -> Result<()> {
        let url = parse_http_url(url)?;
        let page = create_page(&self.browser, url.as_str(), &self.config).await?;

        if let Some(ref ua) = self.config.user_agent {
            timeout(self.config.timeout(), page.set_user_agent(ua))
                .await
                .map_err(|_| scraper_timeout("setting the user agent"))?
                .map_err(|e| RivuletError::Scraper(format!("Failed to set user agent: {}", e)))?;
        }

        timeout(self.config.timeout(), page.wait_for_navigation())
            .await
            .map_err(|_| scraper_timeout("waiting for navigation"))?
            .map_err(|e| RivuletError::Scraper(format!("Navigation failed: {}", e)))?;

        Ok(())
    }

    /// Evaluate a bounded extraction script in a normally loaded browser page.
    /// The caller validates the final URL and the meaning of the returned content.
    pub async fn evaluate_page(&self, url: &str, script: &str) -> Result<serde_json::Value> {
        let url = parse_http_url(url)?;
        let page = create_page(&self.browser, url.as_str(), &self.config).await?;
        let result = timeout(self.config.timeout(), async {
            page.wait_for_navigation()
                .await
                .map_err(|error| RivuletError::Scraper(format!("Navigation failed: {error}")))?;
            tokio::time::sleep(self.config.wait_after_load()).await;
            page.evaluate(script)
                .await
                .map_err(|error| RivuletError::Scraper(format!("Page extraction failed: {error}")))?
                .into_value()
                .map_err(|error| {
                    RivuletError::Scraper(format!("Invalid page extraction result: {error:?}"))
                })
        })
        .await
        .map_err(|_| scraper_timeout("loading a subscription page"))
        .and_then(|result| result);
        if let Err(error) = page.close().await {
            tracing::debug!("Failed to close browser page: {error}");
        }
        result
    }

    /// Scrape a single page and extract content
    async fn scrape_page(&self, url: &str) -> Result<ScrapeResult> {
        scrape_browser_page(&self.browser, &self.config, &self.extractor, url).await
    }
}

fn user_browser_command(executable: &Path, profile_dir: &Path, url: &str) -> Result<Command> {
    let url = parse_http_url(url)?;
    let mut profile_arg = OsString::from("--user-data-dir=");
    profile_arg.push(profile_dir);
    let mut command = Command::new(executable);
    command
        .args([
            profile_arg,
            "--restore-last-session".into(),
            "--new-window".into(),
            "--no-first-run".into(),
        ])
        .arg(url.as_str())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    Ok(command)
}

#[async_trait]
impl Scraper for ChromeScraper {
    async fn scrape(&self, url: &str) -> Result<ScrapeResult> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|e| RivuletError::Scraper(format!("Semaphore error: {}", e)))?;

        self.scrape_page(url).await
    }

    async fn scrape_items(
        &self,
        items: &[Item],
        concurrency: usize,
    ) -> Vec<(String, Result<ScrapeResult>)> {
        if concurrency == 0 {
            return items
                .iter()
                .filter(|item| Self::needs_scraping(item))
                .map(|item| {
                    (
                        item.id.clone(),
                        Err(RivuletError::Config(
                            "Scraper concurrency must be greater than zero".to_string(),
                        )),
                    )
                })
                .collect();
        }

        let semaphore = Arc::new(Semaphore::new(concurrency));
        let mut handles = Vec::new();

        for item in items {
            if !Self::needs_scraping(item) {
                continue;
            }

            let Some(ref url) = item.link else {
                continue;
            };

            let item_id = item.id.clone();
            let url = url.clone();
            let sem = semaphore.clone();
            let browser = self.browser.clone();
            let config = self.config.clone();

            let handle = tokio::spawn(async move {
                let _permit = match sem.acquire().await {
                    Ok(permit) => permit,
                    Err(error) => {
                        return (
                            item_id,
                            Err(RivuletError::Scraper(format!("Semaphore error: {}", error))),
                        );
                    }
                };
                let extractor = ContentExtractor::new(config.clone());
                let result = scrape_browser_page(&browser, &config, &extractor, &url).await;

                (item_id, result)
            });

            handles.push(handle);
        }

        let mut results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(e) => {
                    tracing::error!("Task join error: {}", e);
                }
            }
        }

        results
    }
}

async fn create_page(browser: &Browser, url: &str, config: &ScraperConfig) -> Result<Page> {
    timeout(config.timeout(), browser.new_page(url))
        .await
        .map_err(|_| scraper_timeout("creating the page"))?
        .map_err(|e| RivuletError::Scraper(format!("Failed to create page: {}", e)))
}

async fn scrape_browser_page(
    browser: &Browser,
    config: &ScraperConfig,
    extractor: &ContentExtractor,
    url: &str,
) -> Result<ScrapeResult> {
    let url = parse_http_url(url)?;
    let page = create_page(browser, url.as_str(), config).await?;
    let operation = timeout(config.timeout(), async {
        if let Some(ref ua) = config.user_agent {
            page.set_user_agent(ua)
                .await
                .map_err(|e| RivuletError::Scraper(format!("Failed to set user agent: {}", e)))?;
        }

        page.wait_for_navigation()
            .await
            .map_err(|e| RivuletError::Scraper(format!("Navigation failed: {}", e)))?;
        tokio::time::sleep(config.wait_after_load()).await;

        let result: serde_json::Value = page
            .evaluate(extractor.extraction_script())
            .await
            .map_err(|e| RivuletError::Scraper(format!("Script execution failed: {}", e)))?
            .into_value()
            .map_err(|e| RivuletError::Scraper(format!("Failed to parse result: {:?}", e)))?;

        scrape_result_from_value(result, config.max_content_bytes)
    })
    .await
    .map_err(|_| scraper_timeout("loading and extracting the page"))
    .and_then(|result| result);

    if let Err(error) = page.close().await {
        tracing::debug!("Failed to close browser page: {}", error);
    }

    operation
}

fn scrape_result_from_value(
    result: serde_json::Value,
    max_content_bytes: usize,
) -> Result<ScrapeResult> {
    let html = truncate_utf8(
        result["html"].as_str().unwrap_or("").to_string(),
        max_content_bytes,
    );
    let text = truncate_utf8(
        result["text"].as_str().unwrap_or("").to_string(),
        max_content_bytes,
    );

    if !html.is_empty() {
        Ok(ScrapeResult {
            content: html,
            is_html: true,
        })
    } else if !text.is_empty() {
        Ok(ScrapeResult {
            content: text,
            is_html: false,
        })
    } else {
        Err(RivuletError::Scraper("No content extracted".to_string()))
    }
}

fn truncate_utf8(mut content: String, max_bytes: usize) -> String {
    if content.len() <= max_bytes {
        return content;
    }

    let mut end = max_bytes;
    while !content.is_char_boundary(end) {
        end -= 1;
    }
    content.truncate(end);
    content
}

fn scraper_timeout(action: &str) -> RivuletError {
    RivuletError::Scraper(format!("Timed out while {}", action))
}

#[cfg(test)]
mod tests {
    use super::{truncate_utf8, user_browser_command};
    use std::path::Path;

    #[test]
    fn login_uses_an_isolated_profile_without_browser_automation() {
        let profile = Path::new("profiles with spaces/bloomberg");
        let command =
            user_browser_command(Path::new("chrome"), profile, "https://www.bloomberg.com/")
                .unwrap();
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"--user-data-dir=profiles with spaces/bloomberg".into()));
        assert_eq!(args.last().unwrap(), "https://www.bloomberg.com/");
        for flag in [
            "remote-debugging",
            "enable-automation",
            "headless",
            "disable-blink-features",
            "password-store",
            "use-mock-keychain",
        ] {
            assert!(
                !args.iter().any(|arg| arg.contains(flag)),
                "unexpected login flag: {flag}"
            );
        }
    }

    #[test]
    fn login_rejects_non_web_navigation() {
        assert!(user_browser_command(
            Path::new("chrome"),
            Path::new("profile"),
            "javascript:alert(1)"
        )
        .is_err());
    }

    #[test]
    fn truncates_content_on_a_utf8_boundary() {
        assert_eq!(truncate_utf8("abé".to_string(), 3), "ab");
        assert_eq!(truncate_utf8("abé".to_string(), 4), "abé");
    }
}
