use serde::Deserialize;

#[derive(Clone, Deserialize)]
pub(crate) struct Config {
    pub(crate) bot_token: String,
    pub(crate) api_url: Option<String>,
    #[serde(default, alias = "admin_users", alias = "admin_user_ids")]
    admins: Vec<AdminId>,
}

#[derive(Clone, Deserialize)]
#[serde(untagged)]
enum AdminId {
    String(String),
    Integer(i64),
}

impl Config {
    pub(crate) fn is_admin(&self, user_id: &str) -> bool {
        self.admins.iter().any(|admin| admin.matches(user_id))
    }
}

impl AdminId {
    fn matches(&self, user_id: &str) -> bool {
        match self {
            Self::String(id) => id == user_id,
            Self::Integer(id) => id.to_string() == user_id,
        }
    }
}

pub(crate) const DEFAULT_CONFIG: &str = r#"# Telegram Adapter Configuration
bot_token = "YOUR_BOT_TOKEN_HERE"
# api_url = "https://api.telegram.org"
admins = []
"#;

#[cfg(test)]
#[path = "../tests/unit/config_tests.rs"]
mod config_tests;
