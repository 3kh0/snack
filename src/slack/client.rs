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
            &format!("{}/api/{}", workspace.url.trim_end_matches('/'), endpoint),
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
        assert!(
            request
                .url
                .starts_with("https://hackclub.slack.com/api/conversations.history?")
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
}
