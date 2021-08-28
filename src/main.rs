mod emoji;
mod error;
mod file;
mod html;
mod json;

use std::path::PathBuf;
use std::str::FromStr;

use log::*;
use once_cell::sync::Lazy;
use regex::Regex;
use serenity::async_trait;
use serenity::model::channel::GuildChannel;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::model::guild::PartialGuild;
use serenity::model::id::ChannelId;
use serenity::model::interactions::application_command::ApplicationCommand;
use serenity::model::interactions::application_command::ApplicationCommandInteractionDataOptionValue;
use serenity::model::interactions::application_command::ApplicationCommandOptionType;
use serenity::model::interactions::Interaction;
use serenity::model::interactions::InteractionResponseType;
use serenity::prelude::*;
use structopt::StructOpt;

use crate::emoji::archive_emoji;

type Result<T> = std::result::Result<T, error::Error>;

const USAGE_STRING: &str = "Invalid syntax.\n Correct usage is `!archive <channel> [mode]`,\
                            where `channel` is the channel you want to archive, and `mode`\
                            is one of either `json` or `html`. If this is blank, or if is\
                            any other value, all output formats will be generated.";

const REPLY_FAILURE: &str = "Failed to reply to message";

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

    let token = std::fs::read_to_string(&OPTIONS.token_filename).expect("File does not exist");
    let application_id = std::fs::read_to_string(&OPTIONS.appid_filename)
        .expect("File does not exist")
        .trim()
        .parse::<u64>()
        .expect("Invalid application_id");

    trace!("Token: {}", token);

    // Create a new instance of the Client, logging in as a bot. This will
    // automatically prepend your bot token with "Bot ", which is a requirement
    // by Discord for bot users.
    let mut client = Client::builder(&token)
        .event_handler(Handler)
        .application_id(application_id)
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

struct ArchiveLog {
    download_time: std::time::Duration,
    render_time: std::time::Duration,
    files_created: Vec<String>,
}

async fn archive(
    ctx: &Context,
    channel: &GuildChannel,
    guild: &PartialGuild,
    output_mode: ArchivalMode,
) -> Result<ArchiveLog> {
    trace!("Begin downloading messages");
    let start = std::time::Instant::now();
    let messages = {
        let mut messages = channel.messages(&ctx, |r| r.limit(100)).await?;
        trace!("Downloaded {} messages...", messages.len());
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
                        "While trying to download messages, \
                        Discord returned an error `{}`. Waiting 5 seconds before retrying",
                        e
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
            };
            x = new_msgs.len();
            messages.extend(new_msgs.into_iter());
            trace!("Downloaded {} messages...", messages.len());
        }
        messages.reverse();
        messages
    };
    let end = std::time::Instant::now();
    let download_time = end - start;
    info!(
        "Downloaded {} messages. Took {:.2}s",
        messages.len(),
        download_time.as_secs_f64()
    );
    let output_path = OPTIONS.output_path.to_string_lossy();
    let output_filename = format!(
        "{}{}{}-{}",
        output_path,
        if output_path.ends_with('/') { "" } else { "/" },
        guild.name.replace(char::is_whitespace, "_"),
        channel.name.replace(char::is_whitespace, "_"),
    );

    let mut files_created: Vec<String> = Vec::new();

    // XXX This is a litte ugly.
    let start = std::time::Instant::now();
    match output_mode {
        ArchivalMode::Json => {
            let filename = format!("{}.json", output_filename);
            json::write_json(ctx, guild, &messages, &filename).await?;
            files_created.push(filename);
        }
        ArchivalMode::Html => {
            let filename = format!("{}.html", output_filename);
            html::write_html(ctx, guild, channel, &messages, &filename).await?;
            files_created.push(filename);
        }
        ArchivalMode::All => {
            let filename = format!("{}.json", output_filename);
            json::write_json(ctx, guild, &messages, &filename).await?;
            files_created.push(filename);

            let filename = format!("{}.html", output_filename);
            html::write_html(ctx, guild, channel, &messages, &filename).await?;
            files_created.push(filename);
        }
    }
    let end = std::time::Instant::now();
    let render_time = end - start;

    info!("Archive complete.");
    Ok(ArchiveLog {
        download_time,
        render_time,
        files_created,
    })
}

struct Handler;

#[derive(Debug)]
enum ArchivalMode {
    Json,
    Html,
    All,
}

