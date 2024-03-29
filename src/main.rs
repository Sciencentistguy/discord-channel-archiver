mod emoji;
mod error;
mod file;
mod html;
mod json;

use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;
use std::time::Instant;

use clap::Parser;
use indoc::indoc;
use once_cell::sync::Lazy;
use regex::Regex;
use serenity::async_trait;
use serenity::model::application::command::Command;
use serenity::model::application::command::CommandOptionType;
use serenity::model::application::interaction::application_command::ApplicationCommandInteraction;
use serenity::model::application::interaction::application_command::CommandDataOptionValue;
use serenity::model::application::interaction::Interaction;
use serenity::model::application::interaction::InteractionResponseType;
use serenity::model::channel::Channel;
use serenity::model::channel::GuildChannel;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::model::guild::Guild;
use serenity::model::id::ChannelId;
use serenity::prelude::*;
use tracing::*;
use tracing_subscriber::EnvFilter;

use crate::emoji::archive_emoji;

type Result<T> = std::result::Result<T, error::Error>;

const USAGE_STRING: &str = indoc! { "
    Invalid syntax.
    Correct usage is `!archive <channel> [mode]`, \
    where `channel` is the channel you want to archive, and `mode` \
    is one of either `json`, `html`, or `all`."
};

const REPLY_FAILURE: &str = "Failed to reply to message";

static COMMAND_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^!archive +<#(\d+)> *([\w,]+)?$").unwrap());

