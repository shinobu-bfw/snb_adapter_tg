use std::future::Future;
use std::time::Duration;

use anyhow::Context as _;
use base64::{Engine as _, engine::general_purpose};
use snb_core::context;
use snb_core::event::{
    ContentItem, Event, EventType, FileSource, ImageSource, Message, TextFormat,
};
use teloxide::prelude::*;
use teloxide::types::{
    ChatId, FileId, InputFile, InputMedia, InputMediaDocument, MessageId, ParseMode,
    ReplyParameters,
};

use crate::state;

pub(crate) fn send_event(event: &Event) -> anyhow::Result<()> {
    let Some(msg) = &event.message else {
        return Ok(());
    };
    let Some(chat_id) = msg.to.as_deref() else {
        anyhow::bail!("TGAdapter outgoing message is missing message.to chat id");
    };
    let chat_id = chat_id
        .parse::<i64>()
        .with_context(|| format!("invalid Telegram chat id: {chat_id}"))?;
    let bot = state::telegram_bot().context("TGAdapter bot not initialized")?;
    let event_type = event.event_type.clone();
    let origin = event.source.clone();
    let msg = msg.clone();

    spawn_send_task(async move {
        match event_type {
            EventType::Message => send_message_items(bot, chat_id, msg, origin).await,
            EventType::MessageEdit => edit_message_item(bot, chat_id, msg).await,
            EventType::MessageDelete => delete_message_item(bot, chat_id, msg).await,
            kind => log::debug!("TGAdapter ignored outgoing event type: {kind:?}"),
        }
    });

    Ok(())
}

fn spawn_send_task<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(future);
    } else {
        std::thread::spawn(move || snb_core::adapter::run_async(future));
    }
}

async fn send_message_items(bot: Bot, chat_id: i64, msg: Message, origin: String) {
    let local_id = msg.id.clone();
    let reply_to = msg.reply_to;
    let delete_after = msg.delete_after;
    let mut text = OutgoingText::default();
    let mut attachments = Vec::new();

    for item in msg.content {
        match item {
            ContentItem::Text {
                text: item_text,
                format,
            } => text.push_text(item_text, format),
            ContentItem::File { .. } | ContentItem::Image { .. } => attachments.push(item),
            ContentItem::Other { kind, .. } => {
                log::debug!("TGAdapter ignored unsupported outgoing content kind: {kind}");
            }
        }
    }

    let caption = text.into_payload();
    let mut sent_ids = Vec::new();
    if attachments.is_empty() {
        if let Some(text) = caption {
            sent_ids
                .extend(send_text(&bot, chat_id, text, reply_to.as_deref(), delete_after).await);
        }
        emit_message_sent_if_needed(
            &origin,
            chat_id,
            local_id.as_deref(),
            sent_ids.first().copied(),
        );
        return;
    }

    if attachments.len() > 1
        && attachments
            .iter()
            .all(|item| matches!(item, ContentItem::File { .. }))
    {
        sent_ids.extend(
            send_file_media_groups(
                &bot,
                chat_id,
                attachments,
                caption,
                reply_to.as_deref(),
                delete_after,
            )
            .await,
        );
        emit_message_sent_if_needed(
            &origin,
            chat_id,
            local_id.as_deref(),
            sent_ids.first().copied(),
        );
        return;
    }

    let mut first_caption = caption;
    for item in attachments {
        match item {
            ContentItem::File {
                source,
                file_name,
                file_id,
            } => {
                sent_ids.extend(
                    send_file(
                        &bot,
                        chat_id,
                        source,
                        file_name,
                        file_id,
                        first_caption.take(),
                        reply_to.as_deref(),
                        delete_after,
                    )
                    .await,
                );
            }
            ContentItem::Image {
                source,
                file_id,
                caption,
            } => {
                let caption = merge_captions(first_caption.take(), caption);
                sent_ids.extend(
                    send_image(
                        &bot,
                        chat_id,
                        source,
                        file_id,
                        caption,
                        reply_to.as_deref(),
                        delete_after,
                    )
                    .await,
                );
            }
            ContentItem::Text { .. } | ContentItem::Other { .. } => unreachable!(),
        }
    }

    emit_message_sent_if_needed(
        &origin,
        chat_id,
        local_id.as_deref(),
        sent_ids.first().copied(),
    );
}

