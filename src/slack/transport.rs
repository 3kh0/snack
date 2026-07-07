use serde_json::Value;
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

        builder = match req.body {
            RequestBody::Form(fields) => builder.form(&fields),
            RequestBody::Json(value) => builder.json(&value),
        };

        let response = builder
            .send()
            .await
            .map_err(|e| Error::Transport(format!("send: {e}")))?;

        let status = response.status();
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
}
