use std::sync::Arc;
use std::time::Duration;

use snb_core::context::BotContext;
use teloxide::prelude::*;
use teloxide::types::Update;

use crate::convert::convert_update;
use crate::state;

pub(crate) async fn run(bot: Arc<dyn BotContext>) {
    let (token, api_url) = match state::config_snapshot() {
        Some(config) => config,
        None => {
            log::error!("bot_token not configured, adapter not starting");
            return;
        }
    };

    let mut tg_bot = Bot::new(token);
    if let Some(api_url) = api_url.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        match url::Url::parse(api_url) {
            Ok(url) => {
                log::info!("using custom Telegram API URL: {api_url}");
                tg_bot = tg_bot.set_api_url(url);
            }
            Err(e) => {
                log::error!("invalid api_url {api_url:?}: {e}; falling back to default");
            }
        }
    }
    state::set_telegram_bot(tg_bot.clone());
    crate::commands::sync_commands();

    log::info!("start Telegram dispatcher");

    // Reconnect loop. teloxide's dispatcher panics when it cannot reach the
    // Telegram API while preparing, so each attempt runs in a spawned task and
    // failed startup attempts become retryable `JoinError`s.
    let mut backoff = Duration::from_secs(1);
    const MAX_BACKOFF: Duration = Duration::from_secs(60);
    loop {
        let tg_bot = tg_bot.clone();
        let bot_ctx = bot.clone();
        let attempt = tokio::spawn(async move {
            let handler = |update: Update, bot_ctx: Arc<dyn BotContext>| async move {
                if let Some(event) = convert_update(&update) {
                    bot_ctx.emit_event(event);
                }
                respond(())
            };

            let mut dispatcher =
                Dispatcher::builder(tg_bot, dptree::entry().branch(dptree::endpoint(handler)))
                    .dependencies(dptree::deps![bot_ctx])
                    .build();

            dispatcher.dispatch().await;
        });

        match attempt.await {
            // Dispatcher returned on its own: a clean shutdown, stop retrying.
            Ok(()) => break,
            Err(e) if e.is_panic() => {
                log::error!(
                    "Telegram dispatcher crashed (network unreachable?); reconnecting in {}s",
                    backoff.as_secs()
                );
            }
            // Task cancelled (e.g. runtime shutting down): don't spin, just stop.
            Err(e) => {
                log::error!("Telegram dispatcher task aborted: {e}");
                break;
            }
        }

        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}
