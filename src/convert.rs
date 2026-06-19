use std::collections::BTreeMap;

use snb_core::event::{
    Chat, ChatType, ContentItem, Event, EventType, FileSource, ImageSource, Message, Sender,
};
use teloxide::types::{ChatKind, MessageEntityKind, PublicChatKind, Update, UpdateKind};

use crate::state;

pub(crate) fn convert_update(update: &Update) -> Option<Event> {
    match &update.kind {
        UpdateKind::Message(msg)
        | UpdateKind::EditedMessage(msg)
        | UpdateKind::ChannelPost(msg)
        | UpdateKind::EditedChannelPost(msg)
        | UpdateKind::BusinessMessage(msg)
        | UpdateKind::EditedBusinessMessage(msg) => convert_message(update, msg),

        kind => {
            let kind_name = match kind {
                UpdateKind::Message(_) => unreachable!(),
                UpdateKind::EditedMessage(_) => unreachable!(),
                UpdateKind::ChannelPost(_) => unreachable!(),
                UpdateKind::EditedChannelPost(_) => unreachable!(),
                UpdateKind::BusinessMessage(_) => unreachable!(),
                UpdateKind::EditedBusinessMessage(_) => unreachable!(),
                UpdateKind::BusinessConnection(_) => "BusinessConnection",
                UpdateKind::DeletedBusinessMessages(_) => "DeletedBusinessMessages",
                UpdateKind::MessageReaction(_) => "MessageReaction",
                UpdateKind::MessageReactionCount(_) => "MessageReactionCount",
                UpdateKind::InlineQuery(_) => "InlineQuery",
                UpdateKind::ChosenInlineResult(_) => "ChosenInlineResult",
                UpdateKind::CallbackQuery(_) => "CallbackQuery",
                UpdateKind::ShippingQuery(_) => "ShippingQuery",
                UpdateKind::PreCheckoutQuery(_) => "PreCheckoutQuery",
                UpdateKind::PurchasedPaidMedia(_) => "PurchasedPaidMedia",
                UpdateKind::Poll(_) => "Poll",
                UpdateKind::PollAnswer(_) => "PollAnswer",
                UpdateKind::MyChatMember(_) => "MyChatMember",
                UpdateKind::ChatMember(_) => "ChatMember",
                UpdateKind::ChatJoinRequest(_) => "ChatJoinRequest",
                UpdateKind::ChatBoost(_) => "ChatBoost",
                UpdateKind::RemovedChatBoost(_) => "RemovedChatBoost",
                UpdateKind::Error(_) => "Error",
            };
            let data = serde_json::to_string(kind).unwrap_or_default();
            Some(Event {
                event_type: EventType::Other(kind_name.to_string()),
                source: "tg-adapter".to_string(),
                data,
                command: None,
                message: None,
                reply_plugin: Some("TGAdapter".to_string()),
                target_plugin: None,
            })
        }
    }
}

fn convert_attachments(msg: &teloxide::types::Message) -> Vec<ContentItem> {
    let mut items = Vec::new();

    if let Some(document) = msg.document() {
        items.push(ContentItem::File {
            source: FileSource::Id(document.file.id.0.clone()),
            file_name: document.file_name.clone(),
            file_id: Some(document.file.id.0.clone()),
        });
    }

    if let Some(photo) = msg
        .photo()
        .and_then(|photos| photos.iter().max_by_key(|photo| photo.width * photo.height))
    {
        items.push(ContentItem::Image {
            source: ImageSource::Id(photo.file.id.0.clone()),
            file_id: Some(photo.file.id.0.clone()),
            caption: msg.caption().map(str::to_string),
        });
    }

    if let Some(audio) = msg.audio() {
        items.push(ContentItem::File {
            source: FileSource::Id(audio.file.id.0.clone()),
            file_name: audio.file_name.clone(),
            file_id: Some(audio.file.id.0.clone()),
        });
    }

    if let Some(video) = msg.video() {
        items.push(ContentItem::File {
            source: FileSource::Id(video.file.id.0.clone()),
            file_name: video.file_name.clone(),
            file_id: Some(video.file.id.0.clone()),
        });
    }

    if let Some(animation) = msg.animation() {
        items.push(ContentItem::File {
            source: FileSource::Id(animation.file.id.0.clone()),
            file_name: animation.file_name.clone(),
            file_id: Some(animation.file.id.0.clone()),
        });
    }

    if let Some(voice) = msg.voice() {
        items.push(ContentItem::File {
            source: FileSource::Id(voice.file.id.0.clone()),
            file_name: Some("voice.ogg".to_string()),
            file_id: Some(voice.file.id.0.clone()),
        });
    }

    items
}

