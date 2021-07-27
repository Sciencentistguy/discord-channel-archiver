mod emoji;
mod file;
mod html;
mod json;

use std::env;
use std::io;
use std::io::Write;
use std::str::FromStr;

use eyre::Context as EyreContext;
use eyre::Result;
use log::*;
use once_cell::sync::Lazy;
use regex::Regex;
use serenity::async_trait;
use serenity::model::channel::GuildChannel;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::model::guild::Guild;
use serenity::model::id::ChannelId;
use serenity::prelude::*;
use structopt::StructOpt;

const USAGE_STRING: &str = r#"Invalid syntax.
Correct usage is `!archive <channel> [mode(s)]`, where `<channel>` is the channel you want to archive, and `[mode(s)]` is a possibly comma-separated list of modes.
Valid modes are: `json,html`. All modes are enabled if this parameter is omitted."#;

static COMMAND_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^!archive +<#(\d+)> *([\w,]+)?$").unwrap());
static OPTIONS: Lazy<Opt> = Lazy::new(Opt::from_args);

#[tokio::main]
async fn main() {
    // Set default log level to info unless otherwise specified.
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "discord_channel_archiver=info");
    }
    pretty_env_logger::init();

    let token = if let Some(ref token) = OPTIONS.token {
        token.to_string()
    } else if let Some(ref filename) = OPTIONS.token_filename {
        std::fs::read_to_string(filename).expect("File does not exist")
    } else if let Ok(token) = env::var("DISCORD_TOKEN") {
        token
    } else {
        eprintln!("Expected either --token, --token-filename, or a token in the environment");
        return;
    };

    println!("Token: {}", token);

    // Create a new instance of the Client, logging in as a bot. This will
    // automatically prepend your bot token with "Bot ", which is a requirement
    // by Discord for bot users.
    let mut client = Client::builder(&token)
        .event_handler(Handler)
        .await
        .expect("Err creating client");

    trace!("Created client.");

    // Finally, start a single shard, and start listening to events.
    //
    // Shards will automatically attempt to reconnect, and will perform
    // exponential backoff until it reconnects.
    if let Err(why) = client.start().await {
        error!("Client error: {:?}", why);
    }
}

async fn archive(
    ctx: &Context,
    channel: &GuildChannel,
    guild: &Guild,
    modes: ArchivalMode,
) -> Result<Vec<String>> {
    trace!("Begin downloading messages");
    let start = std::time::Instant::now();
    let messages = {
        let mut messages = channel.messages(&ctx, |r| r.limit(100)).await?;
        let mut x = 100;
        while x == 100 {
            let last_msg = messages.last().unwrap();
            let new_msgs = match channel
                .id
                .messages(&ctx, |retreiver| retreiver.before(last_msg.id).limit(100))
                .await
            {
                Ok(x) => x,
                Err(e) => {
                    warn!(
                        "Discord returned an error '{}'. Waiting 5 seconds before retrying",
                        e
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(5000)).await;
                    continue;
                }
            };
            x = new_msgs.len();
            messages.extend(new_msgs.into_iter());
            trace!(
                "Downloaded {} more messages, for a total of {} so far",
                x,
                messages.len()
            );
        }
        messages.reverse();
        messages
    };
    let end = std::time::Instant::now();
    trace!(
        "{} messages downloaded. Took {:.2}s",
        messages.len(),
        (end - start).as_secs_f64()
    );

    let output_filename = format!("{}/{}-{}", OPTIONS.output_path, guild.name, channel.name);

    let mut created_files: Vec<String> = Vec::new();
    if modes.json {
        let filename = format!("{}.json", output_filename);
        while let Err(x) = json::write_json(&ctx, &guild, &messages, &filename).await {
            error!("Failed to write json: {}", x);
            if !confirm("Retry?", true)? {
                break;
            }
        }
        created_files.push(filename);
    }

    if modes.html {
        let filename = format!("{}.html", output_filename);
        while let Err(x) = html::write_html(&ctx, &guild, &channel, &messages, &filename).await {
            error!("Failed to write html: {}", x);
            if !confirm("Retry?", true)? {
                break;
            }
        }
        created_files.push(filename);
    }

    info!("Archive complete.");
    Ok(created_files)
}

struct Handler;

#[derive(Debug)]
struct ArchivalMode {
    json: bool,
    html: bool,
}

