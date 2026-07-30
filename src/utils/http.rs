//! HTTP client utilities for wenget

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde::de::DeserializeOwned;
use std::time::Duration;

/// HTTP client wrapper
#[derive(Clone)]
pub struct HttpClient {
    client: Client,
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
        let client = Client::builder()
            .user_agent(format!("wenget/{}", env!("CARGO_PKG_VERSION")))
            .timeout(timeout)
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self { client, token })
    }

    /// Send a GET request and return the response as text
    pub fn get_text(&self, url: &str) -> Result<String> {
        log::debug!("GET {}", url);

        let mut request = self.client.get(url);

        // Add authorization header if token is available
        if let Some(ref token) = self.token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        let response = request
            .send()
            .with_context(|| format!("Failed to send GET request to {}", url))?;

        if !response.status().is_success() {
            anyhow::bail!("HTTP {} for {}", response.status(), url);
        }

        let text = response
            .text()
            .context("Failed to read response body as text")?;

        Ok(text)
    }

    /// Send a GET request and parse JSON response
    pub fn get_json<T: DeserializeOwned>(&self, url: &str) -> Result<T> {
        log::debug!("GET {} (JSON)", url);

        let mut request = self.client.get(url).header("Accept", "application/json");

        // Add authorization header if token is available
        if let Some(ref token) = self.token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        let response = request
            .send()
            .with_context(|| format!("Failed to send GET request to {}", url))?;

        if !response.status().is_success() {
            anyhow::bail!("HTTP {} for {}", response.status(), url);
        }

        let data = response
            .json::<T>()
            .context("Failed to parse JSON response")?;

        Ok(data)
    }

    /// Check GitHub API rate limit
    #[allow(dead_code)]
    pub fn check_rate_limit(&self) -> Result<RateLimit> {
        let data: serde_json::Value = self
            .get_json("https://api.github.com/rate_limit")
            .context("Failed to check rate limit")?;

        let core = &data["rate"];
        let limit = core["limit"].as_u64().unwrap_or(0);
        let remaining = core["remaining"].as_u64().unwrap_or(0);
        let reset = core["reset"].as_u64().unwrap_or(0);

        Ok(RateLimit {
            limit,
            remaining,
            reset,
        })
    }
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new().expect("Failed to create HTTP client")
    }
}

/// GitHub API rate limit information
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct RateLimit {
    pub limit: u64,
    pub remaining: u64,
    pub reset: u64,
}

impl RateLimit {
    /// Check if we're close to the rate limit
    #[allow(dead_code)]
    pub fn is_low(&self) -> bool {
        self.remaining < 10
    }

    /// Get a warning message if rate limit is low
    #[allow(dead_code)]
    pub fn warning_message(&self) -> Option<String> {
        if self.is_low() {
            Some(format!(
                "⚠ GitHub API rate limit low: {}/{} remaining",
                self.remaining, self.limit
            ))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_client_creation() {
        let client = HttpClient::new();
        assert!(client.is_ok());
    }

    #[test]
    #[ignore] // Requires network access
    fn test_rate_limit_check() {
        let client = HttpClient::new().unwrap();
        let rate_limit = client.check_rate_limit();
        assert!(rate_limit.is_ok());
    }
}
