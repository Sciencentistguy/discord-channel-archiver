use crate::Result;

use std::path::Path;

use serde_json::json;
use serenity::model::channel::Message;
use serenity::model::guild::Guild;
use serenity::prelude::Context;
use tracing::*;

#[instrument(skip_all)]
pub async fn write_json<P: AsRef<Path>>(
    ctx: &Context,
    guild: &Guild,
    messages: &[Message],
    path: P,
) -> Result<()> {
    trace!("Entered json writer");
    let channel = messages
        .first()
        .expect("Messages should not be empty")
        .channel(&ctx)
        .await
        .unwrap()
        .guild()
        .unwrap();

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
