use std::time::Duration;
use tracing::{warn, info};

// Retry utilities for the soul and other crates.

/// Generic retry configuration.
#[derive(Debug, Clone, Copy)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub base_delay: Duration,
}

/// Result of a retryable operation, allows the operation to specify a custom delay.
pub enum RetryResult<T, E> {
    Success(T),
    RetryableError { error: E, next_delay: Option<Duration> },
    FatalError(E),
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(1000),
        }
    }
}

/// Executes a future with retries based on the provided configuration and retry criteria.
pub async fn with_retry<F, Fut, T, E, R>(
    config: RetryConfig,
    f: F,
    is_retryable: R,
) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    R: Fn(&E) -> bool,
    E: std::fmt::Debug,
{
    let mut last_err = None;
    for attempt in 0..config.max_attempts {
        if attempt > 0 {
            let delay = compute_delay(attempt, config.base_delay);
            tokio::time::sleep(delay).await;
        }

        match f().await {
            Ok(val) => return Ok(val),
            Err(e) => {
                if !is_retryable(&e) {
                    return Err(e);
                }
                warn!(
                    attempt = attempt + 1,
                    max_attempts = config.max_attempts,
                    error = ?e,
                    "Retryable error occurred"
                );
                last_err = Some(e);
            }
        }
    }
    Err(last_err.expect("max_attempts must be > 0"))
}

fn compute_delay(attempt: u32, base_delay: Duration) -> Duration {
    // attempt is 0-indexed, but we only call this for attempt > 0.
    // attempt 1 -> factor 1
    // attempt 2 -> factor 2
    // attempt 3 -> factor 4
    let factor = 2u64.pow(attempt as u32 - 1);
    let base_ms = base_delay.as_millis() as u64 * factor;
    
    // Jitter: ±25%
    let jitter_range = base_ms / 4;
    if jitter_range == 0 {
        return Duration::from_millis(base_ms);
    }
    
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    
    // Use nanos for some pseudo-randomness
    let jitter = (now.subsec_nanos() as u64 % (2 * jitter_range + 1)) as i64 - jitter_range as i64;
    Duration::from_millis((base_ms as i64 + jitter).max(100) as u64)
}

/// Helper for reqwest errors.
pub fn is_retryable_reqwest_error(err: &reqwest::Error) -> bool {
    if err.is_timeout() || err.is_connect() {
        return true;
    }
    if let Some(status) = err.status() {
        return matches!(status.as_u16(), 429 | 500 | 502 | 503 | 504);
    }
    false
}

/// Parse the `Retry-After` header value (seconds) from a response.
pub fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    headers
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_secs)
}

/// Executes a future with retries, supporting custom delays (like Retry-After).
pub async fn with_retry_robust<F, Fut, T, E>(
    config: RetryConfig,
    f: F,
) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = RetryResult<T, E>>,
    E: std::fmt::Debug,
{
    let mut last_err = None;
    for attempt in 0..config.max_attempts {
        if attempt > 0 {
            // This case is handled inside the loop for custom delays
        }

        match f().await {
            RetryResult::Success(val) => return Ok(val),
            RetryResult::FatalError(e) => return Err(e),
            RetryResult::RetryableError { error, next_delay } => {
                warn!(
                    attempt = attempt + 1,
                    max_attempts = config.max_attempts,
                    error = ?error,
                    "Retryable error occurred"
                );
                last_err = Some(error);

                if attempt + 1 < config.max_attempts {
                    let delay = next_delay.unwrap_or_else(|| compute_delay(attempt + 1, config.base_delay));
                    info!(delay_ms = delay.as_millis() as u64, "Sleeping before retry");
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }
    Err(last_err.expect("max_attempts must be > 0"))
}
