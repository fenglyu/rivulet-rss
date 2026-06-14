use std::sync::Arc;

use tokio::sync::Semaphore;

use crate::app::{Result, RivuletError};
use crate::domain::{Feed, FeedUpdate};
use crate::fetcher::{FetchResult, Fetcher};
use crate::normalizer::Normalizer;
use crate::store::{FeedRefreshResult, Store};

pub const DEFAULT_WORKERS: usize = 30;

pub struct ParallelFetcher {
    fetcher: Arc<dyn Fetcher + Send + Sync>,
    semaphore: Arc<Semaphore>,
}

impl ParallelFetcher {
    pub fn new(fetcher: Arc<dyn Fetcher + Send + Sync>) -> Self {
        Self {
            fetcher,
            semaphore: Arc::new(Semaphore::new(DEFAULT_WORKERS)),
        }
    }

    pub fn with_workers(fetcher: Arc<dyn Fetcher + Send + Sync>, workers: usize) -> Result<Self> {
        if workers == 0 {
            return Err(RivuletError::Config(
                "Worker count must be greater than zero".to_string(),
            ));
        }

        Ok(Self {
            fetcher,
            semaphore: Arc::new(Semaphore::new(workers)),
        })
    }

    pub async fn fetch_all<S: Store + Send + Sync + 'static>(
        &self,
        feeds: Vec<Feed>,
        store: Arc<S>,
        normalizer: &Normalizer,
        progress_tx: Option<tokio::sync::mpsc::UnboundedSender<(usize, usize)>>,
    ) -> Vec<(i64, Result<FeedRefreshResult>)> {
        let total = feeds.len();
        let mut handles = Vec::new();

        for feed in feeds {
            let fetcher = self.fetcher.clone();
            let semaphore = self.semaphore.clone();
            let store = store.clone();
            let normalizer = normalizer.clone();

            let handle = tokio::spawn(async move {
                let _permit = semaphore.acquire().await.expect("Semaphore closed");

                let result = fetch_single_feed(&fetcher, &feed, &store, &normalizer).await;
                (feed.id, result)
            });

            handles.push(handle);
        }

        let mut results = Vec::new();
        let mut current = 0;
        for handle in handles {
            match handle.await {
                Ok(result) => {
                    results.push(result);
                    current += 1;
                    if let Some(ref tx) = progress_tx {
                        let _ = tx.send((current, total));
                    }
                }
                Err(e) => {
                    tracing::error!("Task join error: {}", e);
                    current += 1;
                    if let Some(ref tx) = progress_tx {
                        let _ = tx.send((current, total));
                    }
                }
            }
        }

        results
    }
}

async fn fetch_single_feed<S: Store>(
    fetcher: &Arc<dyn Fetcher + Send + Sync>,
    feed: &Feed,
    store: &Arc<S>,
    normalizer: &Normalizer,
) -> Result<FeedRefreshResult> {
    let result = fetcher
        .fetch(
            &feed.url,
            feed.etag.as_deref(),
            feed.last_modified.as_deref(),
        )
        .await?;

    match result {
        FetchResult::NotModified => {
            tracing::debug!("Feed {} not modified", feed.url);
            Ok(FeedRefreshResult {
                feed_id: feed.id,
                new_count: 0,
                inserted_item_ids: Vec::new(),
            })
        }
        FetchResult::Content {
            body,
            etag,
            last_modified,
        } => {
            let (feed_meta, items) = normalizer.normalize(feed.id, &feed.url, &body)?;

            // Update feed metadata
            let mut update = FeedUpdate::default();
            if let Some(title) = feed_meta.title {
                update.title = Some(title);
            }
            if let Some(description) = feed_meta.description {
                update.description = Some(description);
            }
            update.etag = etag;
            update.last_modified = last_modified;
            update.last_fetched_at = Some(chrono::Utc::now());

            store.update_feed(feed.id, &update)?;

            // Add new items
            let add_result = store.add_items_with_report(&items)?;
            tracing::info!("Added {} new items from {}", add_result.count, feed.url);

            Ok(FeedRefreshResult {
                feed_id: feed.id,
                new_count: add_result.count,
                inserted_item_ids: add_result.inserted_ids,
            })
        }
    }
}
