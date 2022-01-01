mod emoji;
mod error;
mod file;
mod html;
mod json;

use std::path::PathBuf;
use std::str::FromStr;

use serenity::async_trait;
use serenity::model::channel::Channel;
use serenity::model::channel::GuildChannel;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::model::guild::PartialGuild;
use serenity::model::id::ChannelId;
use serenity::model::interactions::application_command::ApplicationCommand;
use serenity::model::interactions::application_command::ApplicationCommandInteraction;
use serenity::model::interactions::application_command::ApplicationCommandInteractionDataOptionValue;
use serenity::model::interactions::application_command::ApplicationCommandOptionType;
use serenity::model::interactions::Interaction;
use serenity::model::interactions::InteractionResponseType;
use serenity::prelude::*;

use clap::Parser;
use once_cell::sync::Lazy;
use regex::Regex;
use tracing::*;

use crate::emoji::archive_emoji;

type Result<T> = std::result::Result<T, error::Error>;

const USAGE_STRING: &str = "Invalid syntax.\n\
                            Correct usage is `!archive <channel> [mode]`, \
                            where `channel` is the channel you want to archive, and `mode` \
                            is one of either `json` or `html`. If this is blank, or if is \
                            any other value, all output formats will be generated.";

const REPLY_FAILURE: &str = "Failed to reply to message";

static COMMAND_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^!archive +<#(\d+)> *([\w,]+)?$").unwrap());

static OPTIONS: Lazy<Opt> = Lazy::new(Opt::parse);

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .pretty()
        .with_max_level(Level::INFO)
        .init();

    //pretty_env_logger::init();

    let token = tokio::fs::read_to_string(&OPTIONS.token_filename)
        .await
        .expect("File does not exist");
    let application_id = tokio::fs::read_to_string(&OPTIONS.appid_filename)
        .await
        .expect("File does not exist")
        .trim()
        .parse::<u64>()
        .expect("Invalid application_id");

    trace!(%token);

    tokio::spawn(async { html::prebuild_regexes() });

    // Create a new instance of the Client, logging in as a bot. This will
    // automatically prepend your bot token with "Bot ", which is a requirement
    // by Discord for bot users.
    let mut client = Client::builder(&token)
        .event_handler(Handler)
        .application_id(application_id)
        .await
        .expect("Err creating client");

    trace!(%token, "Created client");

    // Finally, start a single shard, and start listening to events.
    //
    // Shards will automatically attempt to reconnect, and will perform
    // exponential backoff until it reconnects.
    if let Err(why) = client.start().await {
        error!(error = ?why, "An error occurred in the client");
    }
}

struct ArchiveLog {
    download_time: std::time::Duration,
    render_time: std::time::Duration,
    files_created: Vec<PathBuf>,
}

