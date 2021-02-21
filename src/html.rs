use log::*;

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use lazy_static::lazy_static;

use serenity::model::channel::Message;
use serenity::model::guild::Member;
use serenity::model::guild::PartialGuild;
use serenity::model::guild::Role;
use serenity::model::id::UserId;
use serenity::model::user::User;
use serenity::prelude::Context;

use regex::Regex;

use futures::future::join_all;

static CORE_THEME_CSS: &str = include_str!("html_templates/core.css");
static DARK_THEME_CSS: &str = include_str!("html_templates/dark.css");
static LIGHT_THEME_CSS: &str = include_str!("html_templates/light.css");

lazy_static! {
    static ref CUSTOM_EMOJI_RE: Regex = Regex::new(r"&lt;a?:(\w+):(\d+)&gt;").unwrap();
    static ref INLINE_CODE_RE: Regex = Regex::new(r"`([^`]*)`").unwrap();
    static ref BOLD_RE: Regex = Regex::new(r"\*\*([^\*]+)\*\*").unwrap();
    static ref UNDERLINE_RE: Regex = Regex::new(r"__([^_]+)__").unwrap();
    static ref ITALICS_RE: Regex = Regex::new(r"\*([^\*]+)\*").unwrap();
    static ref ITALICS_RE2: Regex = Regex::new(r"_([^_]+)_").unwrap();
    static ref STRIKETHROUGH_RE: Regex = Regex::new(r"~~([^~]+)~~").unwrap();
    static ref EMOJI_RE: Regex = Regex::new(r":(\w+):").unwrap();
    static ref CHANNEL_MENTION_RE: Regex = Regex::new(r"&lt;#(\d+)&gt;").unwrap();
    static ref USER_MENTION_RE: Regex = Regex::new(r"&lt;@!(\d+)&gt;").unwrap();
    static ref USER_MENTION_UNSANITISED_RE: Regex = Regex::new(r"<@!(\d+)>").unwrap();
}

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

    trace!("Begin getting channel members");
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

    let message_renderer = MessageRenderer::new(&ctx, &guild, members).await?;

    trace!("Begin saving messages");
    for (i, message) in messages.iter().enumerate() {
        let author = &message.author;
        let author_nick_or_user = message_renderer.get_name_used(&message.author);
        let author_highest_role = message_renderer.get_highest_role(&guild, &message.author);

        let author_avatar_container = format!(
            r#"<div class="chatlog__author-avatar-container">
    <img class="chatlog__author-avatar" src="{}" alt="Avatar" title="Avatar" />
</div>"#,
            get_avatar_url(&author)
        );

        let message_timestamp = format!(
            r#"<span class="chatlog__timestamp">{}</span>"#,
            message.timestamp
        );

        let author_name_container = format!(
            r#"<span class="chatlog__author-name" title="{}#{:04}" data-user-id="{}" style="color: rgb({}, {}, {})">
    {}
</span>"#,
            author.name,
            author.discriminator,
            author.id.to_string(),
            author_highest_role.map(|x| x.colour.r()).unwrap_or(255),
            author_highest_role.map(|x| x.colour.g()).unwrap_or(255),
            author_highest_role.map(|x| x.colour.b()).unwrap_or(255),
            author_nick_or_user,
        );

        let content = message_renderer
            .render_message(&message.content, &ctx)
            .await;

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

struct MessageRenderer {
    channel_names: HashMap<u64, String>,
    members: HashMap<UserId, Member>,
}

impl MessageRenderer {
    async fn new(
        ctx: &Context,
        guild: &PartialGuild,
        members: Vec<Member>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        trace!("Begin getting channel names");
        let mut channel_names = HashMap::new();
        let mut channels = guild.channels(&ctx).await?;
        for (id, channel) in channels.iter_mut() {
            channel_names.insert(*id.as_u64(), std::mem::take(&mut channel.name));
        }

        //trace!("Begin getting names for mentioned users");
        //trace!("Need to get names for {} users.", mentioned_uids.len());
        //let mut user_names: HashMap<u64, User> = HashMap::new();
        //for uid in mentioned_uids {
        //let name = uid.to_user(&ctx).await?;
        //trace!("Got name '{}' for user '{}'", name, uid.as_u64());
        //user_names.insert(*uid.as_u64(), name);
        //}

        let m: HashMap<_, _> = members
            .into_iter()
            .map(|member| (member.user.id, member))
            .collect();

        Ok(Self {
            channel_names,
            members: m,
        })
    }

    async fn render_message(&self, content: &str, ctx: &Context) -> String {
        //TODO don't render mardown inside code blocks.

        let content = content.replace("<", "&lt;").replace(">", "&gt;");

        // Custom (non-unicode) emoji
        let content = CUSTOM_EMOJI_RE.replace_all(&content, |capts: &regex::Captures| {
            trace!("Found custom emoji '{}' in '{}'", &capts[1], content);
            let emoji_mention = capts[0]
                .to_string() // TODO this is an allocation that can probably be avoided
                .replace("&lt;", "<")
                .replace("&gt;", ">");
            if let Some(emoji) = serenity::utils::parse_emoji(emoji_mention) {
                format!(
                    r#"<img class="emoji" src="{0:}" alt="{1:}" title="{1:}"/>"#,
                    emoji.url(),
                    &capts[1]
                )
            } else {
                capts[0].to_string()
            }
        });

        // Code blocks
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

        // Inline code blocks
        let content = INLINE_CODE_RE.replace_all(&content, |capts: &regex::Captures| {
            trace!("Found inline code block in '{}'", content);
            format!(r#"<code class="pre pre--inline">{}</code>"#, &capts[1])
        });

        // Bold (double asterisk)
        let content = BOLD_RE.replace_all(&content, |capts: &regex::Captures| {
            trace!("Found bold block in '{}'", content);
            format!("<b>{}</b>", &capts[1])
        });

        // Underline (double underscore)
        let content = UNDERLINE_RE.replace_all(&content, |capts: &regex::Captures| {
            trace!("Found underline block in '{}'", content);
            format!("<u>{}</u>", &capts[1])
        });

        // Italics (single asterisk)
        let content = ITALICS_RE.replace_all(&content, |capts: &regex::Captures| {
            trace!("Found italics block in '{}'", content);
            format!("<i>{}</i>", &capts[1])
        });

        // Italics (single underscore)
        let content = ITALICS_RE2.replace_all(&content, |capts: &regex::Captures| {
            trace!("Found italics 2 block in '{}'", content);
            format!("<i>{}</i>", &capts[1])
        });

        // Strikethrough (double tilde)
        let content = STRIKETHROUGH_RE.replace_all(&content, |capts: &regex::Captures| {
            trace!("Found strikethrough block in '{}'", content);
            format!("<s>{}</s>", &capts[1])
        });

        // Emoji (unicode)
        let content = EMOJI_RE.replace_all(&content, |capts: &regex::Captures| {
            trace!("Found emoji '{}' in '{}'", &capts[0], content);
            let emoji_name = &capts[1];
            let emoji_symbol = match gh_emoji::get(emoji_name) {
                Some(x) => x,
                None => emoji_name,
            };
            emoji_symbol.to_string()
        });

        let mut content: String = content.into();

        // Channel mentions
        while let Some(m) = CHANNEL_MENTION_RE.find(&content) {
            trace!("Found channel mention '{}' in '{}'", m.as_str(), &content);
            let cid: u64 = content[m.start() + 5..m.end() - 4].parse().unwrap();
            let name = self.channel_names.get(&cid).unwrap();
            content = content.replace(m.as_str(), &format!("<span class=mention>#{}</span>", name));
        }

        while let Some(m) = USER_MENTION_RE.find(&content) {
            trace!("Found user mention '{}' in '{}'", m.as_str(), &content);
            let uid: serenity::model::id::UserId = content[m.start() + 6..m.end() - 4]
                .parse::<u64>()
                .unwrap()
                .into();
            let member = self.members.get(&uid);
            content = content.replace(m.as_str(), {
                &format!(
                    "<span class=mention>@{}</span>",
                    if member.is_some() {
                        get_member_nick(&member.unwrap()).to_string()
                    } else {
                        ctx.http
                            .get_user(*uid.as_u64())
                            .await
                            .map(|x| x.name)
                            .unwrap_or(uid.as_u64().to_string())
                    }
                )
            });
        }
        content.into()
    }

    fn get_name_used<'a>(&'a self, user: &'a User) -> &'a str {
        trace!("Begin getting name for user {}", user.name);
        if self.members.keys().find(|x| *x == &user.id).is_none() {
            warn!("Message author found who is not a member of the channel");
            return user.name.as_str();
        }
        match self
            .members
            .values()
            .find(|member| member.user.id == user.id)
            .unwrap()
            .nick
        {
            Some(ref x) => x.as_str(),
            None => user.name.as_str(),
        }
    }

    fn get_highest_role<'a>(&self, guild: &'a PartialGuild, user: &User) -> Option<&'a Role> {
        trace!("Begin getting highest role for user {}", user.name);
        //if !channel_members_users.iter().find(|x| x.).is_some();
        if self.members.keys().find(|x| *x == &user.id).is_none() {
            warn!("Message author found who is not a member of the channel");
            return None;
        }
        let roles = match self
            .members
            .values()
            .find(|member| member.user.id == user.id)
        {
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
    }
}

fn get_avatar_url(author: &User) -> String {
    match author.avatar_url() {
        Some(x) => x,
        None => match author.discriminator % 5 {
            0 | 5 => "https://discordapp.com/assets/6debd47ed13483642cf09e832ed0bc1b.png".into(),
            1 | 6 => "https://discordapp.com/assets/322c936a8c8be1b803cd94861bdfa868.png".into(),
            2 | 7 => "https://discordapp.com/assets/dd4dbc0016779df1378e7812eabaa04d.png".into(),
            3 | 8 => "https://discordapp.com/assets/0e291f67c9274a1abdddeb3fd919cbaa.png".into(),
            4 | 9 => "https://discordapp.com/assets/1cbd08c76f8af6dddce02c5138971129.png".into(),
            _ => "".into(),
        },
    }
}

fn get_member_nick<'a>(member: &'a Member) -> &'a str {
    match member.nick {
        Some(ref x) => x.as_str(),
        None => member.user.name.as_str(),
    }
}
