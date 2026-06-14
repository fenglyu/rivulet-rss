pub mod http_fetcher;
pub mod parallel;

use async_trait::async_trait;

use crate::app::Result;

#[derive(Debug, Clone, PartialEq)]
pub enum FetchResult {
    /// New content fetched successfully
    Content {
        body: Vec<u8>,
        etag: Option<String>,
        last_modified: Option<String>,
    },
    /// Content not modified (HTTP 304)
    NotModified,
}

#[async_trait]
pub trait Fetcher {
    async fn fetch(
        &self,
        url: &str,
        etag: Option<&str>,
        last_modified: Option<&str>,
    ) -> Result<FetchResult>;
}

/// Test utilities. Exposed publicly so integration tests in `tests/` can use
/// them; not intended for production callers.
pub mod testing {
    use super::{FetchResult, Fetcher};
    use crate::app::Result;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::Duration;

    /// In-memory fetcher that returns canned responses keyed by URL.
    /// Optional `delay` simulates network latency for refresh-progress tests.
    pub struct MockFetcher {
        responses: Mutex<HashMap<String, FetchResult>>,
        delay: Duration,
    }

    impl MockFetcher {
        pub fn new() -> Self {
            Self {
                responses: Mutex::new(HashMap::new()),
                delay: Duration::ZERO,
            }
        }

        pub fn with_delay(delay: Duration) -> Self {
            Self {
                responses: Mutex::new(HashMap::new()),
                delay,
            }
        }

        pub fn set_response(&self, url: impl Into<String>, result: FetchResult) {
            self.responses.lock().unwrap().insert(url.into(), result);
        }
    }

    impl Default for MockFetcher {
        fn default() -> Self {
            Self::new()
        }
    }

    #[async_trait]
    impl Fetcher for MockFetcher {
        async fn fetch(
            &self,
            url: &str,
            _etag: Option<&str>,
            _last_modified: Option<&str>,
        ) -> Result<FetchResult> {
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            self.responses
                .lock()
                .unwrap()
                .get(url)
                .cloned()
                .ok_or_else(|| {
                    crate::app::error::RivuletError::Other(format!(
                        "No mock response for URL: {url}"
                    ))
                })
        }
    }
}
