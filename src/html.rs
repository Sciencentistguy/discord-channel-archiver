use log::*;

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use lazy_static::lazy_static;

use serenity::model::channel::GuildChannel;
use serenity::model::channel::Message;
use serenity::model::guild::Member;
use serenity::model::guild::PartialGuild;
use serenity::model::guild::Role;
use serenity::model::id::UserId;
use serenity::model::user::User;
use serenity::prelude::Context;

use regex::Regex;

use futures::future::join_all;

const CORE_THEME_CSS: &str = include_str!("html_templates/core.css");
const DARK_THEME_CSS: &str = include_str!("html_templates/dark.css");
const LIGHT_THEME_CSS: &str = include_str!("html_templates/light.css");
const IMAGE_FILE_EXTS: [&str; 7] = [".jpg", ".jpeg", ".JPG", ".JPEG", ".png", ".PNG", ".gif"];

const USE_DARK_MODE: bool = true;

lazy_static! {
    static ref CUSTOM_EMOJI_REGEX: Regex = Regex::new(r"(\\?)&lt;(a?):(\w+):(\d+)&gt;").unwrap();
    static ref INLINE_CODE_REGEX: Regex = Regex::new(r"`([^`]*)`").unwrap();
    static ref BOLD_REGEX: Regex = Regex::new(r"\*\*([^\*]+)\*\*").unwrap();
    static ref UNDERLINE_REGEX: Regex = Regex::new(r"__([^_]+)__").unwrap();
    static ref ITALICS_REGEX: Regex = Regex::new(r"\*([^\*]+)\*").unwrap();
    static ref ITALICS_REGEX2: Regex = Regex::new(r"_([^_>]+)_").unwrap();
    static ref STRIKETHROUGH_REGEX: Regex = Regex::new(r"~~([^~]+)~~").unwrap();
    static ref EMOJI_REGEX: Regex = Regex::new(r":(\w+):").unwrap();
    static ref CHANNEL_MENTION_REGEX: Regex = Regex::new(r"&lt;#(\d+)&gt;").unwrap();
    static ref USER_MENTION_REGEX: Regex = Regex::new(r"&lt;@!(\d+)&gt;").unwrap();
    static ref USER_MENTION_UNSANITISED_REGEX: Regex = Regex::new(r"<@!(\d+)>").unwrap();
    static ref URL_REGEX: Regex = Regex::new(
        r"(?:&lt;)?((?:(?:http|https|ftp)://)(?:\S+(?::\S*)?@)?(?:(?:(?:[1-9]\d?|1\d\d|2[01]\d|22[0-3])(?:\.(?:1?\d{1,2}|2[0-4]\d|25[0-5])){2}(?:\.(?:[0-9]\d?|1\d\d|2[0-4]\d|25[0-4]))|(?:(?:[a-z\u00a1-\uffff0-9]+-?)*[a-z\u00a1-\uffff0-9]+)(?:\.(?:[a-z\u00a1-\uffff0-9]+-?)*[a-z\u00a1-\uffff0-9]+)*(?:\.(?:[a-z\u00a1-\uffff]{2,})))|localhost)(?::\d{2,5})?(?:(/|\?|#)[^\s]*)?)(?:&gt;)?"
    )
    .unwrap();
    static ref QUOTE_REGEX: Regex = Regex::new(r"(?:^|<br>)((?:&gt;[^<\n]*(?:<br>)?)+)").unwrap();
}

