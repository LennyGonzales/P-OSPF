// Gestion des erreurs personnalisées

use std::fmt;
use std::error::Error as StdError;

#[derive(Debug)]
pub enum AppError {
    NetworkError(String),
    ConfigError(String),
    IOError(std::io::Error),
    SerializationError(serde_json::Error),
    RouteError(String),
    CryptoError(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::NetworkError(msg) => write!(f, "Network error: {}", msg),
            AppError::ConfigError(msg) => write!(f, "Configuration error: {}", msg),
            AppError::IOError(err) => write!(f, "IO error: {}", err),
            AppError::SerializationError(err) => write!(f, "Serialization error: {}", err),
            AppError::RouteError(msg) => write!(f, "Route error: {}", msg),
            AppError::CryptoError(msg) => write!(f, "Crypto error: {}", msg),
        }
    }
}

impl StdError for AppError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            AppError::IOError(err) => Some(err),
            AppError::SerializationError(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::IOError(err)
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError::SerializationError(err)
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