#[derive(Default)]
struct OutgoingText {
    text: String,
    format: Option<TextFormat>,
    has_plain: bool,
    mixed_formats: bool,
}

#[derive(Clone)]
struct FormattedPayload {
    text: String,
    format: Option<TextFormat>,
}

impl OutgoingText {
    fn push_text(&mut self, text: String, format: Option<TextFormat>) {
        self.push(text, format);
    }

    fn push(&mut self, text: String, format: Option<TextFormat>) {
        if text.is_empty() {
            return;
        }

        match format {
            Some(format) if !self.has_plain => {
                if let Some(existing) = self.format {
                    self.mixed_formats |= existing != format;
                } else {
                    self.format = Some(format);
                }
            }
            Some(_) => {
                self.mixed_formats = true;
            }
            None => {
                if self.format.is_some() {
                    self.mixed_formats = true;
                }
                self.has_plain = true;
            }
        }

        self.text.push_str(&text);
    }

    fn into_payload(self) -> Option<FormattedPayload> {
        (!self.text.is_empty()).then_some(FormattedPayload {
            text: self.text,
            format: (!self.mixed_formats && !self.has_plain)
                .then_some(self.format)
                .flatten(),
        })
    }
}

async fn send_text(
    bot: &Bot,
    chat_id: i64,
    text: FormattedPayload,
    reply_to: Option<&str>,
    delete_after: Option<Duration>,
) -> Vec<MessageId> {
    let mut req = bot.send_message(ChatId(chat_id), text.text);
    if let Some(parse_mode) = text.format.map(parse_mode_from_format) {
        req = req.parse_mode(parse_mode);
    }
    if let Some(reply) = reply_parameters(reply_to) {
        req = req.reply_parameters(reply);
    }
    match req.await {
        Ok(sent) => {
            schedule_delete_after(bot, chat_id, sent.id, delete_after);
            vec![sent.id]
        }
        Err(e) => {
            log::error!("TGAdapter send_message error: {e}");
            Vec::new()
        }
    }
}

async fn send_file(
    bot: &Bot,
    chat_id: i64,
    source: FileSource,
    file_name: Option<String>,
    file_id: Option<String>,
    caption: Option<FormattedPayload>,
    reply_to: Option<&str>,
    delete_after: Option<Duration>,
) -> Vec<MessageId> {
    let input = match input_file_from_source(source, file_name, file_id) {
        Ok(input) => input,
        Err(e) => {
            log::error!("TGAdapter cannot prepare file: {e:#}");
            return Vec::new();
        }
    };

    let mut req = bot.send_document(ChatId(chat_id), input);
    if let Some(caption) = caption {
        if let Some(parse_mode) = caption.format.map(parse_mode_from_format) {
            req = req.parse_mode(parse_mode);
        }
        req = req.caption(caption.text);
    }
    if let Some(reply) = reply_parameters(reply_to) {
        req = req.reply_parameters(reply);
    }
    match req.await {
        Ok(sent) => {
            schedule_delete_after(bot, chat_id, sent.id, delete_after);
            vec![sent.id]
        }
        Err(e) => {
            log::error!("TGAdapter send_document error: {e}");
            Vec::new()
        }
    }
}

async fn send_image(
    bot: &Bot,
    chat_id: i64,
    source: ImageSource,
    file_id: Option<String>,
    caption: Option<FormattedPayload>,
    reply_to: Option<&str>,
    delete_after: Option<Duration>,
) -> Vec<MessageId> {
    let input = match input_file_from_image(source, file_id) {
        Ok(input) => input,
        Err(e) => {
            log::error!("TGAdapter cannot prepare image: {e:#}");
            return Vec::new();
        }
    };

    let mut req = bot.send_photo(ChatId(chat_id), input);
    if let Some(caption) = caption {
        if let Some(parse_mode) = caption.format.map(parse_mode_from_format) {
            req = req.parse_mode(parse_mode);
        }
        req = req.caption(caption.text);
    }
    if let Some(reply) = reply_parameters(reply_to) {
        req = req.reply_parameters(reply);
    }
    match req.await {
        Ok(sent) => {
            schedule_delete_after(bot, chat_id, sent.id, delete_after);
            vec![sent.id]
        }
        Err(e) => {
            log::error!("TGAdapter send_photo error: {e}");
            Vec::new()
        }
    }
}

