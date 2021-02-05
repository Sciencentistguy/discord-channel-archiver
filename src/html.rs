use log::*;
use std::fs;
use std::path::Path;

use lazy_static::lazy_static;

use serenity::model::channel::Message;
use serenity::model::user::User;
use serenity::prelude::Context;

use regex::Regex;

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
        trace!("Begin getting highest role for user {}", user.name);
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

    let render_message = |content: &str| -> String {
        let content = content.replace("<", "&lt;").replace(">", "&gt;");
        lazy_static! {
            static ref CUSTOM_EMOJI_RE: Regex = Regex::new(r"&lt;a?:(\w+):(\d+)&gt;").unwrap();
            static ref INLINE_CODE_RE: Regex = Regex::new(r"`([^`]*)`").unwrap();
            static ref BOLD_RE: Regex = Regex::new(r"\*\*([^\*]+)\*\*").unwrap();
            static ref UNDERLINE_RE: Regex = Regex::new(r"__([^_]+)__").unwrap();
            static ref ITALICS_RE: Regex = Regex::new(r"\*([^\*]+)\*").unwrap();
            static ref ITALICS_RE2: Regex = Regex::new(r"_([^_]+)_").unwrap();
            static ref STRIKETHROUGH_RE: Regex = Regex::new(r"~~([^~]+)~~").unwrap();
            static ref EMOJI_RE: Regex = Regex::new(r":(\w+):").unwrap();
        };

        let content = CUSTOM_EMOJI_RE.replace_all(&content, |capts: &regex::Captures| {
            trace!("Found custom emoji '{}' in '{}'", &capts[1], content);
            //let emoji_id = &capts[2];
            if let Some(emoji) = serenity::utils::parse_emoji(&capts[0]) {
                format!(r#"<img src="{}" alt="{}"/>"#, emoji.url(), &capts[1])
            } else {
                capts[0].to_string()
            }
            //let emoji_symbol = match gh_emoji::get(emoji_name) {
            //Some(x) => x,
            //None => emoji_name,
            //};
            //emoji_symbol.to_string()
        });

        let content = {
            let content: String = content.into();
            trace!("Found multiline code block in '{}'", content);
            let mut out = String::new();
            let split = content.split("```");
            for (c, block) in split.enumerate() {
                if c & 1 == 0 {
                    out.push_str(block);
                } else {
                    out.push_str(r#"<pre class="pre pre--multiline">"#);
                    out.push_str(block);
                    out.push_str(r#"</pre>"#);
                }
            }
            out
        };
        let content = INLINE_CODE_RE.replace_all(&content, |capts: &regex::Captures| {
            trace!("Found inline code block in '{}'", content);
            format!(r#"<code class="pre pre--inline">{}</code>"#, &capts[1])
        });

        let content = BOLD_RE.replace_all(&content, |capts: &regex::Captures| {
            trace!("Found bold block in '{}'", content);
            format!("<b>{}</b>", &capts[1])
        });

        let content = UNDERLINE_RE.replace_all(&content, |capts: &regex::Captures| {
            trace!("Found underline block in '{}'", content);
            format!("<u>{}</u>", &capts[1])
        });

        let content = ITALICS_RE.replace_all(&content, |capts: &regex::Captures| {
            trace!("Found italics block in '{}'", content);
            format!("<i>{}</i>", &capts[1])
        });
        let content = ITALICS_RE2.replace_all(&content, |capts: &regex::Captures| {
            trace!("Found italics 2 block in '{}'", content);
            format!("<i>{}</i>", &capts[1])
        });

        let content = STRIKETHROUGH_RE.replace_all(&content, |capts: &regex::Captures| {
            trace!("Found strikethrough block in '{}'", content);
            format!("<s>{}</s>", &capts[1])
        });

        let content = EMOJI_RE.replace_all(&content, |capts: &regex::Captures| {
            trace!("Found emoji '{}' in '{}'", &capts[0], content);
            let emoji_name = &capts[1];
            let emoji_symbol = match gh_emoji::get(emoji_name) {
                Some(x) => x,
                None => emoji_name,
            };
            emoji_symbol.to_string()
        });

        let content: String = content.into();
        content
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

        let content = render_message(&message.content);

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
            content,
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
