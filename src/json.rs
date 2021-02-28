use std::fs;
use std::path::Path;

use log::*;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use serenity::model::channel::Message;
use serenity::prelude::Context;

#[derive(Serialize, Deserialize, Debug)]
struct GuildJson<'a> {
    id: u64,
    name: &'a str,
    icon_url: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct ChannelJson<'a> {
    id: u64,
    category: Option<String>,
    name: &'a str,
    topic: Option<&'a str>,
}

#[derive(Serialize, Deserialize, Debug)]
struct UserJson<'a> {
    id: u64,
    name: &'a str,
    discriminator: u16,
    is_bot: bool,
    avatar_url: Option<String>,
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
    id: u64,
    timestamp: i64,
    timestamp_edited: Option<i64>,
    is_pinned: bool,
    content: &'a str,
    author: UserJson<'a>,
    attachments: Vec<AttachmentJson<'a>>,
}

pub async fn write_json<P: AsRef<Path>>(
    messages: &[Message],
    path: P,
    ctx: &Context,
) -> Result<(), Box<dyn std::error::Error>> {
    trace!("Entered json writer.");
    let channel = messages
        .first()
        .unwrap()
        .channel(&ctx)
        .await
        .unwrap()
        .guild()
        .unwrap();
    let guild = channel.guild_id.to_partial_guild(&ctx).await?;

    let guild_json = GuildJson {
        id: *guild.id.as_u64(),
        icon_url: (&guild).icon_url(),
        name: guild.name.as_str(),
    };

    let channel_json = ChannelJson {
        id: *channel.id.as_u64(),
        category: match channel.category_id {
            Some(x) => x.name(&ctx).await,
            None => None,
        },
        name: channel.name(),
        topic: channel.topic.as_deref(),
    };

    let message_jsons: Vec<MessageJson> = messages
        .iter()
        .enumerate()
        .map(|(i, message)| {
            let author = &message.author;
            trace!("Archived message {} / {}", i, messages.len());
            MessageJson {
                id: *message.id.as_u64(),
                timestamp: message.timestamp.timestamp(),
                timestamp_edited: message.edited_timestamp.map(|x| x.timestamp()),
                is_pinned: message.pinned,
                content: message.content.as_str(),
                author: UserJson {
                    id: *author.id.as_u64(),
                    name: author.name.as_str(),
                    discriminator: author.discriminator,
                    is_bot: author.bot,
                    avatar_url: author.avatar_url(),
                },
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
