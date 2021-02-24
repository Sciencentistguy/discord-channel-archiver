mod html;
mod json;

use std::env;
use std::str::FromStr;

use lazy_static::lazy_static;
use log::*;
use regex::Regex;
use serenity::{
    async_trait,
    model::{channel::Message, gateway::Ready, id::ChannelId},
    prelude::*,
};
use structopt::StructOpt;

static PATH: &str = "/dev/shm";

lazy_static! {
    static ref COMMAND_REGEX: Regex = Regex::new(r"^!archive +<#(\d+)> *([\w,]+)?$").unwrap();
}

#[tokio::main]
async fn main() {
    // Set default log level to info unless otherwise specified.
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "discord_channel_archiver=info");
    }
    pretty_env_logger::init();

    let opts = Opt::from_args();
    let token = if opts.token.is_some() {
        opts.token.unwrap()
    } else if opts.token_filename.is_some() {
        std::fs::read_to_string(opts.token_filename.unwrap()).expect("File does not exist")
    } else {
        env::var("DISCORD_TOKEN")
            .expect("Expected either --token, --token-filename, or a token in the environment")
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
    async fn message(&self, ctx: Context, msg: Message) {
        if msg.content.starts_with("!archive") {
            let capts = COMMAND_REGEX.captures(&msg.content);
            if capts.as_ref().map(|x| x.get(0)).is_none() {
                msg.reply(&ctx,
                    r#"Invalid syntax.
Correct usage is `!archive <channel> [mode(s)]`, where `<channel>` is the channel you want to archive, and `[mode(s)]` is a possibly comma-separated list of modes.
Valid modes are: `json,html`. All modes are enabled if this parameter is omitted."#).await.expect("Failed to reply to message.");
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
            let guild = channel.guild_id.to_partial_guild(&ctx).await.unwrap();

            info!(
                "Archive started by user '{}#{:04}' in guild '{}', in channel '{}', with modes '{}'",
                msg.author.name,
                msg.author.discriminator,
                guild.name,
                channel.name,
                modes
            );
            trace!("Begin downloading messages");
            let messages = {
                let mut messages: Vec<Message> = Vec::new();
                let mut x = 100;
                while x == 100 {
                    let last_msg = (&messages).last().unwrap_or(&msg);
                    let new_msgs = channel
                        .id
                        .messages(&ctx, |retreiver| retreiver.before(last_msg.id).limit(100))
                        .await
                        .expect("Failed getting messages");
                    x = new_msgs.len();
                    messages.extend(new_msgs.into_iter());
                }
                messages.reverse();
                messages
            };
            trace!("{} messages downloaded", messages.len());

            let output_filename = format!("{}/{}-{}", PATH, guild.name, channel.name);

            let mut created_files: Vec<String> = Vec::new();
            if modes.json {
                let filename = format!("{}.json", output_filename);
                match json::write_json(&messages, &filename, &ctx).await {
                    Ok(_) => {}
                    Err(x) => error!("Error writing json: {}", x),
                }
                created_files.push(filename);
            }

            if modes.html {
                let filename = format!("{}.html", output_filename);
                match html::write_html(&messages, &filename, &ctx).await {
                    Ok(_) => {}
                    Err(x) => error!("Error writing html: {}", x),
                }
                created_files.push(filename);
            }

            info!("Archive complete.");

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

    // Set a handler to be called on the `ready` event. This is called when a
    // shard is booted, and a READY payload is sent by Discord. This payload
    // contains data like the current user's guild Ids, current user data,
    // private channels, and more.
    //
    // In this case, just print what the current user's username is.
    async fn ready(&self, _: Context, ready: Ready) {
        info!("{} is connected!", ready.user.name);
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
}