impl std::fmt::Display for ArchivalMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::ApplicationCommand(command) = interaction {
            let response = match command.data.name.as_str() {
                "archive_emoji" => {
                    // archive emoji
                    command
                        .create_interaction_response(&ctx, |reponse_builder| {
                            reponse_builder
                                .kind(InteractionResponseType::DeferredChannelMessageWithSource)
                        })
                        .await
                        .expect(REPLY_FAILURE);
                    match command.guild_id {
                        Some(guild_id) => {
                            let guild = guild_id.to_partial_guild(&ctx).await.unwrap();
                            let (n, output_path) = archive_emoji(guild).await;
                            format!(
                                "Archived {} emoji into `{}`",
                                n,
                                output_path.as_os_str().to_str().unwrap()
                            )
                        }
                        None => "This command must be used within a guild".to_owned(),
                    }
                }
                "archive" => {
                    // archive channel
                    command
                        .create_interaction_response(&ctx, |reponse_builder| {
                            reponse_builder
                                .kind(InteractionResponseType::DeferredChannelMessageWithSource)
                        })
                        .await
                        .expect(REPLY_FAILURE);

                    let channel = match command
                        .data
                        .options
                        .get(0)
                        .and_then(|o| o.resolved.as_ref())
                    {
                        Some(ApplicationCommandInteractionDataOptionValue::Channel(c)) => c,
                        _ => panic!("Expected channel as first argument"),
                    }
                    .id
                    .to_channel(&ctx)
                    .await
                    .expect("Failed to fetch channel");

                    let mode = match command
                        .data
                        .options
                        .get(1)
                        .and_then(|o| o.resolved.as_ref())
                    {
                        Some(ApplicationCommandInteractionDataOptionValue::String(s)) => {
                            match s.as_str() {
                                "json" => ArchivalMode::Json,
                                "html" => ArchivalMode::Html,
                                "all" => ArchivalMode::All,
                                _ => panic!("Invalid string choice"),
                            }
                        }
                        _ => panic!("Expected format as second argument"),
                    };

                    match channel.guild() {
                        Some(channel) => match command.guild_id {
                            Some(guild_id) => {
                                let guild = guild_id
                                    .to_partial_guild(&ctx)
                                    .await
                                    .expect("Failed to fetch guild");
                                match archive(&ctx, &channel, &guild, mode).await {
                                    Ok(archive_log) => {
                                        let download_seconds =
                                            archive_log.download_time.as_secs() % 60;
                                        let download_minutes =
                                            archive_log.download_time.as_secs() / 60;
                                        let render_seconds = archive_log.render_time.as_secs() % 60;
                                        let render_minutes = archive_log.render_time.as_secs() / 60;
                                        format!(
                                            "Done!\n\
                                            Downloading messages took {}m{:02}s\n\
                                            Rendering output took {}m{:02}s\n\
                                            Created files:\n\
                                            ```\n\
                                            {}\n\
                                            ```",
                                            &download_minutes,
                                            &download_seconds,
                                            &render_minutes,
                                            &render_seconds,
                                            archive_log.files_created.join("\n")
                                        )
                                    }
                                    Err(e) => {
                                        error!("{}", e);
                                        format!("Error!\n```\n{}\n```", e)
                                    }
                                }
                            }
                            None => "This command must be used within a guild".to_owned(),
                        },
                        None => "Error: Argument `channel` must be a text channel in this guild."
                            .to_owned(),
                    }
                }
                _ => "Error: Invalid command".to_owned(),
            };

