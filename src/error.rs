use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Liquid error: {0}")]
    Liquid(#[from] liquid::Error),

    #[error("Serenity error: {0}")]
    Serenity(#[from] serenity::Error),

    #[error("Serenity error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("Reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("{0}")]
    Custom(String),
}

impl From<String> for Error {
    fn from(s: String) -> Self {
        Self::Custom(s)
    }
}

impl From<&str> for Error {
    fn from(s: &str) -> Self {
        Self::Custom(s.to_owned())
    }
}
