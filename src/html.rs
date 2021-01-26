use std::collections::HashMap;

use std::fs;
use std::path::Path;

use log::*;

use serenity::model::channel::ChannelCategory;
use serenity::model::channel::Message;
use serenity::model::guild::Role;
use serenity::model::id::RoleId;
use serenity::model::user::User;
use serenity::prelude::Context;

static CORE_THEME_CSS: &str = include_str!("html_templates/core.css");
static DARK_THEME_CSS: &str = include_str!("html_templates/dark.css");
static LIGHT_THEME_CSS: &str = include_str!("html_templates/light.css");

pub async fn write_html<P: AsRef<Path>>(
    messages: &[Message],
    path: P,
    ctx: &Context,
) -> Result<(), Box<dyn std::error::Error>> {
    let channel = messages
        .first()
        .unwrap()
        .channel_id
        .to_channel(&ctx)
        .await
        .unwrap()
        .guild()
        .unwrap();

    let guild = channel.guild_id.to_partial_guild(&ctx).await.unwrap();

    let dark_mode = true;
    let html = include_str!("html_templates/preamble_template.html");
    let html = html.replace("DISCORD_GUILD_NAME", &guild.name);
    let html = html.replace("DISCORD_CHANNEL_NAME", &channel.name);

    let html = html.replace("CORE_STYLESHEET", CORE_THEME_CSS);

    let html = if dark_mode {
        html.replace("THEME_STYLESHEET", DARK_THEME_CSS)
    } else {
        html.replace("THEME_STYLESHEET", LIGHT_THEME_CSS)
    };

    let html = html.replace(
        "DISCORD_GUILD_ICON_URL",
        &guild.icon_url().unwrap_or("".into()),
    );

    let category_name = match channel.category_id {
        Some(x) => x.name(&ctx).await,
        None => None,
    };

    let html = html.replace(
        "DISCORD_CHANNEL_CATEGORY_SLASH_NAME",
        &match category_name {
            Some(x) => format!("{} / {}", x, channel.name,),
            None => channel.name,
        },
    );

    let mut html = if channel.topic.is_some() {
        html.replace(
            "CHANNEL_TOPIC_DIV",
            &format!(
                r#"<div class="preamble__entry preamble__entry--small">{}</div>"#,
                channel.topic.unwrap()
            ),
        )
    } else {
        html.replace("CHANNEL_TOPIC_DIV", "")
    };

    for (i, message) in messages.iter().enumerate() {
        let author = &message.author;
        let author_nick_or_user = match author.nick_in(&ctx, guild.id).await {
            Some(x) => x,
            None => author.name.clone(),
        };

        let author_highest_role = {
            let roles = {
                match guild.member(&ctx, &author.id).await {
                    Ok(x) => x.roles,
                    Err(_) => Vec::new(),
                }
            };
            let mut roles: Vec<_> = roles
                .iter()
                .map(|roleid| guild.roles.get(&roleid).unwrap())
                .collect();
            roles.sort_by_key(|role| role.position);
            match roles.last() {
                Some(x) => Some(*x),
                None => None,
            }
        };

        let author_avatar_container = format!(
            r#"<div class="chatlog__author-avatar-container">
    <img
        class="chatlog__author-avatar"
        src="{}"
        alt="Avatar"
    />
</div>"#,
            author.avatar_url().unwrap_or("".into())
        );

        let message_timestamp = format!(
            r#"<span class="chatlog__timestamp">{}</span>"#,
            message.timestamp
        );

        let author_name_container = format!(
            r#"<span
class="chatlog__author-name"
title="{}#{:04}"
data-user-id="{}"
style="color: rgb({}, {}, {})">
{}</span>"#,
            author.name,
            author.discriminator,
            author.id.to_string(),
            author_highest_role.map(|x| x.colour.r()).unwrap_or(255),
            author_highest_role.map(|x| x.colour.g()).unwrap_or(255),
            author_highest_role.map(|x| x.colour.b()).unwrap_or(255),
            author_nick_or_user,
        );
        let message_group = format!(
            r#"<div class="chatlog__message-group">
{}
<div class="chatlog__messages">
{}
{}

<div
  class="chatlog__message"
  data-message-id="{}"
  id="message-{}"
>
<div class="chatlog__content">
<div class="markdown">{}</div>
</div>
</div>
</div>
</div>"#,
            author_avatar_container,
            author_name_container,
            message_timestamp,
            message.id.to_string(),
            message.id.to_string(),
            message.content,
        );
        html.extend(message_group.chars());
        info!("Archived message {} / {}", i, messages.len());
    }

    html.extend(include_str!("html_templates/postamble_template.html").chars());

    let html = html.replace(
        "EXPORTED_MESSAGES_NUMBER",
        &format!(
            "Exported {} message{}",
            messages.len(),
            if messages.len() == 1 { "" } else { "s" }
        ),
    );

    let html = html.replace("\u{feff}", "");

    println!("{}", html);

    fs::write(path, html)?;

    Ok(())
}
