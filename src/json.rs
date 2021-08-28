use crate::Result;

use std::fs;
use std::path::Path;

use log::*;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use serenity::model::channel::Message;
use serenity::model::channel::MessageType;
use serenity::model::guild::PartialGuild;
use serenity::prelude::Context;

#[derive(Serialize, Deserialize, Debug)]
struct GuildJson<'a> {
    icon_url: Option<String>,
    id: u64,
    name: &'a str,
}

#[derive(Serialize, Deserialize, Debug)]
struct ChannelJson<'a> {
    category: Option<String>,
    id: u64,
    name: &'a str,
    num_messages: usize,
    topic: Option<&'a str>,
}

#[derive(Serialize, Deserialize, Debug)]
struct UserJson<'a> {
    avatar_url: Option<String>,
    discriminator: u16,
    id: u64,
    is_bot: bool,
    name: &'a str,
}

#[derive(Serialize, Deserialize, Debug)]
struct AttachmentJson<'a> {
    id: u64,
    url: &'a str,
    file_name: &'a str,
    file_size_bytes: u64,
}

#[derive(Serialize, Deserialize, Debug)]
struct MessageJson<'a> {
    attachments: Vec<AttachmentJson<'a>>,
    author: UserJson<'a>,
    content: &'a str,
    id: u64,
    is_pinned: bool,
    kind: MessageType,
    message_url: String,
    timestamp: i64,
    timestamp_edited: Option<i64>,
}

pub async fn write_json<P: AsRef<Path>>(
    ctx: &Context,
    guild: &PartialGuild,
    messages: &[Message],
    path: P,
) -> Result<()> {
    trace!("Entered json writer.");
    let channel = messages
        .first()
        .unwrap()
        .channel(&ctx)
        .await
        .unwrap()
        .guild()
        .unwrap();

    let guild_json = GuildJson {
        id: guild.id.0,
        icon_url: guild.icon_url(),
        name: guild.name.as_str(),
    };

    let channel_json = ChannelJson {
        id: channel.id.0,
        category: match channel.category_id {
            Some(x) => x.name(&ctx).await,
            None => None,
        },
        name: channel.name(),
        topic: channel.topic.as_deref(),
        num_messages: messages.len(),
    };

    let message_jsons: Vec<MessageJson> = messages
        .iter()
        .map(|message| {
            let author = &message.author;
            MessageJson {
                attachments: message
                    .attachments
                    .iter()
                    .map(|x| AttachmentJson {
                        id: *x.id.as_u64(),
                        url: x.url.as_str(),
                        file_name: x.filename.as_str(),
                        file_size_bytes: x.size,
                    })
                    .collect(),
                author: UserJson {
                    avatar_url: author.avatar_url(),
                    discriminator: author.discriminator,
                    id: *author.id.as_u64(),
                    is_bot: author.bot,
                    name: author.name.as_str(),
                },
                content: message.content.as_str(),
                id: *message.id.as_u64(),
                is_pinned: message.pinned,
                kind: message.kind,
                message_url: message.link(),
                timestamp: message.timestamp.timestamp(),
                timestamp_edited: message.edited_timestamp.map(|x| x.timestamp()),
            }
        })
        .collect();

    let json = json!({
        "guild" : guild_json,
        "channel" : channel_json,
        "messages" : message_jsons
    });
    let file = fs::File::create(path)?;
    serde_json::to_writer_pretty(file, &json)?;
    info!("JSON generation complete.");
    Ok(())
}
