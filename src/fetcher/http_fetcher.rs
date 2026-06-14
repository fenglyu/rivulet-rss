use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, IF_MODIFIED_SINCE, IF_NONE_MATCH};
use reqwest::{Client, StatusCode};

use crate::app::{Result, RivuletError};
use crate::fetcher::{FetchResult, Fetcher};
use crate::url_policy::parse_http_url;

const MAX_FEED_BYTES: usize = 10 * 1024 * 1024;

pub struct HttpFetcher {
    client: Client,
}

impl HttpFetcher {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .gzip(true)
            .brotli(true)
            .user_agent("rivulet/0.1.0")
            .build()
            .expect("Failed to build HTTP client");

        Self { client }
    }
}

impl Default for HttpFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Fetcher for HttpFetcher {
    async fn fetch(
        &self,
        url: &str,
        etag: Option<&str>,
        last_modified: Option<&str>,
    ) -> Result<FetchResult> {
        let url = parse_http_url(url)?;
        let mut headers = HeaderMap::new();

        if let Some(etag) = etag {
            if let Ok(value) = HeaderValue::from_str(etag) {
                headers.insert(IF_NONE_MATCH, value);
            }
        }

        if let Some(last_modified) = last_modified {
            if let Ok(value) = HeaderValue::from_str(last_modified) {
                headers.insert(IF_MODIFIED_SINCE, value);
            }
        }

        let response = self.client.get(url).headers(headers).send().await?;

        if response.status() == StatusCode::NOT_MODIFIED {
            return Ok(FetchResult::NotModified);
        }

        response.error_for_status_ref()?;

        let etag = response
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        let last_modified = response
            .headers()
            .get("last-modified")
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        if response
            .content_length()
            .is_some_and(|length| length > MAX_FEED_BYTES as u64)
        {
            return Err(RivuletError::Other(format!(
                "Feed response exceeds the {} MiB limit",
                MAX_FEED_BYTES / 1024 / 1024
            )));
        }

        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if body.len().saturating_add(chunk.len()) > MAX_FEED_BYTES {
                return Err(RivuletError::Other(format!(
                    "Feed response exceeds the {} MiB limit",
                    MAX_FEED_BYTES / 1024 / 1024
                )));
            }
            body.extend_from_slice(&chunk);
        }

        Ok(FetchResult::Content {
            body,
            etag,
            last_modified,
        })
    }
}
