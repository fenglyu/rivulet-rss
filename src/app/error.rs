use thiserror::Error;

#[derive(Error, Debug)]
pub enum RivuletError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Feed parsing error: {0}")]
    FeedParse(String),

    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Feed not found: {0}")]
    FeedNotFound(String),

    #[error("Item not found: {0}")]
    ItemNotFound(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Scraper error: {0}")]
    Scraper(String),

    #[error("{0}")]
    Other(String),
}

impl From<std::convert::Infallible> for RivuletError {
    fn from(x: std::convert::Infallible) -> Self {
        match x {}
    }
}

pub type Result<T> = std::result::Result<T, RivuletError>;
