use crate::Result;

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use once_cell::sync::Lazy;
use regex::Regex;
use serenity::model::channel::GuildChannel;
use serenity::model::channel::Message;
use serenity::model::guild::Member;
use serenity::model::guild::PartialGuild;
use serenity::model::guild::Role;
use serenity::model::id::ChannelId;
use serenity::model::id::UserId;
use serenity::model::user::User;
use serenity::prelude::Context;
use tracing::*;

const CORE_THEME_CSS: &str = include_str!("html_templates/core.css");
const DARK_THEME_CSS: &str = include_str!("html_templates/dark.css");
const LIGHT_THEME_CSS: &str = include_str!("html_templates/light.css");
const PREAMBLE_TEMPLATE: &str = include_str!("html_templates/preamble_template.liquid");
const POSTAMBLE_TEMPLATE: &str = include_str!("html_templates/postamble_template.liquid");
const MESSAGE_GROUP_TEMPLATE: &str = include_str!("html_templates/message_group.liquid");

const IMAGE_FILE_EXTS: &[&str] = &[".jpg", ".jpeg", ".JPG", ".JPEG", ".png", ".PNG", ".gif"];
const USE_DARK_MODE: bool = true;

static CUSTOM_EMOJI_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(\\?)&lt;(a?):(\w+):(\d+)&gt;").unwrap());
static BOLD_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"\*\*([^\*]+)\*\*").unwrap());
static UNDERLINE_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"__([^_]+)__").unwrap());
static ITALICS_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"\*([^\*]+)\*").unwrap());
static ITALICS_REGEX2: Lazy<Regex> = Lazy::new(|| Regex::new(r"_([^_>]+)_").unwrap());
static STRIKETHROUGH_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"~~([^~]+)~~").unwrap());
static CHANNEL_MENTION_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"&lt;#(\d+)&gt;").unwrap());
static USER_MENTION_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"&lt;@!?(\d+)&gt;").unwrap());
static URL_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new( r"(?:&lt;)?((?:(?:http|https|ftp)://)(?:\S+(?::\S*)?@)?(?:(?:(?:[1-9]\d?|1\d\d|2[01]\d|22[0-3])(?:\.(?:1?\d{1,2}|2[0-4]\d|25[0-5])){2}(?:\.(?:[0-9]\d?|1\d\d|2[0-4]\d|25[0-4]))|(?:(?:[a-z\u00a1-\uffff0-9]+-?)*[a-z\u00a1-\uffff0-9]+)(?:\.(?:[a-z\u00a1-\uffff0-9]+-?)*[a-z\u00a1-\uffff0-9]+)*(?:\.(?:[a-z\u00a1-\uffff]{2,})))|localhost)(?::\d{2,5})?(?:(/|\?|#)[^\s]*)?)(?:&gt;)?") .unwrap()
});
static QUOTE_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?:^|<br>)((?:&gt;[^<\n]*(?:<br>)?)+)").unwrap());

pub fn prebuild_regexes() {
    Lazy::force(&CUSTOM_EMOJI_REGEX);
    Lazy::force(&BOLD_REGEX);
    Lazy::force(&UNDERLINE_REGEX);
    Lazy::force(&ITALICS_REGEX);
    Lazy::force(&ITALICS_REGEX2);
    Lazy::force(&STRIKETHROUGH_REGEX);
    Lazy::force(&CHANNEL_MENTION_REGEX);
    Lazy::force(&USER_MENTION_REGEX);
    Lazy::force(&URL_REGEX);
    Lazy::force(&QUOTE_REGEX);

    trace!("Forced regexes");
}

