use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::json;
use serenity::model::channel::Message;
use serenity::prelude::Context;
use std::fs;

#[derive(Serialize, Deserialize, Debug)]
struct GuildJson {
    id: String,
    name: String,
    icon_url: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct ChannelJson {
    id: String,
    category: Option<String>,
    name: String,
    topic: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct UserJson {
    id: String,
    name: String,
    tag: String,
    is_bot: bool,
    avatar_url: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct AttachmentJson {
    id: String,
    url: String,
    file_name: String,
    file_size_bytes: u64,
}

#[derive(Serialize, Deserialize, Debug)]
struct MessageJson {
    id: String,
    timestamp: String,
    timestamp_edited: Option<String>,
    is_pinned: bool,
    content: String,
    author: UserJson,
    attachments: Vec<AttachmentJson>,
}

pub async fn write_json<P: AsRef<Path>>(
    messages: &[Message],
    path: P,
    ctx: &Context,
) -> Result<(), Box<dyn std::error::Error>> {
    let channel = match (&messages)
        .first()
        .unwrap()
        .channel(&ctx)
        .await
        .unwrap()
        .guild()
    {
        Some(x) => x,
        None => return Err("Invalid channel type.".into()),
    };
    let guild = channel.guild_id.to_partial_guild(&ctx).await?;

    let guild_json = GuildJson {
        id: guild.id.to_string(),
        icon_url: (&guild).icon_url(),
        name: guild.name,
    };

    let channel_json = ChannelJson {
        id: channel.id.to_string(),
        category: match channel.category_id {
            Some(x) => x.name(&ctx).await,
            None => None,
        },
        name: channel.name().into(),
        topic: channel.topic,
    };

    let message_jsons: Vec<MessageJson> = messages
        .iter()
        .map(|message| {
            let author = &message.author;
            MessageJson {
                id: message.id.to_string(),
                timestamp: message.timestamp.to_string(),
                timestamp_edited: message.edited_timestamp.map(|x| x.to_string()),
                is_pinned: message.pinned,
                content: message.content.clone(),
                author: UserJson {
                    id: author.id.to_string(),
                    name: author.name.clone(),
                    tag: format!("{:04}", author.discriminator),
                    is_bot: author.bot,
                    avatar_url: author.avatar_url(),
                },
                attachments: message
                    .attachments
                    .iter()
                    .map(|x| AttachmentJson {
                        id: x.id.to_string(),
                        url: x.url.clone(),
                        file_name: x.filename.clone(),
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
    Ok(())
}