            command
                .edit_original_interaction_response(&ctx, |edit_response_builder| {
                    edit_response_builder.content(response)
                })
                .await
                .expect(REPLY_FAILURE);
        }
    }

    async fn message(&self, ctx: Context, msg: Message) {
        if msg.content.starts_with("!archive") {
            if msg.content == "!archive_emoji" {
                let guild = match msg.guild_id {
                    Some(guild_id) => guild_id.to_partial_guild(&ctx).await.unwrap(),
                    None => {
                        msg.reply(&ctx, "This command must be used within a guild")
                            .await
                            .expect(REPLY_FAILURE);
                        return;
                    }
                };
                let (n, output_path) = emoji::archive_emoji(guild).await;
                msg.reply(
                    &ctx,
                    format!(
                        "Archived {} emoji into `{}`",
                        n,
                        output_path.as_os_str().to_str().unwrap()
                    ),
                )
                .await
                .expect(REPLY_FAILURE);
                return;
            } else {
                let capts = COMMAND_REGEX.captures(&msg.content);
                if capts.as_ref().and_then(|x| x.get(0)).is_none() {
                    msg.reply(&ctx, USAGE_STRING).await.expect(REPLY_FAILURE);
                    info!("Invalid archive command supplied: '{}'", &msg.content);
                    return;
                }
                let capts = capts.unwrap();
                let channel_id_str = &capts[1];
                let modes = match capts.get(2).map(|x| x.as_str()) {
                    Some("json") => ArchivalMode::Json,
                    Some("html") => ArchivalMode::Html,
                    _ => ArchivalMode::All,
                };
                trace!("Command parsed");

                let channel = match ChannelId::from_str(channel_id_str) {
                    Ok(x) => x,
                    Err(_) => {
                        msg.reply(&ctx, format!("Invalid channel id {}.", channel_id_str))
                            .await
                            .expect(REPLY_FAILURE);
                        return;
                    }
                }
                .to_channel(&ctx)
                .await
                .expect("Channel not found")
                .guild()
                .expect("Invalid channel type");

                let guild = match msg.guild_id {
                    Some(guild_id) => guild_id.to_partial_guild(&ctx).await.unwrap(),
                    None => {
                        msg.reply(&ctx, "This bot must be used in a guild channel.")
                            .await
                            .expect(REPLY_FAILURE);
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

                let archive_log = match archive(&ctx, &channel, &guild, modes).await {
                    Ok(x) => x,
                    Err(e) => {
                        error!("{}", e);
                        msg.reply(&ctx, format!("Error!\n```\n{}\n```", e))
                            .await
                            .expect(REPLY_FAILURE);
                        return;
                    }
                };

                let download_seconds = archive_log.download_time.as_secs() % 60;
                let download_minutes = archive_log.download_time.as_secs() / 60;
                let render_seconds = archive_log.render_time.as_secs() % 60;
                let render_minutes = archive_log.render_time.as_secs() / 60;
                let response = format!(
                    "Done!\n\
                    Downloading messages took {}m{:02}s\n\
                    Rendering output took {}m{:02}s\n\
                    Created files:\n\
                    ```\n\
                    {}\n\
                    ```",
                    &download_minutes,
                    &download_seconds,
                    &render_minutes,
                    &render_seconds,
                    archive_log.files_created.join("\n")
                );
                msg.reply(&ctx, response).await.expect(REPLY_FAILURE);
            }
        }
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        info!(
            "Bot logged in with username {} to {} guilds!",
            ready.user.name,
            ready.guilds.len()
        );

        let commands = ApplicationCommand::set_global_application_commands(&ctx, |builder| {
            builder
                .create_application_command(|command_builder| {
                    command_builder
                        .name("archive_emoji")
                        .description("Archive the emoji from the current server")
                })
                .create_application_command(|command_builder| {
                    command_builder
                        .name("archive")
                        .description("Archive the contents of a channel")
                        .create_option(|option_builder| {
                            option_builder
                                .name("channel")
                                .description("The channel to archive")
                                .kind(ApplicationCommandOptionType::Channel)
                                .required(true)
                        })
                        .create_option(|option_builder| {
                            option_builder
                                .name("output_format")
                                .description("The file format to output to")
                                .kind(ApplicationCommandOptionType::String)
                                .add_string_choice("JSON", "json")
                                .add_string_choice("HTML", "html")
                                .add_string_choice("all", "all")
                                .required(true)
                        })
                })
        })
        .await
        .unwrap();

        info!(
            "Registered the following slash commands: {:?}",
            commands
                .iter()
                .map(|cmd| cmd.name.as_str())
                .collect::<Vec<_>>()
        );
    }
}

#[derive(StructOpt, Debug)]
#[structopt(
    name = "discord-channel-archiver",
    about = "A small discord bot to archive the messages in a discord text channel. Provide the token with either --token, --token-filename, or as the environment variable DISCORD_TOKEN, in order of decreasing priority."
)]
struct Opt {
    /// File containing the token
    #[structopt()]
    token_filename: PathBuf,
    /// File containing the application id
    #[structopt()]
    appid_filename: PathBuf,
    /// The path to output files to
    #[structopt(default_value = "/dev/shm/")]
    output_path: PathBuf,
}