#[instrument(skip_all)]
pub async fn write_html<P: AsRef<Path>>(
    ctx: &Context,
    guild: &PartialGuild,
    channel: &GuildChannel,
    messages: &[Message],
    path: P,
) -> Result<()> {
    trace!("Entered HTML generator");

    let liquid_parser = liquid::ParserBuilder::with_stdlib().build()?;
    let preamble_template = liquid_parser.parse(PREAMBLE_TEMPLATE)?;
    let postamble_template = liquid_parser.parse(POSTAMBLE_TEMPLATE)?;
    let message_group_template = liquid_parser.parse(MESSAGE_GROUP_TEMPLATE)?;

    let category_name = match channel.category_id {
        Some(x) => x.name(&ctx).await,
        None => None,
    };

    let liquid_objects = liquid::object!({
        "guild_name": &guild.name,
        "channel_name": &channel.name,
        "core_css": CORE_THEME_CSS,
        "theme_css": if USE_DARK_MODE {DARK_THEME_CSS} else {LIGHT_THEME_CSS},
        "guild_icon_url": guild.icon_url().unwrap_or_default(),
        "guild_icon_alt": get_acronym_from_str(guild.name.as_str()),
        "category_name": category_name.unwrap_or_default(),
        "channel_topic": channel.topic.as_deref().unwrap_or_default(),
    });

    let mut html = preamble_template.render(&liquid_objects)?;

    trace!("Generated preamble");

    let channels = guild.channels(&ctx).await?;

    let mut message_renderer = MessageRenderer::new(ctx, guild, channels).await;

    trace!("Begin generating message HTML");
    for (i, message) in messages.iter().enumerate() {
        let author = &message.author;

        let author_highest_role = message_renderer
            .get_highest_role_with_colour(&message.author, guild)
            .await;

        let content = message_renderer.render_message(message).await;

        let message_liquid_objects = liquid::object!({
            "author_avatar_url": author.face(),
            "author_username": author.name,
            "author_discriminator": format!("{:04}", author.discriminator),
            "author_user_id": author.id.0,
            "author_name_colour": format!(
                "rgb({}, {}, {})",
                author_highest_role.map(|x| x.colour.r()).unwrap_or(255),
                author_highest_role.map(|x| x.colour.g()).unwrap_or(255),
                author_highest_role.map(|x| x.colour.b()).unwrap_or(255),
                ),
            "author_nick": message_renderer.get_nickname(&message.author).await.unwrap_or(""),
            "message_timestamp": message.timestamp,
            "message_content": content,
            "message_id": message.id.0,
        });

        let message_group = message_group_template.render(&message_liquid_objects)?;
        html.push_str(&message_group);
        trace!("Generated message html {} / {}", i + 1, messages.len());
    }

    let postamble_liquid_objects = liquid::object!({
        "num_exported_messages": messages.len(),
    });

    html.push_str(
        postamble_template
            .render(&postamble_liquid_objects)?
            .as_str(),
    );

    trace!(path = ?path.as_ref(), "Writing html file");
    tokio::fs::write(path, html).await?;

    info!("HTML generation complete");

    Ok(())
}

struct MessageRenderer<'context> {
    channel_names: HashMap<u64, String>,
    members: HashMap<UserId, Option<Member>>,
    usernames: HashMap<UserId, Option<String>>,
    guild: &'context PartialGuild,
    ctx: &'context Context,
}

impl<'context> MessageRenderer<'context> {
    #[instrument(skip_all)]
    async fn new(
        ctx: &'context Context,
        guild: &'context PartialGuild,
        channels: HashMap<ChannelId, GuildChannel>,
    ) -> MessageRenderer<'context> {
        trace!("Begin getting channel names");

        let channel_names = channels
            .into_iter()
            .map(|(id, channel)| (id.0, channel.name))
            .collect();

