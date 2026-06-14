use url::Url;

use crate::app::{Result, RivuletError};

/// Parse a URL and allow only network schemes supported by Rivulet.
pub fn parse_http_url(input: &str) -> Result<Url> {
    let url = Url::parse(input).map_err(|error| {
        RivuletError::InvalidUrl(format!("{input:?} could not be parsed: {error}"))
    })?;

    match url.scheme() {
        "http" | "https" => Ok(url),
        scheme => Err(RivuletError::InvalidUrl(format!(
            "{input:?} uses unsupported scheme {scheme:?}; only http and https are allowed"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_http_and_https() {
        assert!(parse_http_url("http://example.com/feed.xml").is_ok());
        assert!(parse_http_url("https://example.com/feed.xml").is_ok());
    }

    #[test]
    fn rejects_non_http_schemes() {
        assert!(parse_http_url("file:///etc/passwd").is_err());
        assert!(parse_http_url("javascript:alert(1)").is_err());
        assert!(parse_http_url("data:text/plain,hello").is_err());
    }
}
