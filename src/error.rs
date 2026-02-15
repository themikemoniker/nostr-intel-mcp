use thiserror::Error;

#[derive(Error, Debug)]
pub enum NostrIntelError {
    #[error("Nostr SDK error: {0}")]
    NostrSdk(String),

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("NWC error: {0}")]
    Nwc(String),

    #[error("Payment error: {0}")]
    Payment(String),

    #[error("L402 error: {0}")]
    L402(String),

    #[error("Relay connection error: {0}")]
    RelayConnection(String),
}

impl From<NostrIntelError> for String {
    fn from(err: NostrIntelError) -> String {
        err.to_string()
    }
}