        Self {
            channel_names,
            members: guild
                .members(&ctx, None, None)
                .await
                .expect("Failed to retrieve members")
                .into_iter()
                .map(|m| (m.user.id, Some(m)))
                .collect(),
            usernames: HashMap::new(),
            guild,
            ctx,
        }
    }

    #[instrument(skip_all)]
    async fn get_username_cached(&mut self, user_id: &UserId) -> Option<&str> {
        if let Some(m) = self.usernames.get(user_id) {
            // It is more obvious that this is safe with a match
            #[allow(clippy::manual_map)]
            match m {
                // SAFETY: These two borrows *are* mutually exclusive, and therefore tricking the
                // borrow checker here is fine.
                Some(x) => Some(unsafe { (*(x as *const String)).as_str() }),
                None => None,
            }
        } else {
            match self
                .ctx
                .http
                .get_user(*user_id.as_u64())
                .await
                .map(|x| x.name)
            {
                Ok(x) => {
                    self.usernames.insert(*user_id, Some(x));
                    self.usernames.get(user_id).and_then(Option::as_deref)
                }
                Err(e) => {
                    warn!(
                        ?user_id,
                        error = ?e,
                        "User id is not associated with a user",
                    );
                    self.usernames.insert(*user_id, None);
                    None
                }
            }
        }
    }

    #[instrument(skip_all)]
    async fn get_member_cached(&mut self, user_id: &UserId) -> Option<&Member> {
        match self.members.get(user_id) {
            Some(member) => {
                // It is more obvious that this is safe with a match
                #[allow(clippy::manual_map)]
                match member {
                    // SAFETY: These two borrows *are* mutually exclusive, and therefore tricking the
                    // borrow checker here is fine.
                    Some(x) => Some(unsafe { &*(x as *const _) }),
                    None => None,
                }
            }
            None => match self.guild.member(&self.ctx, user_id).await {
                Ok(x) => {
                    self.members.insert(*user_id, Some(x));
                    self.members.get(user_id).and_then(Option::as_ref)
                }
                Err(_) => {
                    warn!(%user_id, "User id not found in channel");
                    self.members.insert(*user_id, None);
                    None
                }
            },
        }
    }

    #[instrument(skip_all)]
    async fn render_message(&mut self, message: &Message) -> String {
        let content = message.content.as_str();
        trace!(%content, "Rendering message");
        let start = Instant::now();

        // Ampersands break things
        let content = content.replace('&', "&amp;");

        // Sanitise < and >
        let content = content.replace('<', "&lt;").replace('>', "&gt;");

        // HTML doesn't respect newlines, it needs <br>
        let content = content.replace('\n', "<br>");

        // Multiline code blocks
        let mut content = {
            // TODO Make this more unique, possibly random
            const URL_PLACEHOLDER: &str = "!!URL_PLACEHOLDER!!";

            // Maximum length of a discord message is 2000 characters. It is therefore unlikely that
            // a formatted message will exceed 8000 characters
            let mut out = String::with_capacity(8000 * std::mem::size_of::<char>());

            let mut urls = Vec::new();

            let code_block_strs = content.split("```").collect::<Vec<_>>();

            let no_code_block = code_block_strs.len() <= 2;

            for (c, whole_block) in code_block_strs.into_iter().enumerate() {
                let whole_block = if no_code_block {
                    strip_html_newlines(content.as_str())
                } else {
                    strip_html_newlines(whole_block)
                };
                if no_code_block || c & 1 == 0 {
                    // Not in a code block
                    if whole_block.is_empty() {
                        continue;
                    }
                    // Inline code blocks
                    let code_block_strs = whole_block.split('`').collect::<Vec<_>>();
                    let no_code_block = code_block_strs.len() <= 2;
                    for (c, block) in code_block_strs.into_iter().enumerate() {
                        let block = if no_code_block {
                            strip_html_newlines(whole_block)
                        } else {
                            strip_html_newlines(block)
                        };
                        if no_code_block || c & 1 == 0 {
                            // Not in a code block
                            if block.is_empty() {
                                continue;
                            }
                            let mut block = block.to_string();

                            // URLs
                            while let Some(m) = URL_REGEX.find(block.as_str()) {
                                trace!(url = %m.as_str(), "Found URL");
                                if m.as_str() == content
                                    && IMAGE_FILE_EXTS.iter().any(|x| m.as_str().ends_with(x))
                                {
                                    trace!("URL is an image embed");
                                    return format!(
                                        r#"<span class="chatlog__embed-image-container">
                                        <a href="{0:}" target="_blank">
                                        <img 
                                            class="chatlog__embed-image"
                                            title="{0:}"
                                            src="{0:}"
                                            alt="{0:}"
                                        />
                                        </a>
                                        </span><br>"#,
                                        m.as_str()
                                    );
                                }
                                let s = strip_html_newlines(m.as_str());
                                urls.push(s.to_owned());
                                block = block.replace(s, URL_PLACEHOLDER);
                            }

                            // Bold (double asterisk)
                            let block =
                                BOLD_REGEX.replace_all(&block, |capts: &regex::Captures| {
                                    trace!(contents = %&capts[1], "Found bold block");
                                    format!("<b>{}</b>", &capts[1])
                                });

                            // Underline (double underscore)
                            let block =
                                UNDERLINE_REGEX.replace_all(&block, |capts: &regex::Captures| {
                                    trace!(contents = %&capts[1], "Found underline block");
                                    format!("<u>{}</u>", &capts[1])
                                });

                            // Italics (single asterisk)
                            let block =
                                ITALICS_REGEX.replace_all(&block, |capts: &regex::Captures| {
                                    trace!(contents = %&capts[1], "Found italics block");
                                    format!("<i>{}</i>", &capts[1])
                                });

                            // Italics 2 (single underscore)
                            let block =
                                ITALICS_REGEX2.replace_all(&block, |capts: &regex::Captures| {
                                    trace!(contents = %&capts[1], "Found italics 2 block");
                                    format!("<i>{}</i>", &capts[1])
                                });

                            // Strikethrough (double tilde)
                            let block = STRIKETHROUGH_REGEX.replace_all(
                                &block,
                                |capts: &regex::Captures| {
                                    trace!(contents = %&capts[1], "Found strikethrough block");
                                    format!("<s>{}</s>", &capts[1])
                                },
                            );

                            // Custom (non-unicode) emoji
                            let block = CUSTOM_EMOJI_REGEX.replace_all(
                                &block,
                                |capts: &regex::Captures| {
                                    if &capts[1] == r"\" {
                                        return capts[0][1..capts[0].len()].replace(":", "&#58;");
                                    }
                                    let animated = &capts[2] == "a";
                                    let name = &capts[3];
                                    let id = &capts[4];
                                    trace!(%name, "Found custom emoji");
                                    let url = match animated {
                                        true => {
                                            format!("https://cdn.discordapp.com/emojis/{}.gif", id)
                                        }
                                        false => {
                                            format!("https://cdn.discordapp.com/emojis/{}.png", id)
                                        }
                                    };
                                    format!(
                                        r#"<img
                                           class="emoji"
                                           src="{0:}"
                                           alt="{1:}"
                                           title="{1:}"
                                           />"#,
                                        url, &capts[1]
                                    )
                                },
                            );
                            // Channel mentions
                            let block = CHANNEL_MENTION_REGEX.replace_all(
                                &block,
                                |capts: &regex::Captures| {
                                    let cid: u64 = capts[1].parse().unwrap();
                                    let name = self.channel_names.get(&cid);

                                    trace!(mention = %&capts[0], "Found channel mention");

                                    match name {
                                        Some(x) => {
                                            format!("<span class=mention>#{}</span>", x)
                                        }
                                        None => {
                                            warn!("Channel mentioned that does not exist");
                                            format!("<span class=mention>#{}</span>", cid)
                                        }
                                    }
                                },
                            );

                            // Quote blocks: (> )
                            let block =
                                QUOTE_REGEX.replace_all(&block, |capts: &regex::Captures| {
                                    trace!(contents = %&capts[0], "Found quote block");
                                    let s = capts[1][4..].replace("<br>&gt;", "<br>");
                                    format!("<div class=quote>{}</div>", s)
                                });

                            let mut block: String = block.into();

                            // User mentions
                            while let Some(m) = USER_MENTION_REGEX.find(&block) {
                                trace!(mention = m.as_str(), "Found user mention");
                                let uid: serenity::model::id::UserId = block
                                    [m.start() + 6..m.end() - 4]
                                    .parse::<u64>()
                                    .unwrap()
                                    .into();
                                let member = self.get_member_cached(&uid).await;
                                block = match member {
                                    Some(x) => block.replace(
                                        m.as_str(),
                                        &format!(
                                            "<span class=mention>@{}</span>",
                                            get_member_nick(x)
                                        ),
                                    ),
                                    None => {
                                        let name = self.get_username_cached(&uid).await;
                                        block.replace(
                                            m.as_str(),
                                            &match name {
                                                Some(name) => {
                                                    format!("<span class=mention>@{}</span>", name)
                                                }
                                                None => {
                                                    format!("<span class=mention>@{}</span>", uid)
                                                }
                                            },
                                        )
                                    }
                                }
                            }

                            // URLs part 2
                            for url in urls.iter() {
                                block = block.replacen(
                                    URL_PLACEHOLDER,
                                    format!(r#"<a href="{0}">{0}</a>"#, url).as_str(),
                                    1,
                                );
                            }

                            out.push_str(block.as_str());
                        } else {
                            // Inline code block
                            trace!(%block, "Found inline code block");
                            out.push_str(r#"<code class="pre pre--inline">"#);
                            out.push_str(block);
                            out.push_str("</code>");
                        }
                    }
                } else {
                    // In code block
                    trace!(%whole_block, "Found code block");
                    out.push_str(r#"<pre class="pre pre--multiline">"#);
                    let b = whole_block.find("<br>").and_then(|idx| {
                        if whole_block[..idx].chars().all(|c| c.is_ascii()) {
                            whole_block.get(idx + 4..)
                        } else {
                            None
                        }
                    });
                    match b {
                        Some(b) => out.push_str(b),
                        None => out.push_str(whole_block),
                    }
                    out.push_str(r#"</pre>"#);
                }
            }

            out
        };

        // Message attachments
        if !message.attachments.is_empty() {
            if !content.is_empty() {
                content.push_str("<br>");
            }
            for attachment in message.attachments.iter() {
                trace!(url = %attachment.url, "Found message attachment");
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

        let end = Instant::now();

        trace!(time_taken = ?(end - start).as_nanos(), "Rendered message");

        content
    }

    #[instrument(skip_all)]
    async fn get_nickname(&mut self, user: &User) -> Option<&str> {
        self.get_member_cached(&user.id)
            .await
            .and_then(|x| x.nick.as_deref())
    }

    #[instrument(skip_all)]
    async fn get_highest_role_with_colour(
        &mut self,
        user: &User,
        guild: &'context PartialGuild,
    ) -> Option<&'context Role> {
        let member = self.get_member_cached(&user.id).await?;

        let mut roles: Vec<_> = member
            .roles
            .iter()
            .flat_map(|roleid| guild.roles.get(roleid))
            .collect();
        roles.sort_unstable_by_key(|role| role.position);

        roles.iter().rev().find(|role| role.colour.0 != 0).copied()
    }
}

#[inline]
fn get_member_nick(member: &Member) -> &str {
    member
        .nick
        .as_deref()
        .unwrap_or_else(|| member.user.name.as_str())
}

fn get_acronym_from_str(string: &str) -> String {
    string.split(' ').filter_map(|x| x.chars().next()).collect()
}

fn strip_html_newlines(string: &str) -> &str {
    const HTML_NEWLINE: &str = "<br>";
    string
        .trim_start_matches(HTML_NEWLINE)
        .trim_end_matches(HTML_NEWLINE)
}
