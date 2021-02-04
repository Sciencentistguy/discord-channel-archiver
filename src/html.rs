use log::*;
use std::fs;
use std::path::Path;

use serenity::model::channel::Message;
use serenity::model::user::User;
use serenity::prelude::Context;

use futures::future::join_all;

static CORE_THEME_CSS: &str = include_str!("html_templates/core.css");
static DARK_THEME_CSS: &str = include_str!("html_templates/dark.css");
static LIGHT_THEME_CSS: &str = include_str!("html_templates/light.css");

pub async fn write_html<P: AsRef<Path>>(
    messages: &[Message],
    path: P,
    ctx: &Context,
) -> Result<(), Box<dyn std::error::Error>> {
    trace!("Entered HTML generator.");
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
            None => channel.name.clone(),
        },
    );

    let mut html = if channel.topic.is_some() {
        html.replace(
            "CHANNEL_TOPIC_DIV",
            &format!(
                r#"<div class="preamble__entry preamble__entry--small">{}</div>"#,
                channel.topic.as_ref().unwrap()
            ),
        )
    } else {
        html.replace("CHANNEL_TOPIC_DIV", "")
    };
    trace!("Generated preamble");
    trace!("Begin getting members");
    let mut members: Vec<_> = messages.iter().map(|x| &x.author).collect();
    members.sort_by_key(|user| user.id);
    members.dedup();
    let members: Vec<_> = members.iter().map(|x| guild.member(&ctx, x.id)).collect();

    trace!("Need to get {} members", members.len());
    let members = join_all(members).await;
    let members: Vec<_> = members
        .into_iter()
        .filter_map(|x| match x {
            Ok(x) => Some(x),
            Err(_) => None,
        })
        .collect();

    let member_userids: Vec<_> = members.iter().map(|x| x.user.id).collect();

    let get_highest_role = |user: &User| {
        //if !channel_members_users.iter().find(|x| x.).is_some();
        if !member_userids.contains(&user.id) {
            warn!("Message author found who is not a member of the channel");
            return None;
        }
        let roles = match members.iter().find(|member| member.user.id == user.id) {
            Some(x) => &x.roles,
            None => {
                warn!("User {} has no roles", user.name);
                return None;
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

    let get_name_used = |user: &User| {
        trace!("Begin getting name for user {}", user.name);
        if !member_userids.contains(&user.id) {
            warn!("Message author found who is not a member of the channel");
            return user.name.clone();
        }
        match members
            .iter()
            .find(|member| member.user.id == user.id)
            .unwrap()
            .nick
        {
            Some(ref x) => x.clone(),
            None => user.name.clone(),
        }
    };

    trace!("Begin saving messages");
    for (i, message) in messages.iter().enumerate() {
        let author = &message.author;
        let author_nick_or_user = get_name_used(&message.author);
        let author_highest_role = get_highest_role(&message.author);

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
        html.push_str(&message_group);
        trace!("Archived message {} / {}", i, messages.len());
    }
    trace!("Generated message html");

    html.push_str(include_str!("html_templates/postamble_template.html"));

    let html = html.replace(
        "EXPORTED_MESSAGES_NUMBER",
        &format!(
            "Exported {} message{}",
            messages.len(),
            if messages.len() == 1 { "" } else { "s" }
        ),
    );

    let html = html.replace("\u{feff}", "");

    fs::write(path, html)?;

    info!("HTML generation complete.");

    Ok(())
}
