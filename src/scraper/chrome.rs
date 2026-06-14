use std::sync::Arc;

use async_trait::async_trait;
use chromiumoxide::browser::{Browser, BrowserConfig};
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
    /// Create a new Chrome scraper with the given configuration
    pub async fn new(config: ScraperConfig) -> Result<Self> {
        config.validate().map_err(RivuletError::Config)?;

        let mut builder = BrowserConfig::builder()
            .arg("--disable-gpu")
            .arg("--disable-dev-shm-usage")
            .arg("--disable-software-rasterizer");

        if !config.headless {
            builder = builder.with_head();
        }

        if let Some(ref user_data_dir) = config.user_data_dir {
            builder = builder.user_data_dir(user_data_dir);
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

    /// Scrape a single page and extract content
    async fn scrape_page(&self, url: &str) -> Result<ScrapeResult> {
        scrape_browser_page(&self.browser, &self.config, &self.extractor, url).await
    }
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
    use super::truncate_utf8;

    #[test]
    fn truncates_content_on_a_utf8_boundary() {
        assert_eq!(truncate_utf8("abé".to_string(), 3), "ab");
        assert_eq!(truncate_utf8("abé".to_string(), 4), "abé");
    }
}
