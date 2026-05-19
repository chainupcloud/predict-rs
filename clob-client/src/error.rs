use reqwest::StatusCode;
use thiserror::Error;

/// Crate-wide result alias.
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("validation: {0}")]
    Validation(String),

    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("url parse: {0}")]
    Url(#[from] url::ParseError),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("api error: status={status} method={method} path={path}: {message}")]
    Api {
        status: StatusCode,
        method: String,
        path: String,
        message: String,
    },

    #[error("not authenticated: this call requires L2 credentials (API key + secret + passphrase)")]
    NotAuthenticated,

    #[error("signer: {0}")]
    Signer(String),

    #[error("eip712: {0}")]
    Eip712(String),

    #[error("hex decode: {0}")]
    Hex(#[from] hex::FromHexError),

    #[error("base64 decode: {0}")]
    Base64(#[from] base64::DecodeError),

    #[error("invalid header value: {0}")]
    InvalidHeader(#[from] reqwest::header::InvalidHeaderValue),
}

impl Error {
    pub fn validation(msg: impl Into<String>) -> Self {
        Self::Validation(msg.into())
    }

    pub fn signer(msg: impl Into<String>) -> Self {
        Self::Signer(msg.into())
    }

    pub fn api(
        status: StatusCode,
        method: impl Into<String>,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::Api {
            status,
            method: method.into(),
            path: path.into(),
            message: message.into(),
        }
    }
}
