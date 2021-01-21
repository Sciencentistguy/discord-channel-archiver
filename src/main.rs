mod json;

use std::env;

use log::*;
use serenity::{
    async_trait,
    model::{channel::Message, gateway::Ready},
    prelude::*,
};
use structopt::StructOpt;

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

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: Message) {
        if msg.content == "!archive" {
            let channel = msg
                .channel_id
                .to_channel(&ctx)
                .await
                .expect("Channel not found")
                .guild()
                .expect("Invalid channel type");
            let channel_name = channel.name;
            let guild_name = {
                let guild = channel.guild_id.to_partial_guild(&ctx).await.unwrap();
                guild.name
            };
            info!(
                "Archive started by user {} in channel {}",
                msg.author,
                channel.id.to_string(),
            );
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
            let messages = messages; // messages is a Vec<Message>, in order from oldest to newest

            match json::write_json(
                &messages,
                format!("/dev/shm/{}-{}.json", guild_name, channel_name),
                &ctx,
            )
            .await
            {
                Ok(_) => {}
                Err(x) => error!("Error writing json: {}", x),
            }

            info!("Archive complete.");
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