impl std::fmt::Display for ArchivalMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "json: {}, html: {}", self.json, self.html)
    }
}

#[async_trait]
impl EventHandler for Handler {
    //async fn cache_ready(&self, ctx: Context, guilds: Vec<serenity::model::id::GuildId>) {
    //for g in guilds {
    //println!("guild: {:#?}", ctx.cache.guild(g).await);
    //}
    ////println!("ctx.cache: {:#?}, guilds: {:#?}", ctx.cache.guilds().await, guilds);
    //}

    async fn message(&self, ctx: Context, msg: Message) {
        if msg.content.starts_with("!archive") {
            if msg.content.starts_with("!archive_emoji") {
                emoji::archive_emoji(&ctx, &msg).await;
            } else {
                let capts = COMMAND_REGEX.captures(&msg.content);
                if capts.as_ref().and_then(|x| x.get(0)).is_none() {
                    msg.reply(&ctx, USAGE_STRING)
                        .await
                        .expect("Failed to reply to message.");
                    info!("Invalid archive command supplied: '{}'", &msg.content);
                    return;
                }
                let capts = capts.unwrap();
                let channel_id_str = &capts[1];
                let modes = match capts
                    .get(2)
                    .map(|x| x.as_str().split(',').collect::<Vec<_>>())
                {
                    Some(x) => ArchivalMode {
                        json: x.contains(&"json"),
                        html: x.contains(&"html"),
                    },
                    None => ArchivalMode {
                        json: true,
                        html: true,
                    },
                };
                trace!("Command parsed");

                let channel = match ChannelId::from_str(channel_id_str) {
                    Ok(x) => x,
                    Err(_) => {
                        msg.reply(&ctx, format!("Invalid channel id {}.", channel_id_str))
                            .await
                            .expect("Failed to reply to message");
                        return;
                    }
                }
                .to_channel(&ctx)
                .await
                .expect("Channel not found")
                .guild()
                .expect("Invalid channel type");

                let guild = match msg.guild_id {
                    Some(x) => ctx.cache.guild(x).await.unwrap(),
                    None => {
                        msg.reply(&ctx, "This bot must be used in a guild channel.")
                            .await
                            .expect("Failed to reply to message.");
                        error!("This bot must be used in a guild channel.");
                        return;
                    }
                };

                info!(
                    "Archive started by user '{}#{:04}' in guild '{}', in channel '{}', with modes '{}'",
                    msg.author.name,
                    msg.author.discriminator,
                    guild.name,
                    channel.name,
                    modes
                );

                let created_files = archive(&ctx, &channel, &guild, modes)
                    .await
                    .wrap_err(format!("Failed to archive channel '{}'", channel));

                let created_files = match created_files {
                    Ok(x) => x,
                    Err(e) => {
                        error!("{}", e);
                        return;
                    }
                };

                msg.reply(
                    &ctx,
                    format!(
                        "Done!\nCreated files:\n```\n{}\n```",
                        created_files.join("\n")
                    ),
                )
                .await
                .expect("Failed to reply to message.");
            }
        }
    }

    async fn ready(&self, _ctx: Context, ready: Ready) {
        info!(
            "Bot logged in with username {} to {} guilds!",
            ready.user.name,
            ready.guilds.len()
        );
    }
}

pub fn confirm(prompt: &str, default: bool) -> Result<bool> {
    let mut buf = String::new();
    loop {
        if default {
            print!("{} (Y/n) ", prompt);
        } else {
            print!("{} (y/N) ", prompt);
        }

        io::stdout().lock().flush()?;
        io::stdin()
            .read_line(&mut buf)
            .wrap_err("Failed to read response from stdin")?;
        buf.make_ascii_lowercase();

        match &*(buf.trim_end()) {
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            "" => return Ok(default),
            _ => println!("Invalid response."),
        }
    }
}

#[derive(StructOpt, Debug)]
#[structopt(
    name = "discord-channel-archiver",
    about = "A small discord bot to archive the messages in a discord text channel. Provide the token with either --token, --token-filename, or as the environment variable DISCORD_TOKEN, in order of decreasing priority."
)]
struct Opt {
    /// Provide the token
    #[structopt(short, long)]
    token: Option<String>,
    /// Provide the name of a file containing the token
    #[structopt(short = "f", long)]
    token_filename: Option<String>,
    /// The path to output files to
    #[structopt(default_value = "/dev/shm/")]
    output_path: String,
}
