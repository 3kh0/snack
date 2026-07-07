use std::time::Duration;

use serde_json::Value;
use wreq::header::HeaderMap;
use wreq_util::Emulation;

use super::Error;
use super::client::{PreparedRequest, RequestBody, redact_secrets};

#[derive(Clone)]
pub struct Transport {
    http: wreq::Client,
    d_cookie: String,
}

impl std::fmt::Debug for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transport").finish_non_exhaustive()
    }
}

impl Transport {
    pub fn new(d_cookie: impl Into<String>) -> Result<Self, Error> {
        let http = wreq::Client::builder()
            .emulation(Emulation::Chrome140)
            .build()
            .map_err(|e| Error::Transport(format!("client build: {e}")))?;
        Ok(Self {
            http,
            d_cookie: d_cookie.into(),
        })
    }

    pub fn http(&self) -> &wreq::Client {
        &self.http
    }

    pub fn d_cookie(&self) -> &str {
        &self.d_cookie
    }

    fn cookie(&self) -> String {
        format!("d={}", self.d_cookie)
    }

    pub async fn execute(&self, req: PreparedRequest) -> Result<Value, Error> {
        let mut attempt = 0;
        loop {
            match self.execute_once(&req).await {
                Ok(value) => return Ok(value),
                Err(e) if req.retry_safe() && retryable_error(&e) && attempt < 2 => {
                    attempt += 1;
                    tokio::time::sleep(retry_delay(&e, attempt)).await;
                }
                Err(e) => return Err(e),
            }
        }
    }

    async fn execute_once(&self, req: &PreparedRequest) -> Result<Value, Error> {
        let req_url = req.url.clone();
        let mut builder = match req.method {
            "POST" => self.http.post(&req.url),
            "GET" => self.http.get(&req.url),
            other => return Err(Error::Transport(format!("unsupported method {other}"))),
        };

        for (key, value) in &req.headers {
            if key.eq_ignore_ascii_case("content-type") {
                continue;
            }
            builder = builder.header(key.as_str(), value.as_str());
        }
        builder = builder.header("Cookie", self.cookie());

        builder = match &req.body {
            RequestBody::Form(fields) => builder.form(fields),
            RequestBody::Json(value) => builder.json(value),
        };

        let response = builder
            .send()
            .await
            .map_err(|e| Error::Transport(format!("send: {e}")))?;

        let status = response.status();
        let retry_after_secs = retry_after_secs(response.headers());
        if status.as_u16() == 429 {
            return Err(Error::RateLimited { retry_after_secs });
        }
        if !status.is_success() {
            tracing::warn!(
                url = %redact_secrets(&req_url),
                status = %status,
                retry_after_secs = ?retry_after_secs,
                "slack http error"
            );
            return Err(Error::HttpStatus {
                status: status.as_u16(),
                retry_after_secs,
            });
        }

        let value: Value = response
            .json()
            .await
            .map_err(|e| Error::Transport(format!("decode ({status}): {e}")))?;

        if value.get("ok").and_then(Value::as_bool) == Some(false) {
            let err = value
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown_error")
                .to_owned();
            tracing::warn!(
                url = %redact_secrets(&req_url),
                status = %status,
                body = %redact_secrets(&value.to_string()),
                "slack api error"
            );
            return Err(Error::Api(err));
        }

        Ok(value)
    }

    pub async fn get_text(&self, url: &str, user_agent: &str) -> Result<String, Error> {
        let response = self
            .http
            .get(url)
            .header("User-Agent", user_agent)
            .header("Cookie", self.cookie())
            .send()
            .await
            .map_err(|e| Error::Transport(format!("send: {e}")))?;
        response
            .text()
            .await
            .map_err(|e| Error::Transport(format!("read body: {e}")))
    }

    pub async fn get_bytes(&self, url: &str, user_agent: &str) -> Result<Vec<u8>, Error> {
        let response = self
            .http
            .get(url)
            .header("User-Agent", user_agent)
            .header("Cookie", self.cookie())
            .send()
            .await
            .map_err(|e| Error::Transport(format!("send: {e}")))?;

        let status = response.status();
        let retry_after_secs = retry_after_secs(response.headers());
        if status.as_u16() == 429 {
            return Err(Error::RateLimited { retry_after_secs });
        }
        if !status.is_success() {
            tracing::warn!(
                url = %redact_secrets(url),
                status = %status,
                retry_after_secs = ?retry_after_secs,
                "slack file http error"
            );
            return Err(Error::HttpStatus {
                status: status.as_u16(),
                retry_after_secs,
            });
        }

        response
            .bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .map_err(|e| Error::Transport(format!("read body: {e}")))
    }
}

pub fn retry_after_secs(headers: &HeaderMap) -> Option<u64> {
    headers
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
}

fn retryable_error(error: &Error) -> bool {
    matches!(error, Error::RateLimited { .. })
        || matches!(
            error,
            Error::HttpStatus {
                status: 500..=599,
                ..
            }
        )
}

fn retry_delay(error: &Error, attempt: u32) -> Duration {
    let fallback_ms = 250 * 2_u64.saturating_pow(attempt.saturating_sub(1));
    let secs = match error {
        Error::RateLimited {
            retry_after_secs: Some(secs),
        }
        | Error::HttpStatus {
            retry_after_secs: Some(secs),
            ..
        } => Some((*secs).min(30)),
        _ => None,
    };
    secs.map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_millis(fallback_ms))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wreq::header::HeaderValue;

    #[test]
    fn parses_retry_after_seconds() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", HeaderValue::from_static("12"));
        assert_eq!(retry_after_secs(&headers), Some(12));
    }

    #[test]
    fn ignores_invalid_retry_after() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", HeaderValue::from_static("soon"));
        assert_eq!(retry_after_secs(&headers), None);
    }

    #[test]
    fn retry_policy_is_bounded() {
        assert!(retryable_error(&Error::RateLimited {
            retry_after_secs: Some(60)
        }));
        assert!(
            retry_delay(
                &Error::RateLimited {
                    retry_after_secs: Some(60)
                },
                1
            ) <= Duration::from_secs(30)
        );
        assert!(retryable_error(&Error::HttpStatus {
            status: 503,
            retry_after_secs: None
        }));
        assert!(!retryable_error(&Error::HttpStatus {
            status: 404,
            retry_after_secs: None
        }));
    }
}