async fn handle_slash_command(
    ctx: &Context,
    command: &ApplicationCommandInteraction,
) -> Result<String> {
    match command.data.name.as_str() {
        "archive_emoji" => {
            // archive emoji
            command
                .create_interaction_response(&ctx, |reponse_builder| {
                    reponse_builder.kind(InteractionResponseType::DeferredChannelMessageWithSource)
                })
                .await
                .expect(REPLY_FAILURE);

            match command.guild_id {
                Some(guild_id) => {
                    let guild = guild_id.to_partial_guild(&ctx).await?;
                    let (n, output_path) = archive_emoji(guild).await;
                    Ok(format!(
                        "Archived {} emoji into `{}`",
                        n,
                        output_path.as_os_str().to_str().unwrap()
                    ))
                }
                None => Err("This command must be used within a guild".to_owned().into()),
            }
        }
        "archive" => {
            // archive channel
            command
                .create_interaction_response(&ctx, |reponse_builder| {
                    reponse_builder.kind(InteractionResponseType::DeferredChannelMessageWithSource)
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
                _ => unreachable!("Expected channel as first argument"),
            }
            .id
            .to_channel(&ctx)
            .await?;

            let mode = match command
                .data
                .options
                .get(1)
                .and_then(|o| o.resolved.as_ref())
            {
                Some(ApplicationCommandInteractionDataOptionValue::String(s)) => match s.as_str() {
                    "json" => ArchivalMode::Json,
                    "html" => ArchivalMode::Html,
                    "all" => ArchivalMode::All,
                    _ => unreachable!("Invalid string choice"),
                },
                _ => unreachable!("Expected format as second argument"),
            };

            match channel {
                Channel::Guild(channel) => match command.guild_id {
                    Some(guild_id) => {
                        let guild = guild_id
                            .to_partial_guild(&ctx)
                            .await
                            .expect("Failed to fetch guild");

                        info!(
                            user = %format!(
                                "{}#{:04}",
                                command.user.name,
                                command.user.discriminator
                                ),
                            guild = %guild.name,
                            channel = %channel.name,
                            ?mode,
                            "Archive requested"
                        );

                        Ok(archive(ctx, &channel, &guild, mode)
                            .await
                            .map(archive_response)?)
                    }
                    None => {
                        error!("Command used outside of a guild channel");

                        Err("This command must be used within a guild".to_owned().into())
                    }
                },
                _ => {
                    error!(?channel, "Channel is not a text channel");

                    Err(
                        "Error: Argument `channel` must be a text channel in this guild."
                            .to_owned()
                            .into(),
                    )
                }
            }
        }
        _ => Err("Error: Invalid command".to_owned().into()),
    }
}

async fn handle_archive_message(ctx: &Context, msg: &Message) -> Result<()> {
    if msg.content == "!archive_emoji" {
        let guild = msg
            .guild_id
            .ok_or_else(|| "This command must be used from within a guild".to_owned())?
            .to_partial_guild(&ctx)
            .await?;
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
        return Ok(());
    } else {
        let capts = match COMMAND_REGEX.captures(&msg.content) {
            Some(x) => x,
            None => {
                msg.reply(&ctx, USAGE_STRING).await.expect(REPLY_FAILURE);
                return Err("Invalid archive command".to_owned().into());
            }
        };

        let channel_id_str = &capts[1];
        let mode = match capts.get(2).map(|x| x.as_str()) {
            Some("json") => ArchivalMode::Json,
            Some("html") => ArchivalMode::Html,
            _ => ArchivalMode::All,
        };
        trace!(channel_id = %channel_id_str, ?mode, "Command parsed");

        let channel = match ChannelId::from_str(channel_id_str) {
            Ok(x) => x,
            Err(e) => {
                error!(channel_id = %channel_id_str, error = ?e, "Invalid channel id");
                return Err(format!("Invalid channel id {}.", channel_id_str).into());
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
                error!(?channel, "Channel is not a guild channel");
                return Err("This bot must be used in a guild channel".to_owned().into());
            }
        };

        info!(
            user = %format!("{}#{:04}", msg.author.name, msg.author.discriminator),
            guild = %guild.name,
            channel = %channel.name,
            ?mode,
            "Archive requested"
        );

        let response = archive(ctx, &channel, &guild, mode)
            .await
            .map(archive_response)?;

        msg.reply(&ctx, response).await.expect(REPLY_FAILURE);
    }
    Ok(())
}

#[instrument(skip_all)]
async fn archive(
    ctx: &Context,
    channel: &GuildChannel,
    guild: &PartialGuild,
    output_mode: ArchivalMode,
) -> Result<ArchiveLog> {
    trace!("Begin downloading messages");
    let start = std::time::Instant::now();

    // Download messages
    let messages = {
        /// The discord api limits us to retrieving 100 messages at a time
        /// See <https://discord.com/developers/docs/resources/channel#get-channel-messages>
        const MESSAGE_DOWNLOAD_LIMIT: u64 = 100;

        // Download the first 100 messages outside the loop, as the retriever closure is different
        let mut messages = channel
            .messages(&ctx, |r| r.limit(MESSAGE_DOWNLOAD_LIMIT))
            .await?;

        trace!(download_count = %messages.len());

        loop {
            let last_msg = messages.last().unwrap();
            let new_msgs = match channel
                .id
                .messages(&ctx, |retriever| retriever.before(last_msg.id).limit(100))
                .await
            {
                Ok(x) => x,
                Err(e) => {
                    warn!(
                        error = ?e,
                        download_count= %messages.len(),
                        "While trying to download messages, \
                        Discord returned an error. Waiting 5 seconds before retrying",
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
            };
            let recv_count = new_msgs.len();

            messages.extend(new_msgs.into_iter());

            // If the api sends fewer than 100 messages, we have fetched all the messages in the
            // channel
            if recv_count != 100 {
                messages.reverse();
                break messages;
            }
        }
    };

    let end = std::time::Instant::now();
    let download_time = end - start;

    info!(
        count = %messages.len(),
        time_taken = ?download_time,
        "Downloaded messages"
    );

    let output_file_stem_common = format!(
        "{}-{}",
        guild.name.replace(char::is_whitespace, "_"),
        channel.name.replace(char::is_whitespace, "_"),
    );

    let mut files_created = Vec::new();

    // XXX This is a litte ugly.
    let start = std::time::Instant::now();
    match output_mode {
        ArchivalMode::Json => {
            let output_path = OPTIONS
                .output_path
                .join(format!("{}.json", output_file_stem_common));
            json::write_json(ctx, guild, &messages, &output_path).await?;
            files_created.push(output_path);
        }
        ArchivalMode::Html => {
            let output_path = OPTIONS
                .output_path
                .join(format!("{}.html", output_file_stem_common));
            html::write_html(ctx, guild, channel, &messages, &output_path).await?;
            files_created.push(output_path);
        }
        ArchivalMode::All => {
            let output_path = OPTIONS
                .output_path
                .join(format!("{}.json", output_file_stem_common));
            json::write_json(ctx, guild, &messages, &output_path).await?;
            files_created.push(output_path);

            let output_path = OPTIONS
                .output_path
                .join(format!("{}.html", output_file_stem_common));
            html::write_html(ctx, guild, channel, &messages, &output_path).await?;
            files_created.push(output_path);
        }
    }
    let end = std::time::Instant::now();
    let render_time = end - start;

    info!(time_taken = ?(download_time + render_time), "Archive complete");

    Ok(ArchiveLog {
        download_time,
        render_time,
        files_created,
    })
}

fn archive_response(
    ArchiveLog {
        download_time,
        render_time,
        files_created,
    }: ArchiveLog,
) -> String {
    let download_time = if download_time.as_secs() >= 1 {
        format!(
            "{}m{:02}s",
            download_time.as_secs() / 60,
            download_time.as_secs() % 60,
        )
    } else {
        format!("{}ms", download_time.as_millis())
    };
    let render_time = if render_time.as_secs() >= 1 {
        format!(
            "{}m{:02}s",
            render_time.as_secs() / 60,
            render_time.as_secs() % 60,
        )
    } else {
        format!("{}ms", render_time.as_millis())
    };
    format!(
        "Archival complete!\
        Downloading messages took {}.\n\
        Rendering output took {}.\n\
        Created the following files:\n\
        ```\n\
        {}\n\
        ```",
        download_time,
        render_time,
        files_created
            .iter()
            .map(|x| x.to_string_lossy())
            .collect::<Vec<_>>()
            .join("\n")
    )
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
            match handle_slash_command(&ctx, &command).await {
                Ok(x) => {
                    command
                        .edit_original_interaction_response(&ctx, |builder| builder.content(x))
                        .await
                        .expect(REPLY_FAILURE);
                }
                Err(e) => {
                    error!(error = ?e, "An error occurred in handle_slash_command()");
                    command
                        .edit_original_interaction_response(&ctx, |builder| {
                            builder.content(format!("Error:\n```\n{:?}\n```", e))
                        })
                        .await
                        .expect(REPLY_FAILURE);
                }
            }
        }
    }

    async fn message(&self, ctx: Context, msg: Message) {
        if msg.content.starts_with("!archive") {
            if let Err(e) = handle_archive_message(&ctx, &msg).await {
                error!(error = ?e, "An error occurred in handle_archive_message()");
                msg.reply(&ctx, format!("Error:\n```\n{:?}\n```", e))
                    .await
                    .expect(REPLY_FAILURE);
            }
        }
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        info!(name = %ready.user.name, num_guilds = %ready.guilds.len(), "Bot logged in");

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
            commands = ?commands
                .iter()
                .map(|cmd| cmd.name.as_str())
                .collect::<Vec<_>>(),
            "Registered slash commands",
        );
    }
}

/// A small discord bot to archive the messages in a discord text channel. Provide the token with either --token, --token-filename, or as the environment variable DISCORD_TOKEN, in order of decreasing priority.
#[derive(Parser, Debug)]
#[clap(name = "discord-channel-archiver", version, author, about)]
struct Opt {
    /// File containing the token
    token_filename: PathBuf,
    /// File containing the application id
    appid_filename: PathBuf,
    /// The path to output files to
    #[structopt(default_value = "/dev/shm/")]
    output_path: PathBuf,
}