fn convert_message(update: &Update, msg: &teloxide::types::Message) -> Option<Event> {
    let text = msg.text().or(msg.caption()).unwrap_or("");
    let mut content = if text.is_empty() {
        vec![]
    } else {
        vec![ContentItem::text(text)]
    };
    content.extend(convert_attachments(msg));

    let from = msg.from.as_ref().map(|u| u.id.0.to_string());
    let chat_id = msg.chat.id.0.to_string();
    let (chat_type, raw_kind) = match &msg.chat.kind {
        ChatKind::Private(_) => (ChatType::Private, "private"),
        ChatKind::Public(public) => match public.kind {
            PublicChatKind::Group => (ChatType::Group, "group"),
            PublicChatKind::Supergroup(_) => (ChatType::Group, "supergroup"),
            PublicChatKind::Channel(_) => (ChatType::Channel, "channel"),
        },
    };

    let sender = msg.from.as_ref().map(|u| {
        let mut extra = BTreeMap::new();
        if u.is_premium {
            extra.insert("is_premium".to_string(), "true".to_string());
        }
        let full = u.full_name();
        Sender {
            id: u.id.0.to_string(),
            username: u.username.clone(),
            display_name: (!full.is_empty()).then_some(full),
            first_name: Some(u.first_name.clone()),
            last_name: u.last_name.clone(),
            is_bot: u.is_bot,
            language: u.language_code.clone(),
            extra,
        }
    });

    let mut chat_extra = BTreeMap::new();
    chat_extra.insert("raw_kind".to_string(), raw_kind.to_string());
    let chat = Chat {
        id: chat_id,
        kind: Some(chat_type),
        title: msg.chat.title().map(str::to_string),
        username: msg.chat.username().map(str::to_string),
        extra: chat_extra,
    };

    let entities = msg
        .parse_entities()
        .or_else(|| msg.parse_caption_entities())
        .unwrap_or_default();

    let mut at = Vec::new();
    let mut command: Option<snb_core::event::Command> = None;

    for entity in &entities {
        match entity.kind() {
            MessageEntityKind::Mention => {
                at.push(entity.text().to_string());
            }
            MessageEntityKind::TextMention { user } => {
                at.push(user.id.0.to_string());
            }
            MessageEntityKind::BotCommand if command.is_none() => {
                let raw = entity.text();
                let stripped = raw.strip_prefix('/').unwrap_or(raw);
                let cmd = match stripped.find('@') {
                    Some(i) => &stripped[..i],
                    None => stripped,
                };
                let args_start = entity.end();
                let args = if args_start < text.len() {
                    text[args_start..].trim_start()
                } else {
                    ""
                };
                command = Some(snb_core::event::Command {
                    cmd: cmd.to_string(),
                    args: args.to_string(),
                });
            }
            _ => {
                log::debug!("Unresolved message entity kind: {:?}", entity.kind());
            }
        }
    }

    let event_msg = Message {
        id: Some(msg.id.0.to_string()),
        reply_to: msg.reply_to_message().map(|m| m.id.0.to_string()),
        content,
        sender,
        chat,
        at,
        is_admin: state::is_configured_admin(from.as_deref()),
        ..Default::default()
    };

    let kind_name = match &update.kind {
        UpdateKind::Message(_) => "Message",
        UpdateKind::EditedMessage(_) => "EditedMessage",
        UpdateKind::ChannelPost(_) => "ChannelPost",
        UpdateKind::EditedChannelPost(_) => "EditedChannelPost",
        UpdateKind::BusinessMessage(_) => "BusinessMessage",
        UpdateKind::EditedBusinessMessage(_) => "EditedBusinessMessage",
        _ => unreachable!(),
    };

    let (event_type, command, message) = match command {
        Some(cmd) => (EventType::Command, Some(cmd), Some(event_msg)),
        None => (EventType::Message, None, Some(event_msg)),
    };

    Some(Event {
        event_type,
        source: "tg-adapter".to_string(),
        data: kind_name.to_string(),
        command,
        message,
        reply_plugin: Some("TGAdapter".to_string()),
        target_plugin: None,
    })
}
