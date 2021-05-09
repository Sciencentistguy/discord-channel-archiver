use log::*;

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use lazy_static::lazy_static;

use serenity::model::channel::GuildChannel;
use serenity::model::channel::Message;
use serenity::model::guild::Guild;
use serenity::model::guild::Member;
use serenity::model::guild::Role;
use serenity::model::id::ChannelId;
use serenity::model::id::UserId;
use serenity::model::user::User;
use serenity::prelude::Context;

use regex::Regex;

const CORE_THEME_CSS: &str = include_str!("html_templates/core.css");
const DARK_THEME_CSS: &str = include_str!("html_templates/dark.css");
const LIGHT_THEME_CSS: &str = include_str!("html_templates/light.css");
const IMAGE_FILE_EXTS: [&str; 7] = [".jpg", ".jpeg", ".JPG", ".JPEG", ".png", ".PNG", ".gif"];

const USE_DARK_MODE: bool = true;

lazy_static! {
    static ref CUSTOM_EMOJI_REGEX: Regex = Regex::new(r"(\\?)&lt;(a?):(\w+):(\d+)&gt;").unwrap();
    static ref INLINE_CODE_REGEX: Regex = Regex::new(r"`([^`]*)`").unwrap();
    static ref CODE_BLOCK_LANGUAGE_TAG_REGEX: Regex = Regex::new(r"^\w*<br>").unwrap();
    static ref BOLD_REGEX: Regex = Regex::new(r"\*\*([^\*]+)\*\*").unwrap();
    static ref UNDERLINE_REGEX: Regex = Regex::new(r"__([^_]+)__").unwrap();
    static ref ITALICS_REGEX: Regex = Regex::new(r"\*([^\*]+)\*").unwrap();
    static ref ITALICS_REGEX2: Regex = Regex::new(r"_([^_>]+)_").unwrap();
    static ref STRIKETHROUGH_REGEX: Regex = Regex::new(r"~~([^~]+)~~").unwrap();
    static ref EMOJI_REGEX: Regex = Regex::new(r":(\w+):").unwrap();
    static ref CHANNEL_MENTION_REGEX: Regex = Regex::new(r"&lt;#(\d+)&gt;").unwrap();
    static ref USER_MENTION_REGEX: Regex = Regex::new(r"&lt;@!?(\d+)&gt;").unwrap();
    static ref USER_MENTION_UNSANITISED_REGEX: Regex = Regex::new(r"<@!(\d+)>").unwrap();
    static ref URL_REGEX: Regex = Regex::new(
        r"(?:&lt;)?((?:(?:http|https|ftp)://)(?:\S+(?::\S*)?@)?(?:(?:(?:[1-9]\d?|1\d\d|2[01]\d|22[0-3])(?:\.(?:1?\d{1,2}|2[0-4]\d|25[0-5])){2}(?:\.(?:[0-9]\d?|1\d\d|2[0-4]\d|25[0-4]))|(?:(?:[a-z\u00a1-\uffff0-9]+-?)*[a-z\u00a1-\uffff0-9]+)(?:\.(?:[a-z\u00a1-\uffff0-9]+-?)*[a-z\u00a1-\uffff0-9]+)*(?:\.(?:[a-z\u00a1-\uffff]{2,})))|localhost)(?::\d{2,5})?(?:(/|\?|#)[^\s]*)?)(?:&gt;)?"
    )
    .unwrap();
    static ref QUOTE_REGEX: Regex = Regex::new(r"(?:^|<br>)((?:&gt;[^<\n]*(?:<br>)?)+)").unwrap();
}

pub async fn write_html<P: AsRef<Path>>(
    ctx: &Context,
    guild: &Guild,
    channel: &GuildChannel,
    messages: &[Message],
    path: P,
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

    let channels = guild.channels(&ctx).await?;

    let mut message_renderer = MessageRenderer::new(&ctx, &guild, channels)?;

    trace!("Begin saving messages");
    for (i, message) in messages.iter().enumerate() {
        let author = &message.author;
        let author_nick_or_user = message_renderer
            .get_name_used(&message.author)
            .await
            .to_owned();
        let author_highest_role = message_renderer
            .get_highest_role(&message.author, guild)
            .await;

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
            author.id.0,
            author_highest_role.map(|x| x.colour.r()).unwrap_or(255),
            author_highest_role.map(|x| x.colour.g()).unwrap_or(255),
            author_highest_role.map(|x| x.colour.b()).unwrap_or(255),
            author_nick_or_user,
        );

        let content = message_renderer.render_message(&message).await;

        let message_group = format!(
            r#"<div class="chatlog__message-group">
    <div class="chatlog__author-avatar-container">
        <img class="chatlog__author-avatar" src="{}" alt="Avatar" title="Avatar" />
    </div>
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
            author.face(),
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

    trace!("Writing html file {:?}", path.as_ref());
    fs::write(path, html)?;

    info!("HTML generation complete.");

    Ok(())
}

struct MessageRenderer<'context> {
    channel_names: HashMap<u64, String>,
    members: HashMap<UserId, Option<Member>>,
    usernames: HashMap<UserId, Option<String>>,
    guild: &'context Guild,
    ctx: &'context Context,
}

