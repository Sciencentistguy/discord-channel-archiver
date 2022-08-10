use crate::Result;

use std::path::Path;

use serde_json::json;
use serenity::model::channel::Message;
use serenity::model::guild::Guild;
use serenity::model::prelude::GuildChannel;
use serenity::prelude::Context;
use tracing::*;

#[instrument(skip_all)]
pub async fn write_json<P: AsRef<Path>>(
    _ctx: &Context,
    guild: &Guild,
    channel: &GuildChannel,
    messages: &[Message],
    path: P,
) -> Result<()> {
    trace!("Entered json writer");
    let json = json!({
        "guild" : guild,
        "channel" : channel,
        "messages" : messages,
    });
    // let json = json!(guild);

    let output = serde_json::to_string_pretty(&json)?;
    tokio::fs::write(path, output).await?;
    //serde_json::to_writer_pretty(file, &json)?;
    info!("JSON generation complete");
    Ok(())
}
