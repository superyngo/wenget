//! HTTP client utilities for wenget

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde::de::DeserializeOwned;
use std::sync::OnceLock;
use std::time::Duration;

/// Process-wide HTTP client: one connection pool, `wenget/<version>` User-Agent
///
/// The blocking client's 30 s timeout bounds waiting for response headers and each individual
/// body `read()`, not the whole transfer, so it works as a stall timeout for large downloads.
/// Never set `RequestBuilder::timeout` on a download: in blocking reqwest that becomes a total
/// deadline over the entire body. It never carries credentials; only [`HttpClient`] adds the
/// GitHub token, so downloads from arbitrary asset hosts cannot leak it.
pub fn shared_client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .user_agent(format!("wenget/{}", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(30))
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

    /// The token, only for GitHub hosts or loopback fixtures, so a redirected
    /// `WENGET_GITHUB_API` or a bucket source URL cannot receive it
    fn token_for(&self, url: &str) -> Option<&str> {
        let token = self.token.as_deref()?;
        token_allowed(url).then_some(token)
    }

    /// GET `url`, retrying a 5xx or a non-rate-limit 429 up to [`MAX_STATUS_RETRIES`] times
    ///
    /// CDN edges (e.g. `raw.githubusercontent.com`) occasionally answer 503 for a moment; one
    /// such answer must not fail a bucket refresh or an update check.
    fn get_ok(&self, url: &str, accept: Option<&str>) -> Result<reqwest::blocking::Response> {
        let mut attempt = 0;
        loop {
            let mut request = self.client.get(url).timeout(self.timeout);
            if let Some(accept) = accept {
                request = request.header("Accept", accept);
            }
            // Add authorization header if token is available
            if let Some(token) = self.token_for(url) {
                request = request.header("Authorization", format!("Bearer {}", token));
            }

            let response = request
                .send()
                .with_context(|| format!("Failed to send GET request to {}", url))?;
            if response.status().is_success() {
                return Ok(response);
            }
            let err = status_error(&response, url);
            if attempt >= MAX_STATUS_RETRIES || !is_transient(&response) {
                return Err(err);
            }
            attempt += 1;
            log::warn!("{err}; retrying ({attempt}/{MAX_STATUS_RETRIES})");
            std::thread::sleep(Duration::from_millis(RETRY_BASE_MS << (attempt - 1)));
        }
    }

    /// Send a GET request and return the response as text
    pub fn get_text(&self, url: &str) -> Result<String> {
        log::debug!("GET {}", url);
        let response = self.get_ok(url, None)?;

        let text = response
            .text()
            .context("Failed to read response body as text")?;

        Ok(text)
    }

    /// Send a GET request and parse JSON response
    pub fn get_json<T: DeserializeOwned>(&self, url: &str) -> Result<T> {
        log::debug!("GET {} (JSON)", url);
        let response = self.get_ok(url, Some("application/json"))?;

        let data = response
            .json::<T>()
            .context("Failed to parse JSON response")?;

        Ok(data)
    }
}

/// Whether `url` may carry the GitHub token
fn token_allowed(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    matches!(
        host,
        "api.github.com" | "github.com" | "raw.githubusercontent.com" | "localhost"
    ) || host
        .parse::<std::net::IpAddr>()
        .is_ok_and(|ip| ip.is_loopback())
}

/// Retries after a transient status (5xx, or 429 with requests remaining)
const MAX_STATUS_RETRIES: u32 = 2;

/// First retry delay, doubled per retry (1 s, 2 s)
const RETRY_BASE_MS: u64 = if cfg!(test) { 5 } else { 1000 };

/// Whether a failed status is worth retrying; an exhausted rate limit is not
fn is_transient(response: &reqwest::blocking::Response) -> bool {
    let status = response.status();
    let exhausted = response
        .headers()
        .get("x-ratelimit-remaining")
        .is_some_and(|v| v.as_bytes() == b"0");
    status.is_server_error() || (status == reqwest::StatusCode::TOO_MANY_REQUESTS && !exhausted)
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
    #[test]
    fn test_token_only_for_github_and_loopback() {
        assert!(token_allowed("https://api.github.com/repos/a/b"));
        assert!(token_allowed("http://127.0.0.1:9/x"));
        assert!(token_allowed("http://[::1]:9/x"));
        assert!(!token_allowed("https://evil.example/x"));
        assert!(!token_allowed("https://api.github.com.evil.example/x"));
        assert!(!token_allowed("not a url"));
    }

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

    /// Serve one canned HTTP response per entry, in order, on a loopback port
    fn serve(responses: Vec<&'static str>) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for body in responses {
                let (mut conn, _) = listener.accept().unwrap();
                let mut buf = [0u8; 4096];
                let _ = conn.read(&mut buf);
                let _ = conn.write_all(body.as_bytes());
            }
        });
        url
    }

    const R503: &str =
        "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    const R200: &str = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";
    const R404: &str = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

    #[test]
    fn get_text_retries_transient_503() {
        let url = serve(vec![R503, R503, R200]);
        assert_eq!(HttpClient::new().unwrap().get_text(&url).unwrap(), "ok");
    }

    #[test]
    fn get_text_gives_up_after_retries() {
        let url = serve(vec![R503, R503, R503]);
        let err = HttpClient::new().unwrap().get_text(&url).unwrap_err();
        assert!(err.to_string().contains("503"), "{err}");
    }

    #[test]
    fn get_text_does_not_retry_404() {
        // A retry would block on accept() of a server that serves one response
        let url = serve(vec![R404]);
        let err = HttpClient::new().unwrap().get_text(&url).unwrap_err();
        assert!(err.to_string().contains("404"), "{err}");
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
