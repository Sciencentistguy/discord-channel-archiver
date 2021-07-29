use crate::file;
use crate::OPTIONS;
use crate::REPLY_FAILURE;

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
                .expect(REPLY_FAILURE);
            error!("This bot must be used in a guild channel.");
            return;
        }
    };
    info!("Starting emoji archive");
    let output_directory = OPTIONS.output_path.join(format!(
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
    .expect(REPLY_FAILURE);
}
