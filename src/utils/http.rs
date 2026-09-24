//! HTTP client utilities for wenget

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde::de::DeserializeOwned;
use std::sync::OnceLock;
use std::time::Duration;

/// Process-wide HTTP client: one connection pool, `wenget/<version>` User-Agent, no total timeout
///
/// Callers set per-request timeouts. It never carries credentials; only [`HttpClient`] adds
/// the GitHub token, so downloads from arbitrary asset hosts cannot leak it.
pub fn shared_client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .user_agent(format!("wenget/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("Failed to create HTTP client")
    })
}

/// HTTP client wrapper over [`shared_client`] with a per-request timeout and optional GitHub token
#[derive(Clone)]
pub struct HttpClient {
    client: &'static Client,
    timeout: Duration,
    token: Option<String>,
}

impl HttpClient {
    /// Create a new HTTP client with default timeout (30 seconds) and no authentication
    pub fn new() -> Result<Self> {
        Self::with_options(None, Duration::from_secs(30))
    }

    /// Create a new HTTP client with optional GitHub token
    pub fn with_token(token: Option<String>) -> Result<Self> {
        Self::with_options(token, Duration::from_secs(30))
    }

    /// Create a new HTTP client with custom timeout
    pub fn with_timeout(timeout: Duration) -> Result<Self> {
        Self::with_options(None, timeout)
    }

    /// Create a new HTTP client with optional token and custom timeout
    pub fn with_options(token: Option<String>, timeout: Duration) -> Result<Self> {
        Ok(Self {
            client: shared_client(),
            timeout,
            token,
        })
    }

    /// Send a GET request and return the response as text
    pub fn get_text(&self, url: &str) -> Result<String> {
        log::debug!("GET {}", url);

        let mut request = self.client.get(url).timeout(self.timeout);

        // Add authorization header if token is available
        if let Some(ref token) = self.token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        let response = request
            .send()
            .with_context(|| format!("Failed to send GET request to {}", url))?;

        if !response.status().is_success() {
            return Err(status_error(&response, url));
        }

        let text = response
            .text()
            .context("Failed to read response body as text")?;

        Ok(text)
    }

    /// Send a GET request and parse JSON response
    pub fn get_json<T: DeserializeOwned>(&self, url: &str) -> Result<T> {
        log::debug!("GET {} (JSON)", url);

        let mut request = self
            .client
            .get(url)
            .timeout(self.timeout)
            .header("Accept", "application/json");

        // Add authorization header if token is available
        if let Some(ref token) = self.token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        let response = request
            .send()
            .with_context(|| format!("Failed to send GET request to {}", url))?;

        if !response.status().is_success() {
            return Err(status_error(&response, url));
        }

        let data = response
            .json::<T>()
            .context("Failed to parse JSON response")?;

        Ok(data)
    }
}

/// Build the error for a non-success response, naming an exhausted GitHub rate limit
fn status_error(response: &reqwest::blocking::Response, url: &str) -> anyhow::Error {
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    describe_status(
        response.status().as_u16(),
        header("x-ratelimit-remaining").as_deref(),
        header("x-ratelimit-reset").as_deref(),
        url,
    )
}

/// Message for a failed status; a 403/429 with no requests remaining is a rate limit
fn describe_status(
    status: u16,
    remaining: Option<&str>,
    reset: Option<&str>,
    url: &str,
) -> anyhow::Error {
    if matches!(status, 403 | 429) && remaining == Some("0") {
        let resets = reset
            .and_then(|r| r.parse::<i64>().ok())
            .and_then(|r| chrono::DateTime::from_timestamp(r, 0))
            .map(|t| {
                format!(
                    " (resets at {})",
                    t.with_timezone(&chrono::Local).format("%H:%M")
                )
            })
            .unwrap_or_default();
        return anyhow::anyhow!("GitHub API rate limit exceeded{} for {}", resets, url);
    }
    anyhow::anyhow!(
        "HTTP {} for {}",
        reqwest::StatusCode::from_u16(status)
            .map(|s| s.to_string())
            .unwrap_or_else(|_| status.to_string()),
        url
    )
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new().expect("Failed to create HTTP client")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_request_timeout_fires_on_silent_server() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let client = HttpClient::with_timeout(Duration::from_millis(300)).unwrap();
        let start = std::time::Instant::now();
        assert!(client.get_text(&url).is_err());
        assert!(start.elapsed() < Duration::from_secs(5));
        drop(listener);
    }

    #[test]
    fn test_http_client_creation() {
        let client = HttpClient::new();
        assert!(client.is_ok());
    }

    #[test]
    fn test_describe_status_rate_limited() {
        let msg = describe_status(403, Some("0"), Some("1790154705"), "https://x").to_string();
        assert!(
            msg.starts_with("GitHub API rate limit exceeded (resets at "),
            "{msg}"
        );
        assert!(msg.ends_with("for https://x"), "{msg}");
    }

    #[test]
    fn test_describe_status_plain_errors() {
        // 403 with requests remaining is a real permission error, not a rate limit
        let msg = describe_status(403, Some("12"), None, "https://x").to_string();
        assert_eq!(msg, "HTTP 403 Forbidden for https://x");
        let msg = describe_status(404, None, None, "https://x").to_string();
        assert_eq!(msg, "HTTP 404 Not Found for https://x");
    }
}