pub async fn write_html<P: AsRef<Path>>(
    messages: &[Message],
    guild: &PartialGuild,
    channel: &GuildChannel,
    path: P,
    ctx: &Context,
) -> Result<(), Box<dyn std::error::Error>> {
    trace!("Entered HTML generator.");
    let html = include_str!("html_templates/preamble_template.html");
    let html = html.replace("DISCORD_GUILD_NAME", &guild.name);
    let html = html.replace("DISCORD_CHANNEL_NAME", &channel.name);

    let html = html.replace("CORE_STYLESHEET", CORE_THEME_CSS);

    let html = if USE_DARK_MODE {
        html.replace("THEME_STYLESHEET", DARK_THEME_CSS)
    } else {
        html.replace("THEME_STYLESHEET", LIGHT_THEME_CSS)
    };

    let html = html.replace(
        "GUILD_ICON",
        format!(
            r#" <img
 class="preamble__guild-icon"
 src="{}"
 alt="{}"
 />"#,
            guild.icon_url().unwrap_or_else(|| "".into()),
            get_acronym_from_str(guild.name.as_str()),
        )
        .as_str(),
    );

    let category_name = match channel.category_id {
        Some(x) => x.name(&ctx).await,
        None => None,
    };

    let html = if category_name.is_some() {
        html.replace(
            "DISCORD_CHANNEL_CATEGORY_SLASH_NAME",
            format!("{} / {}", category_name.unwrap(), channel.name).as_str(),
        )
    } else {
        html.replace("DISCORD_CHANNEL_CATEGORY_SLASH_NAME", channel.name.as_str())
    };

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

        let content = message_renderer.render_message(&message, &ctx).await;

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
                <div class="markdown">
                    {}
                </div>
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
        trace!("Archived message {} / {}", i + 1, messages.len());
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

    async fn render_message(
        &self,
        message: &serenity::model::channel::Message,
        ctx: &Context,
    ) -> String {
        let content = message.content.as_str();
        trace!("Rendering message:\n{}.", content);
        let start = std::time::Instant::now();
        //TODO don't render mardown inside code blocks.

        // Sanitise < and >
        let content = content.replace("<", "&lt;").replace(">", "&gt;");

        // HTML doesn't respect newlines, it needs <br>
        let content = content.replace("\n", "<br>");

        // URLs
        let content = URL_REGEX.replace_all(&content, |capts: &regex::Captures| {
            trace!("Found URL '{}' in '{}'", &capts[1], &content);
            if capts[0] == content && IMAGE_FILE_EXTS.iter().any(|x| capts[1].ends_with(x)) {
                format!(
                    r#"<span class="chatlog__embed-image-container">
    <a href="{0:}" target="_blank">
        <img class="chatlog__embed-image" title="{0:}", src="{0:}" alt="{0:}"/>
    </a>
</span><br>"#,
                    &capts[1]
                )
            } else {
                format!(r#"<a href="{0}">{0}</a>"#, &capts[1])
            }
        });

        // Custom (non-unicode) emoji
        let content = CUSTOM_EMOJI_REGEX.replace_all(&content, |capts: &regex::Captures| {
            if &capts[1] == r"\" {
                return capts[0][1..capts[0].len()].replace(":", "&#58;");
            }

            let animated = &capts[2] == "a";
            let name = &capts[3];
            let id = &capts[4];

            trace!("Found custom emoji '{}' in '{}'", name, content);
            let url = match animated {
                true => format!("https://cdn.discordapp.com/emojis/{}.gif", id),
                false => format!("https://cdn.discordapp.com/emojis/{}.png", id),
            };

            format!(
                r#"<img class="emoji" src="{0:}" alt="{1:}" title="{1:}"/>"#,
                url, &capts[1]
            )
        });

        // Code blocks
        let content = {
            let content: String = content.replace("```<br>", "```");

            let mut out = String::new();
            out.reserve(2000 * std::mem::size_of::<char>()); // Maximum length of a discord message is 2000 characters.
            let split = content.split("```");
            for (c, block) in split.enumerate() {
                if c & 1 == 0 {
                    out.push_str(block);
                } else {
                    trace!("Found multiline code block '{}' in '{}'", block, content);
                    out.push_str(r#"<pre class="pre pre--multiline">"#);
                    out.push_str(block);
                    out.push_str(r#"</pre>"#);
                }
            }
            out
        };

        // Inline code blocks
        let content = INLINE_CODE_REGEX.replace_all(&content, |capts: &regex::Captures| {
            trace!("Found inline code block in '{}'", content);
            format!(r#"<code class="pre pre--inline">{}</code>"#, &capts[1])
        });

        // Bold (double asterisk)
        let content = BOLD_REGEX.replace_all(&content, |capts: &regex::Captures| {
            trace!("Found bold block in '{}'", content);
            format!("<b>{}</b>", &capts[1])
        });

        // Underline (double underscore)
        let content = UNDERLINE_REGEX.replace_all(&content, |capts: &regex::Captures| {
            trace!("Found underline block in '{}'", content);
            format!("<u>{}</u>", &capts[1])
        });

        // Italics (single asterisk)
        let content = ITALICS_REGEX.replace_all(&content, |capts: &regex::Captures| {
            trace!("Found italics block in '{}'", content);
            format!("<i>{}</i>", &capts[1])
        });

        // Italics (single underscore)
        let content = ITALICS_REGEX2.replace_all(&content, |capts: &regex::Captures| {
            trace!("Found italics 2 block in '{}'", content);
            format!("<i>{}</i>", &capts[1])
        });

        // Strikethrough (double tilde)
        let content = STRIKETHROUGH_REGEX.replace_all(&content, |capts: &regex::Captures| {
            trace!("Found strikethrough block in '{}'", content);
            format!("<s>{}</s>", &capts[1])
        });

        // Emoji (unicode)
        let content = EMOJI_REGEX.replace_all(&content, |capts: &regex::Captures| {
            trace!("Found emoji '{}' in '{}'", &capts[0], content);
            let emoji_name = &capts[1];
            let emoji_symbol = match gh_emoji::get(emoji_name) {
                Some(x) => x,
                None => emoji_name,
            };
            emoji_symbol.to_string()
        });

        // Channel mentions
        let content = CHANNEL_MENTION_REGEX.replace_all(&content, |capts: &regex::Captures| {
            trace!("Found channel mention '{}' in '{}'", &capts[0], &content);
            let cid: u64 = capts[1].parse().unwrap();
            let name = self.channel_names.get(&cid).unwrap();
            format!("<span class=mention>#{}</span>", name)
        });

        // Quote blocks
        let content = QUOTE_REGEX.replace_all(&content, |capts: &regex::Captures| {
            trace!("Found quote block '{}' in '{}'", &capts[0], &content);
            let s = capts[1][4..].replace("<br>&gt;", "<br>");
            format!("<div class=quote>{}</div>", s)
        });

        let mut content: String = content.into();

        // User mentions
        while let Some(m) = USER_MENTION_REGEX.find(&content) {
            trace!("Found user mention '{}' in '{}'", m.as_str(), &content);
            let uid: serenity::model::id::UserId = content[m.start() + 6..m.end() - 4]
                .parse::<u64>()
                .unwrap()
                .into();
            let member = self.members.get(&uid);
            content = if member.is_some() {
                content.replace(m.as_str(), {
                    &format!(
                        "<span class=mention>@{}</span>",
                        get_member_nick(&member.unwrap())
                    )
                })
            } else {
                warn!("User mentioned that's not a member of the channel");
                content.replace(
                    m.as_str(),
                    &format!(
                        "<span class=mention>@{}</span>",
                        ctx.http
                            .get_user(*uid.as_u64())
                            .await // TODO make this synchronous somehow
                            .map(|x| x.name)
                            .unwrap_or_else(|_| uid.as_u64().to_string())
                    ),
                )
            }
        }

        // Message attachments
        if !message.attachments.is_empty() {
            if content != "" {
                content.push_str("<br>");
            }
            for attachment in message.attachments.iter() {
                trace!(
                    "Found message attachment '{}' in message '{}'",
                    attachment.url,
                    message.content
                );
                if IMAGE_FILE_EXTS.iter().any(|x| attachment.url.ends_with(x)) {
                    content.push_str(&format!(
                        r#"<span class="chatlog__embed-image-container">
    <a href="{0:}" target="_blank">
        <img class="chatlog__embed-image" title="{0:}", src="{0:}" alt="{0:}"/>
    </a>
</span><br>"#,
                        attachment.url
                    ));
                } else {
                    content.push_str(&format!(r#"<a href="{0}">{0}</a><br>"#, attachment.url));
                }
            }
        }

        let end = std::time::Instant::now();

        trace!("Rendered message. Took {}ns", (end - start).as_nanos());

        // This is either needed or not needed depending on what the last rendering step is
        #[allow(clippy::useless_conversion)]
        content.into()
    }

    fn get_name_used<'a>(&'a self, user: &'a User) -> &'a str {
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

fn get_member_nick(member: &Member) -> &str {
    match member.nick {
        Some(ref x) => x.as_str(),
        None => member.user.name.as_str(),
    }
}

fn get_acronym_from_str(string: &str) -> String {
    let splitted = string.split(' ').collect::<Vec<_>>();
    let mut out = String::with_capacity(splitted.len());
    for word in splitted {
        out.push(match word.chars().next() {
            Some(x) => x,
            None => {
                break;
            }
        })
    }

    out
}
