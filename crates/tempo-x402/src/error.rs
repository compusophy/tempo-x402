//! Error types for x402 payment operations. (updated)
//!
//! [`X402Error`] covers signature failures, chain interaction errors,
//! invalid payments, unsupported schemes, configuration issues, and HTTP errors.

use thiserror::Error;

/// Errors returned by x402 operations.
#[derive(Debug, Error)]
pub enum X402Error {
    #[error("signature error: {0}")]
    SignatureError(String),

    #[error("chain error: {0}")]
    ChainError(String),

    #[error("invalid payment: {0}")]
    InvalidPayment(String),

    #[error("unsupported scheme: {0}")]
    UnsupportedScheme(String),

    #[error("config error: {0}")]
    ConfigError(String),

    #[error("http error: {0}")]
    HttpError(String),

    #[error("serialization error: {0}")]
    SerdeError(#[from] serde_json::Error),
}