async fn send_file_media_groups(
    bot: &Bot,
    chat_id: i64,
    files: Vec<ContentItem>,
    mut caption: Option<FormattedPayload>,
    reply_to: Option<&str>,
    delete_after: Option<Duration>,
) -> Vec<MessageId> {
    let mut sent_ids = Vec::new();
    let mut chunk = Vec::new();
    for file in files {
        chunk.push(file);
        if chunk.len() == 10 {
            let chunk_caption = caption.take();
            sent_ids.extend(
                send_file_media_group(
                    bot,
                    chat_id,
                    std::mem::take(&mut chunk),
                    chunk_caption,
                    reply_to,
                    delete_after,
                )
                .await,
            );
        }
    }

    if !chunk.is_empty() {
        sent_ids.extend(
            send_file_media_group(bot, chat_id, chunk, caption, reply_to, delete_after).await,
        );
    }

    sent_ids
}

async fn send_file_media_group(
    bot: &Bot,
    chat_id: i64,
    files: Vec<ContentItem>,
    caption: Option<FormattedPayload>,
    reply_to: Option<&str>,
    delete_after: Option<Duration>,
) -> Vec<MessageId> {
    if files.len() == 1 {
        if let Some(ContentItem::File {
            source,
            file_name,
            file_id,
        }) = files.into_iter().next()
        {
            return send_file(
                bot,
                chat_id,
                source,
                file_name,
                file_id,
                caption,
                reply_to,
                delete_after,
            )
            .await;
        }
        return Vec::new();
    }

    let last_index = files.len() - 1;
    let mut media = Vec::with_capacity(files.len());
    for (idx, file) in files.into_iter().enumerate() {
        let ContentItem::File {
            source,
            file_name,
            file_id,
        } = file
        else {
            unreachable!();
        };
        let input = match input_file_from_source(source, file_name, file_id) {
            Ok(input) => input,
            Err(e) => {
                log::error!("TGAdapter cannot prepare file: {e:#}");
                return Vec::new();
            }
        };
        let mut document = InputMediaDocument::new(input);
        if idx == last_index
            && let Some(caption) = caption.clone()
        {
            if let Some(parse_mode) = caption.format.map(parse_mode_from_format) {
                document = document.parse_mode(parse_mode);
            }
            document = document.caption(caption.text);
        }
        media.push(InputMedia::Document(document));
    }

    let mut req = bot.send_media_group(ChatId(chat_id), media);
    if let Some(reply) = reply_parameters(reply_to) {
        req = req.reply_parameters(reply);
    }
    match req.await {
        Ok(sent) => sent
            .into_iter()
            .map(|sent_message| {
                schedule_delete_after(bot, chat_id, sent_message.id, delete_after);
                sent_message.id
            })
            .collect(),
        Err(e) => {
            log::error!("TGAdapter send_media_group error: {e}");
            Vec::new()
        }
    }
}

fn emit_message_sent_if_needed(
    origin: &str,
    chat_id: i64,
    local_id: Option<&str>,
    message_id: Option<MessageId>,
) {
    let (Some(local_id), Some(message_id)) = (local_id, message_id) else {
        return;
    };

    let mut event = Event::message_sent(
        "TGAdapter",
        message_id.0.to_string(),
        Some(local_id.to_string()),
    );
    if let Some(msg) = event.message.as_mut() {
        msg.to = Some(chat_id.to_string());
    }
    event.receiver = Some(origin.to_string());
    context::bot().emit_event(event);
}

async fn edit_message_item(bot: Bot, chat_id: i64, msg: Message) {
    let Some(message_id) = msg.id.as_deref() else {
        log::error!("TGAdapter edit_message missing message.id");
        return;
    };
    let Ok(message_id) = message_id.parse::<i32>() else {
        log::error!("TGAdapter edit_message requires native Telegram message id: {message_id}");
        return;
    };

    let mut text = OutgoingText::default();
    for item in &msg.content {
        if let ContentItem::Text {
            text: item_text,
            format,
        } = item
        {
            text.push_text(item_text.clone(), *format);
        }
    }
    let Some(payload) = text.into_payload() else {
        log::error!("TGAdapter edit_message has no text content");
        return;
    };

    let mut req = bot.edit_message_text(ChatId(chat_id), MessageId(message_id), payload.text);
    if let Some(parse_mode) = payload.format.map(parse_mode_from_format) {
        req = req.parse_mode(parse_mode);
    }
    if let Err(e) = req.await {
        // Telegram returns an error when the new text is identical to the old
        // one; that is harmless for status updates, so only warn.
        log::warn!("TGAdapter edit_message error: {e}");
    }
}