static OPTIONS: Lazy<Opt> = Lazy::new(Opt::parse);

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .pretty()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let token = tokio::fs::read_to_string(&OPTIONS.token_filename)
        .await
        .expect("File does not exist");
    let token = token.trim();

    let application_id = tokio::fs::read_to_string(&OPTIONS.appid_filename)
        .await
        .expect("File does not exist")
        .trim()
        .parse::<u64>()
        .expect("Invalid application_id");

    trace!(%token);

    tokio::spawn(async { html::prebuild_regexes() });

    let intents = GatewayIntents::all();

    // Create a new instance of the Client, logging in as a bot. This will
    // automatically prepend your bot token with "Bot ", which is a requirement
    // by Discord for bot users.
    let mut client = Client::builder(&token, intents)
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
    download_time: Duration,
    render_time: Duration,
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
                    let guild = guild_id
                        .to_guild_cached(&ctx)
                        .ok_or_else(|| "Guild not found in cache".to_owned())?;
                    let (n, output_path) = archive_emoji(guild).await;
                    Ok(format!(
                        "Archived {} emoji into `{}`",
                        n,
                        output_path.display(),
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

            assert_eq!(
                command.data.options.len(),
                2,
                "Command framework should only ever provide 2 args"
            );

            let channel = match command.data.options[0].resolved.as_ref() {
                Some(CommandDataOptionValue::Channel(c)) => c,
                _ => unreachable!("Expected channel as first argument"),
            }
            .id
            .to_channel(&ctx)
            .await?;

            let mode = match command.data.options[1].resolved.as_ref() {
                Some(CommandDataOptionValue::String(s)) => s
                    .parse()
                    .expect("Command framework should prevent invalid responses"),
                _ => unreachable!("Expected format as second argument"),
            };

            match channel {
                Channel::Guild(channel) => match command.guild_id {
                    Some(guild_id) => {
                        let guild = guild_id
                            .to_guild_cached(&ctx)
                            // .to_partial_guild(&ctx)
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
            .to_guild_cached(&ctx)
            .ok_or_else(|| "Guild not found in cache".to_owned())?;
        let (n, output_path) = emoji::archive_emoji(guild).await;
        msg.reply(
            &ctx,
            format!("Archived {} emoji into `{}`", n, output_path.display(),),
        )
        .await
        .expect(REPLY_FAILURE);
        return Ok(());
    } else {
        let capts = match COMMAND_REGEX.captures(&msg.content) {
            Some(x) => x,
            None => {
                msg.reply(&ctx, USAGE_STRING).await.expect(REPLY_FAILURE);
                warn!(command = %msg.content, "Invalid `!` command");
                return Ok(());
            }
        };

        let channel_id_str = &capts[1];
        let mode = capts[2].parse()?;
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
            Some(guild_id) => guild_id.to_guild_cached(&ctx).unwrap(),
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

async fn download_channel_messages(
    ctx: &Context,
    channel: &GuildChannel,
) -> Result<(Vec<Message>, Duration)> {
    trace!("Begin downloading messages");
    let start = Instant::now();

    /// The discord api limits us to retrieving 100 messages at a time
    ///
    /// See <https://discord.com/developers/docs/resources/channel#get-channel-messages>
    const MESSAGE_DOWNLOAD_LIMIT: u64 = 100;

    // Download the first 100 messages.
    let mut messages = channel
        .messages(&ctx, |r| r.limit(MESSAGE_DOWNLOAD_LIMIT))
        .await?;

    trace!(download_count = %messages.len());

    // Don't attempt to download more messages if zero were downloaded before.
    if !messages.is_empty() {
        loop {
            let last_msg = messages.last().unwrap();
            let new_msgs = channel
                .id
                .messages(&ctx, |r| {
                    r.before(last_msg.id).limit(MESSAGE_DOWNLOAD_LIMIT)
                })
                .await;
            let new_msgs = match new_msgs {
                Ok(x) => x,
                Err(e) => {
                    warn!(
                        error = ?e,
                        download_count= %messages.len(),
                        "While trying to download messages, \
                        Discord returned an error. Waiting 5 seconds before retrying",
                    );
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
            };
            let recv_count = new_msgs.len();

            messages.extend(new_msgs.into_iter());

            // If the api sends fewer than `MESSAGE_DOWNLOAD_LIMIT` messages, we have fetched all
            // the messages in the channel
            if recv_count != MESSAGE_DOWNLOAD_LIMIT as usize {
                messages.reverse();
                break;
            }
        }
    }

    let end = Instant::now();
    let download_time = end - start;

    Ok((messages, download_time))
}

#[instrument(skip_all)]
async fn archive(
    ctx: &Context,
    channel: &GuildChannel,
    guild: &Guild,
    output_mode: OutputMode,
) -> Result<ArchiveLog> {
    let (messages, download_time) = download_channel_messages(ctx, channel).await?;
    info!(
        count = %messages.len(),
        time_taken = ?download_time,
        "Downloaded messages"
    );

    let output_file_stem = format!(
        "{}-{}",
        guild.name.replace(char::is_whitespace, "_"),
        channel.name.replace(char::is_whitespace, "_"),
    );

    let mut files_created = Vec::new();

    let start = Instant::now();

    if output_mode.do_json() {
        let output_path = OPTIONS.output_path.join(format!("{output_file_stem}.json"));
        json::write_json(ctx, guild, channel, &messages, &output_path).await?;
        files_created.push(output_path);
    }

    if output_mode.do_html() {
        let output_path = OPTIONS.output_path.join(format!("{output_file_stem}.html"));
        html::write_html(ctx, guild, channel, &messages, &output_path).await?;
        files_created.push(output_path);
    }

    let end = Instant::now();
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
        indoc! { "
            Archival complete!
            Downloading messages took {}.
            Rendering output took {}.
            Created the following files:
            ```
            {}
            ```"
        },
        download_time,
        render_time,
        files_created
            .iter()
            .map(|x| x.display().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    )
}

struct Handler;

#[derive(Debug, Clone, Copy)]
enum OutputMode {
    Json,
    Html,
    All,
}

impl OutputMode {
    fn do_json(self) -> bool {
        matches!(self, OutputMode::Json | OutputMode::All)
    }

    fn do_html(self) -> bool {
        matches!(self, OutputMode::Html | OutputMode::All)
    }
}

impl FromStr for OutputMode {
    type Err = String;

    fn from_str(s: &str) -> core::result::Result<Self, Self::Err> {
        match s {
            "json" => Ok(OutputMode::Json),
            "html" => Ok(OutputMode::Html),
            "all" => Ok(OutputMode::All),
            _ => Err(format!(
                indoc! { "
                Invalid output mode {}. Valid values are one of the following:
                ```
                - json
                - html
                - all
                ```"
                },
                s
            )),
        }
    }
}

impl std::fmt::Display for OutputMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[async_trait]
impl EventHandler for Handler {
    // Called when a slash-command is invoked.
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

    // Called when a message is sent in any channel the bot can see.
    //
    // If that message starts with `!archive`, attempt to parse that as an archive command (emoji
    // or channel.)
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

    // Called when the bot is ready.
    //
    // Register slash commands.
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!(name = %ready.user.name, num_guilds = %ready.guilds.len(), "Bot logged in");

        let commands = Command::set_global_application_commands(&ctx, |builder| {
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
                                .kind(CommandOptionType::Channel)
                                .required(true)
                        })
                        .create_option(|option_builder| {
                            option_builder
                                .name("output_format")
                                .description("The file format to output to")
                                .kind(CommandOptionType::String)
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

/// A small discord bot to archive the messages in a discord text channel.
#[derive(Parser, Debug)]
#[clap(name = "discord-channel-archiver", version, author, about)]
struct Opt {
    /// File containing the token
    token_filename: PathBuf,
    /// File containing the application id
    appid_filename: PathBuf,
    /// The path to output files to
    #[clap(default_value = "/dev/shm")]
    output_path: PathBuf,
}