impl<'context> MessageRenderer<'context> {
    fn new(
        ctx: &'context Context,
        guild: &'context Guild,
        mut channels: HashMap<ChannelId, GuildChannel>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        trace!("Begin getting channel names");
        let mut channel_names = HashMap::new();
        for (id, channel) in channels.iter_mut() {
            channel_names.insert(*id.as_u64(), std::mem::take(&mut channel.name));
        }

        Ok(Self {
            channel_names,
            members: guild
                .members
                .iter()
                .map(|(&k, v)| (k, Some(v.clone())))
                .collect(),
            usernames: HashMap::new(),
            guild,
            ctx,
        })
    }

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
                    self.usernames.get(&user_id).unwrap().as_deref()
                }
                Err(_) => {
                    warn!("User id '{}' is not associated with a user.", user_id);
                    self.usernames.insert(*user_id, None);
                    None
                }
            }
        }
    }

    async fn get_member_cached(&mut self, user_id: &UserId) -> Option<&Member> {
        if let Some(m) = self.members.get(user_id) {
            // It is more obvious that this is safe with a match
            #[allow(clippy::manual_map)]
            match m {
                // SAFETY: These two borrows *are* mutually exclusive, and therefore tricking the
                // borrow checker here is fine.
                Some(x) => Some(unsafe { &*(x as *const _) }),
                None => None,
            }
        } else {
            match self.guild.member(&self.ctx, user_id).await {
                Ok(x) => {
                    self.members.insert(*user_id, Some(x));
                    self.members.get(&user_id).unwrap().as_ref()
                }
                Err(_) => {
                    warn!("User with id '{}' not found in channel.", user_id);
                    self.members.insert(*user_id, None);
                    None
                }
            }
        }
    }

    async fn render_message(&mut self, message: &serenity::model::channel::Message) -> String {
        let content = message.content.as_str();
        trace!("Rendering message:\n{}.", content);
        let start = std::time::Instant::now();

        // Ampersands break things
        let content = content.replace('&', "&amp;");

        // Sanitise < and >
        let content = content.replace('<', "&lt;").replace('>', "&gt;");

        // HTML doesn't respect newlines, it needs <br>
        let content = content.replace('\n', "<br>");

        // Multiline code blocks
        let mut content = {
            // Maximum length of a discord message is 2000 characters. It is therefore unlikely that
            // a formatted message will exceed 8000 characters
            let mut out = String::with_capacity(8000 * std::mem::size_of::<char>());

            let mut urls = Vec::new();

            let split = content.split("```");
            for (c, block) in split.enumerate() {
                if c & 1 == 0 {
                    // Inline code blocks
                    let block = {
                        let mut out = String::with_capacity(8000 * std::mem::size_of::<char>());
                        let split = block.split('`');
                        for (c, block) in split.enumerate() {
                            if c & 1 == 0 {
                                let mut block = block.to_string();

                                // URLs
                                while let Some(m) = URL_REGEX.find(block.as_str()) {
                                    trace!("Found URL '{}' in '{}'", m.as_str(), content);
                                    if m.as_str() == content
                                        && IMAGE_FILE_EXTS.iter().any(|x| m.as_str().ends_with(x))
                                    {
                                        trace!("Found image embed '{}'.", m.as_str());
                                        return format!(
                                            r#"<span class="chatlog__embed-image-container">
    <a href="{0:}" target="_blank">
        <img class="chatlog__embed-image" title="{0:}", src="{0:}" alt="{0:}"/>
    </a>
</span><br>"#,
                                            m.as_str()
                                        );
                                    }
                                    let s = if m.as_str().contains("<br>") {
                                        &m.as_str()[..m.as_str().find('<').unwrap()]
                                    } else {
                                        m.as_str()
                                    };
                                    urls.push(s.to_owned());
                                    block = block.replace(s, "!!URL!!");
                                }

                                // Bold (double asterisk)
                                let block =
                                    BOLD_REGEX.replace_all(&block, |capts: &regex::Captures| {
                                        trace!("Found bold block in '{}'", block);
                                        format!("<b>{}</b>", &capts[1])
                                    });

                                // Underline (double underscore)
                                let block = UNDERLINE_REGEX.replace_all(
                                    &block,
                                    |capts: &regex::Captures| {
                                        trace!("Found underline block in '{}'", block);
                                        format!("<u>{}</u>", &capts[1])
                                    },
                                );

                                // Italics (single asterisk)
                                let block =
                                    ITALICS_REGEX.replace_all(&block, |capts: &regex::Captures| {
                                        trace!("Found italics block in '{}'", block);
                                        format!("<i>{}</i>", &capts[1])
                                    });

                                // Italics (single underscore)
                                let block = ITALICS_REGEX2.replace_all(
                                    &block,
                                    |capts: &regex::Captures| {
                                        trace!("Found italics 2 block in '{}'", block);
                                        format!("<i>{}</i>", &capts[1])
                                    },
                                );

                                // Strikethrough (double tilde)
                                let block = STRIKETHROUGH_REGEX.replace_all(
                                    &block,
                                    |capts: &regex::Captures| {
                                        trace!("Found strikethrough block in '{}'", block);
                                        format!("<s>{}</s>", &capts[1])
                                    },
                                );

                                // Custom (non-unicode) emoji
                                let block = CUSTOM_EMOJI_REGEX.replace_all(&block, |capts: &regex::Captures| {
                                    if &capts[1] == r"\" {
                                        return capts[0][1..capts[0].len()].replace(":", "&#58;");
                                    }

                                    let animated = &capts[2] == "a";
                                    let name = &capts[3];
                                    let id = &capts[4];

                                    trace!("Found custom emoji '{}' in '{}'", name, block);
                                    let url = match animated {
                                        true => format!("https://cdn.discordapp.com/emojis/{}.gif", id),
                                        false => format!("https://cdn.discordapp.com/emojis/{}.png", id),
                                    };

                                    format!(
                                        r#"<img class="emoji" src="{0:}" alt="{1:}" title="{1:}"/>"#,
                                        url, &capts[1]
                                    )
                                });

                                // Channel mentions
                                let block = CHANNEL_MENTION_REGEX.replace_all(
                                    &block,
                                    |capts: &regex::Captures| {
                                        trace!(
                                            "Found channel mention '{}' in '{}'",
                                            &capts[0],
                                            &block
                                        );
                                        let cid: u64 = capts[1].parse().unwrap();
                                        let name = self.channel_names.get(&cid);
                                        match name {
                                            Some(x) => format!("<span class=mention>#{}</span>", x),
                                            None => {
                                                warn!("Channel mentioned that does not exist");
                                                format!("<span class=mention>#{}</span>", cid)
                                            }
                                        }
                                    },
                                );

                                // Quote blocks
                                let block =
                                    QUOTE_REGEX.replace_all(&block, |capts: &regex::Captures| {
                                        trace!("Found quote block '{}' in '{}'", &capts[0], &block);
                                        let s = capts[1][4..].replace("<br>&gt;", "<br>");
                                        format!("<div class=quote>{}</div>", s)
                                    });

                                let mut block: String = block.into();

                                // User mentions
                                while let Some(m) = USER_MENTION_REGEX.find(&block) {
                                    trace!("Found user mention '{}' in '{}'", m.as_str(), &block);
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
                                                        format!(
                                                            "<span class=mention>@{}</span>",
                                                            name
                                                        )
                                                    }
                                                    None => format!(
                                                        "<span class=mention>@{}</span>",
                                                        uid
                                                    ),
                                                },
                                            )
                                        }
                                    }
                                }

                                // URLs part 2
                                for url in urls.iter() {
                                    block = block.replacen(
                                        "!!URL!!",
                                        format!(r#"<a href="{0}">{0}</a>"#, url).as_str(),
                                        1,
                                    );
                                }

                                out.push_str(block.as_str());
                            } else {
                                trace!("Found inline code block '{}' in '{}'", block, content);
                                out.push_str(r#"<code class="pre pre--inline">"#);
                                out.push_str(block);
                                out.push_str("</code>");
                            }
                        }
                        out
                    };

                    out.push_str(block.as_str());
                } else {
                    trace!("Found multiline code block '{}' in '{}'", block, content);
                    out.push_str(r#"<pre class="pre pre--multiline">"#);
                    out.push_str(CODE_BLOCK_LANGUAGE_TAG_REGEX.replace(block, "").as_ref()); // TODO syntax highlighting with this
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

    async fn get_name_used<'a>(&'a mut self, user: &'a User) -> &'a str {
        match self
            .get_member_cached(&user.id)
            .await
            .and_then(|ref x| x.nick.as_ref())
        {
            Some(x) => x.as_str(),
            None => user.name.as_str(),
        }
    }

    async fn get_highest_role(
        &mut self,
        user: &User,
        guild: &'context Guild,
    ) -> Option<&'context Role> {
        let member = match self.get_member_cached(&user.id).await {
            Some(x) => x,
            None => return None,
        };

        let mut roles: Vec<_> = member
            .roles
            .iter()
            .map(|roleid| guild.roles.get(roleid).unwrap())
            .collect();
        roles.sort_unstable_by_key(|role| role.position);
        roles.last().copied()
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
    let mut out = String::with_capacity(string.chars().filter(|&x| x == ' ').count() * 2);
    for word in string.split(' ') {
        out.push(match word.chars().next() {
            Some(x) => x,
            None => {
                break;
            }
        })
    }

    out
}
