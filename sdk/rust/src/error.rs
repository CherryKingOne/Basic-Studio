pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("api error {status}: {message}")]
    Api { status: u16, message: String },

    #[error("invalid response: {0}")]
    Decode(String),

    #[error("stream error: {0}")]
    Stream(String),
}

impl Error {
    pub fn status(&self) -> Option<u16> {
        match self {
            Error::Api { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// 是否值得重试：网络抖动与 429/5xx 可重试。
    pub fn is_retryable(&self) -> bool {
        match self {
            Error::Http(_) | Error::Stream(_) => true,
            Error::Api { status, .. } => *status == 429 || *status >= 500,
            Error::Decode(_) => false,
        }
    }
}
