use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Identity {
    pub user_agent: String,
    pub sec_ch_ua: String,
    pub sec_ch_ua_mobile: String,
    pub sec_ch_ua_platform: String,
    pub referer: String,
}

impl Identity {
    pub fn from_capture() -> Self {
        Self {
            user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 26_6_0) AppleWebKit/537.36 (KHTML, like Gecko) Slack/4.50.143 Chrome/148.0.7778.265 Electron/42.4.1 Safari/537.36 AppleSilicon Sonic Slack_SSB/4.50.143".to_owned(),
            sec_ch_ua: r#""Chromium";v="148", "Slack";v="4", "Not=A?Brand";v="99""#.to_owned(),
            sec_ch_ua_mobile: "?0".to_owned(),
            sec_ch_ua_platform: r#""macOS""#.to_owned(),
            referer: "https://app.slack.com/client/".to_owned(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct XParams {
    pub version_ts: String,
    pub gantry: String,
    pub frontend_build_type: String,
}

impl Default for XParams {
    fn default() -> Self {
        Self {
            version_ts: "capture-required".to_owned(),
            gantry: "true".to_owned(),
            frontend_build_type: "current".to_owned(),
        }
    }
}

impl XParams {
    pub fn rest_pairs(&self) -> Vec<(String, String)> {
        vec![
            ("_x_id".to_owned(), Uuid::new_v4().simple().to_string()),
            ("_x_csid".to_owned(), Uuid::new_v4().simple().to_string()),
            ("_x_desktop_ia".to_owned(), "true".to_owned()),
            ("_x_foreground".to_owned(), "true".to_owned()),
            (
                "_x_frontend_build_type".to_owned(),
                self.frontend_build_type.clone(),
            ),
            ("_x_gantry".to_owned(), self.gantry.clone()),
            ("_x_num_retries".to_owned(), "0".to_owned()),
            ("_x_version_ts".to_owned(), self.version_ts.clone()),
            ("fp".to_owned(), Uuid::new_v4().simple().to_string()),
            ("slack_route".to_owned(), "default".to_owned()),
        ]
    }

    pub fn edge_pairs(&self) -> Vec<(String, String)> {
        vec![
            ("_x_app_name".to_owned(), "client".to_owned()),
            ("_x_b3_sampled".to_owned(), "0".to_owned()),
            ("_x_b3_spanid".to_owned(), trace_id(16)),
            ("_x_b3_traceid".to_owned(), trace_id(32)),
            ("_x_num_retries".to_owned(), "0".to_owned()),
            ("fp".to_owned(), Uuid::new_v4().simple().to_string()),
        ]
    }
}

fn trace_id(len: usize) -> String {
    Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(len)
        .collect()
}
