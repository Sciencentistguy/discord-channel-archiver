use crate::Result;

use std::path::Path;

use tracing::*;

#[instrument(skip(destination_filename))]
pub async fn download_url<P>(url: String, destination_filename: P) -> Result<()>
where
    P: AsRef<Path>,
{
    let destination_filename = destination_filename.as_ref();
    info!(%url, ?destination_filename, "Downloading file");

    let response = reqwest::get(url.as_str()).await?;

    let destdir = destination_filename
        .parent()
        .expect("Destination path did not have a parent");
    if !destdir.is_dir() {
        tokio::fs::create_dir(destdir).await?;
    };

    let bytes = response.bytes().await?;

    tokio::fs::write(destination_filename, bytes).await?;

    trace!("Download complete");

    Ok(())
}