async fn delete_message_item(bot: Bot, chat_id: i64, msg: Message) {
    let Some(message_id) = msg.id else {
        log::error!("TGAdapter delete_message missing message.id");
        return;
    };

    let Ok(message_id) = message_id.parse::<i32>() else {
        log::error!("TGAdapter delete_message requires native Telegram message id: {message_id}");
        return;
    };

    if let Err(e) = bot
        .delete_message(ChatId(chat_id), MessageId(message_id))
        .await
    {
        log::error!("TGAdapter delete_message error: {e}");
    }
}

fn schedule_delete_after(bot: &Bot, chat_id: i64, message_id: MessageId, delay: Option<Duration>) {
    let Some(delay) = delay else {
        return;
    };

    let bot = bot.clone();
    spawn_send_task(async move {
        tokio::time::sleep(delay).await;
        if let Err(e) = bot.delete_message(ChatId(chat_id), message_id).await {
            log::warn!("TGAdapter delete_after delete_message error: {e}");
        }
    });
}

fn merge_captions(
    first: Option<FormattedPayload>,
    second: Option<String>,
) -> Option<FormattedPayload> {
    match (first, second.filter(|s| !s.is_empty())) {
        (Some(first), Some(second)) if first.text == second => Some(first),
        (Some(mut first), Some(second)) => {
            first.text.push('\n');
            first.text.push_str(&second);
            first.format = None;
            Some(first)
        }
        (Some(first), None) => Some(first),
        (None, Some(second)) => Some(FormattedPayload {
            text: second,
            format: None,
        }),
        (None, None) => None,
    }
}

#[allow(deprecated)]
fn parse_mode_from_format(format: TextFormat) -> ParseMode {
    match format {
        TextFormat::Markdown => ParseMode::Markdown,
        TextFormat::MarkdownV2 => ParseMode::MarkdownV2,
        TextFormat::Html => ParseMode::Html,
    }
}

fn reply_parameters(reply_to: Option<&str>) -> Option<ReplyParameters> {
    let msg_id = reply_to?.parse::<i32>().ok()?;
    Some(ReplyParameters {
        message_id: MessageId(msg_id),
        ..Default::default()
    })
}

fn input_file_from_source(
    source: FileSource,
    file_name: Option<String>,
    file_id: Option<String>,
) -> anyhow::Result<InputFile> {
    let mut input = if let Some(file_id) = non_empty(file_id) {
        InputFile::file_id(FileId(file_id))
    } else {
        match source {
            FileSource::Id(file_id) => InputFile::file_id(FileId(file_id)),
            FileSource::Path(path) => InputFile::file(path),
            FileSource::Url(url) => InputFile::url(
                url::Url::parse(&url).with_context(|| format!("invalid file URL: {url}"))?,
            ),
        }
    };

    if let Some(file_name) = non_empty(file_name) {
        input = input.file_name(file_name);
    }

    Ok(input)
}

fn input_file_from_image(
    source: ImageSource,
    file_id: Option<String>,
) -> anyhow::Result<InputFile> {
    if let Some(file_id) = non_empty(file_id) {
        return Ok(InputFile::file_id(FileId(file_id)));
    }

    match source {
        ImageSource::Id(file_id) => Ok(InputFile::file_id(FileId(file_id))),
        ImageSource::Url(url) => Ok(InputFile::url(
            url::Url::parse(&url).with_context(|| format!("invalid image URL: {url}"))?,
        )),
        ImageSource::Path(path) => Ok(InputFile::file(path)),
        ImageSource::Base64(data) => {
            let bytes = decode_base64_image(&data)?;
            Ok(InputFile::memory(bytes))
        }
    }
}

fn decode_base64_image(data: &str) -> anyhow::Result<Vec<u8>> {
    let encoded = data
        .split_once(',')
        .filter(|(prefix, _)| prefix.contains("base64"))
        .map_or(data, |(_, encoded)| encoded)
        .trim();
    general_purpose::STANDARD
        .decode(encoded)
        .context("invalid base64 image data")
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

#[cfg(test)]
#[path = "../tests/unit/outgoing_tests.rs"]
mod outgoing_tests;
