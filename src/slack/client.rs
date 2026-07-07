use serde::Serialize;
use serde_json::{Map, Value};

use crate::config::WorkspaceSession;

use super::xparams::{Identity, XParams};

#[derive(Debug, Clone)]
pub struct SlackClientConfig {
    pub identity: Identity,
    pub xparams: XParams,
}

impl Default for SlackClientConfig {
    fn default() -> Self {
        Self {
            identity: Identity::from_capture(),
            xparams: XParams::default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SlackClient {
    config: SlackClientConfig,
}

impl SlackClient {
    pub fn new(config: SlackClientConfig) -> Self {
        Self { config }
    }

    pub fn rest_form(
        &self,
        workspace: &WorkspaceSession,
        endpoint: &str,
        fields: Vec<(&str, String)>,
    ) -> PreparedRequest {
        let mut body = vec![("token".to_owned(), workspace.token.clone())];
        body.extend(
            fields
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value)),
        );

        let url = with_query(
            &format!("{}/api/{}", rest_base(workspace), endpoint),
            self.config.xparams.rest_pairs(),
        );

        PreparedRequest {
            method: "POST",
            url,
            headers: self.desktop_headers("application/x-www-form-urlencoded"),
            body: RequestBody::Form(body),
        }
    }

    pub fn edge_json<T: Serialize>(
        &self,
        workspace: &WorkspaceSession,
        path: &str,
        body: T,
    ) -> serde_json::Result<PreparedRequest> {
        let mut object = match serde_json::to_value(body)? {
            Value::Object(object) => object,
            other => {
                let mut object = Map::new();
                object.insert("value".to_owned(), other);
                object
            }
        };

        object.insert("token".to_owned(), Value::String(workspace.token.clone()));
        if let Some(enterprise_id) = &workspace.enterprise_id {
            object.insert(
                "enterprise_token".to_owned(),
                Value::String(workspace.token.clone()),
            );
            object.insert(
                "enterprise_id".to_owned(),
                Value::String(enterprise_id.clone()),
            );
        }

        let url = with_query(
            &format!(
                "https://edgeapi.slack.com/cache/{}/{}",
                workspace
                    .enterprise_id
                    .as_deref()
                    .unwrap_or(workspace.team_id.as_str()),
                path.trim_start_matches('/')
            ),
            self.config.xparams.edge_pairs(),
        );

        Ok(PreparedRequest {
            method: "POST",
            url,
            headers: self.desktop_headers("application/json"),
            body: RequestBody::Json(Value::Object(object)),
        })
    }

    fn desktop_headers(&self, content_type: &'static str) -> Vec<(String, String)> {
        let identity = &self.config.identity;
        vec![
            (
                "sec-ch-ua-platform".to_owned(),
                identity.sec_ch_ua_platform.clone(),
            ),
            ("Referer".to_owned(), identity.referer.clone()),
            ("User-Agent".to_owned(), identity.user_agent.clone()),
            ("sec-ch-ua".to_owned(), identity.sec_ch_ua.clone()),
            ("Content-Type".to_owned(), content_type.to_owned()),
            (
                "sec-ch-ua-mobile".to_owned(),
                identity.sec_ch_ua_mobile.clone(),
            ),
        ]
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedRequest {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: RequestBody,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RequestBody {
    Form(Vec<(String, String)>),
    Json(Value),
}

fn rest_base(workspace: &WorkspaceSession) -> String {
    let url = workspace.url.trim_end_matches('/');
    if workspace.enterprise_id.is_none() {
        return url.to_owned();
    }
    let Some(host) = url.strip_prefix("https://") else {
        return url.to_owned();
    };
    if host.contains(".enterprise.") || !host.ends_with(".slack.com") {
        return url.to_owned();
    }
    host.split('.')
        .next()
        .map(|sub| format!("https://{sub}.enterprise.slack.com"))
        .unwrap_or_else(|| url.to_owned())
}

fn with_query(base: &str, pairs: Vec<(String, String)>) -> String {
    let query = pairs
        .into_iter()
        .map(|(key, value)| format!("{}={}", encode_component(&key), encode_component(&value)))
        .collect::<Vec<_>>()
        .join("&");

    format!("{base}?{query}")
}

fn encode_component(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}

pub fn redact_secrets(input: &str) -> String {
    let mut out = input.to_owned();
    for prefix in ["xoxc-", "xoxd-", "xoxp-"] {
        out = redact_prefixed(&out, prefix);
    }
    for key in ["token", "enterprise_token"] {
        out = redact_kv(&out, &format!("{key}="), |tail| {
            tail.char_indices()
                .find(|(_, c)| *c == '&' || c.is_whitespace())
                .map(|(i, _)| i)
                .unwrap_or(tail.len())
        });
        out = redact_kv(&out, &format!("\"{key}\":\""), |tail| {
            tail.find('"').unwrap_or(tail.len())
        });
    }
    out
}

fn redact_prefixed(input: &str, prefix: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find(prefix) {
        out.push_str(&rest[..start]);
        out.push_str(prefix);
        out.push_str("REDACTED");
        rest = &rest[start + prefix.len()..];
        let end = rest
            .char_indices()
            .find(|(_, c)| !c.is_ascii_alphanumeric() && *c != '-')
            .map(|(i, _)| i)
            .unwrap_or(rest.len());
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

fn redact_kv(input: &str, pattern: &str, end_at: impl Fn(&str) -> usize) -> String {
    let mut out = input.to_owned();
    let mut at = 0;
    while let Some(rel) = out[at..].find(pattern) {
        let start = at + rel;
        let value = start + pattern.len();
        let end = value + end_at(&out[value..]);
        out.replace_range(value..end, "REDACTED");
        at = value + "REDACTED".len();
    }
    out
}

impl PreparedRequest {
    pub fn retry_safe(&self) -> bool {
        self.method == "GET"
            || [
                "/api/client.userBoot?",
                "/api/client.counts?",
                "/api/conversations.history?",
                "/api/conversations.replies?",
                "/api/conversations.mark?",
                "/api/search.modules.messages?",
            ]
            .iter()
            .any(|endpoint| self.url.contains(endpoint))
    }

    pub fn redacted_debug(&self) -> String {
        let body = match &self.body {
            RequestBody::Form(fields) => fields
                .iter()
                .map(|(key, value)| {
                    if key == "token" || key == "enterprise_token" {
                        format!("{key}=REDACTED")
                    } else {
                        format!("{key}={}", redact_secrets(value))
                    }
                })
                .collect::<Vec<_>>()
                .join("&"),
            RequestBody::Json(value) => redact_secrets(&value.to_string()),
        };
        format!("{} {} body={body}", self.method, redact_secrets(&self.url),)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rest_request_matches_captured_header_shape() {
        let client = SlackClient::default();
        let workspace = workspace();
        let request = client.rest_form(
            &workspace,
            "conversations.history",
            vec![("channel", "C123".to_owned()), ("limit", "28".to_owned())],
        );

        assert_eq!(request.method, "POST");
        // Enterprise-grid workspace (enterprise_id set) routes through the
        // enterprise host, not the plain workspace host.
        assert!(
            request
                .url
                .starts_with("https://hackclub.enterprise.slack.com/api/conversations.history?"),
            "unexpected url: {}",
            request.url
        );
        assert!(request.url.contains("_x_version_ts="));
        assert_eq!(
            request
                .headers
                .iter()
                .map(|(key, _)| key.as_str())
                .collect::<Vec<_>>(),
            vec![
                "sec-ch-ua-platform",
                "Referer",
                "User-Agent",
                "sec-ch-ua",
                "Content-Type",
                "sec-ch-ua-mobile",
            ],
        );
    }

    fn workspace() -> WorkspaceSession {
        WorkspaceSession {
            team_id: "T123".to_owned(),
            enterprise_id: Some("E123".to_owned()),
            user_id: "U080A3QP42C".to_owned(),
            name: "Hack Club".to_owned(),
            url: "https://hackclub.slack.com".to_owned(),
            token: "xoxc-redacted".to_owned(),
        }
    }

    #[test]
    fn redact_secrets_hides_tokens_and_cookies() {
        let raw =
            r#"token=xoxc-abc123&enterprise_token=xoxc-ent&d=xoxd-cookie99 {"token":"xoxc-json"}"#;
        let redacted = redact_secrets(raw);
        assert!(!redacted.contains("xoxc-abc"));
        assert!(!redacted.contains("xoxc-ent"));
        assert!(!redacted.contains("xoxd-cookie"));
        assert!(!redacted.contains("xoxc-json"));
        assert!(redacted.contains("token=REDACTED"));
        assert!(redacted.contains("enterprise_token=REDACTED"));
        assert!(redacted.contains("xoxd-REDACTED"));
        assert!(redacted.contains(r#""token":"REDACTED""#));
    }

    #[test]
    fn prepared_request_redacted_debug_hides_form_token() {
        let client = SlackClient::default();
        let workspace = WorkspaceSession {
            token: "xoxc-secret-token".to_owned(),
            ..workspace()
        };
        let request = client.rest_form(
            &workspace,
            "chat.postMessage",
            vec![("channel", "C123".to_owned()), ("text", "hi".to_owned())],
        );
        let debug = request.redacted_debug();
        assert!(!debug.contains("xoxc-secret"));
        assert!(debug.contains("token=REDACTED"));
        assert!(debug.contains("channel=C123"));
        assert!(debug.contains("text=hi"));
    }

    #[test]
    fn retry_safe_only_allows_idempotent_requests() {
        let client = SlackClient::default();
        let workspace = workspace();
        let history = client.rest_form(&workspace, "conversations.history", Vec::new());
        let send = client.rest_form(&workspace, "chat.postMessage", Vec::new());

        assert!(history.retry_safe());
        assert!(!send.retry_safe());
    }
}
