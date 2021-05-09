use std::path::PathBuf;

use crate::file;

use futures::stream::FuturesUnordered;
use futures::StreamExt;
use log::*;
use serenity::model::channel::Message;
use serenity::prelude::*;

pub async fn archive_emoji(ctx: &Context, msg: &Message) {
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
    info!("Starting emoji archive");
    let mut output_directory = PathBuf::from(&crate::OPTIONS.path);
    output_directory.push(format!(
        "{}-{}",
        guild.name.replace(' ', "-").to_lowercase(),
        chrono::Utc::now().format("%Y-%m-%dT%H-%M-%S")
    ));

    let n = guild.emojis.len();

    let mut fut: FuturesUnordered<_> = guild
        .emojis
        .iter()
        .map(|(_, emoji)| {
            let url = emoji.url();
            let ext = &url[url
                .rfind('.')
                .expect("Emoji url does not have a file extension")
                + 1..];
            let download_path = output_directory.join(format!("{}.{}", emoji.name, ext));
            file::download_url(url, download_path)
        })
        .collect();
    #[allow(clippy::redundant_pattern_matching)]
    while let Some(_) = fut.next().await {}

    info!("Downloads complete. Archived {} emoji.", n);
    msg.reply(
        &ctx,
        format!(
            "Archived {} emoji into `{}`",
            n,
            output_directory.as_os_str().to_str().unwrap()
        ),
    )
    .await
    .expect("Failed to reply to message");
}
