use std::sync::RwLock;

use teloxide::Bot;

use crate::config::Config;

// Plugin-wide state. Each plugin is a singleton (one cdylib, one instance), so
// module-level globals mirror the framework's own `context::set_bot` pattern and
// let the stateless adapter and plugin lifecycle share data.
// `RwLock<Option<_>>` (not `OnceLock`) so `on_unload` can reset it for reloads.
static CONFIG: RwLock<Option<Config>> = RwLock::new(None);
static TG_BOT: RwLock<Option<Bot>> = RwLock::new(None);

pub(crate) fn set_config(config: Config) {
    *CONFIG.write().unwrap() = Some(config);
}

pub(crate) fn config_snapshot() -> Option<(String, Option<String>)> {
    CONFIG
        .read()
        .unwrap()
        .as_ref()
        .map(|config| (config.bot_token.clone(), config.api_url.clone()))
}

pub(crate) fn is_configured_admin(user_id: Option<&str>) -> bool {
    let Some(user_id) = user_id else {
        return false;
    };
    CONFIG
        .read()
        .unwrap()
        .as_ref()
        .is_some_and(|config| config.is_admin(user_id))
}

pub(crate) fn set_telegram_bot(bot: Bot) {
    *TG_BOT.write().unwrap() = Some(bot);
}

pub(crate) fn telegram_bot() -> Option<Bot> {
    TG_BOT.read().unwrap().as_ref().cloned()
}

pub(crate) fn reset() {
    *TG_BOT.write().unwrap() = None;
    *CONFIG.write().unwrap() = None;
}
