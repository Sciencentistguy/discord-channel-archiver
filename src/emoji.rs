use std::path::PathBuf;

// Tracing appears to get angry without this `use`
use std::file;

use crate::file;
use crate::OPTIONS;

use futures::stream::FuturesUnordered;
use futures::StreamExt;
use serenity::model::guild::PartialGuild;
use tracing::*;

#[instrument(skip_all)]
pub async fn archive_emoji(guild: PartialGuild) -> (usize, PathBuf) {
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
    while let Some(x) = fut.next().await {
        if let Err(e) = x {
            error!(error = ?e, "Failed to download an emoji");
        }
    }

    info!(?n, "Emoji download complete");

    (n, output_directory)
}
