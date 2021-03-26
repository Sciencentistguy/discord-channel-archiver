use std::fs::File;
use std::io;
use std::path::Path;

use log::*;

pub async fn download_url<P>(
    url: String,
    destination_filename: P,
) -> Result<(), Box<dyn std::error::Error>>
where
    P: AsRef<Path>,
{
    let destination_filename = destination_filename.as_ref();
    trace!(
        "Downloading '{}' to '{}'",
        url,
        destination_filename.to_str().unwrap()
    );
    let response = reqwest::get(url.as_str()).await?;
    let mut dest = {
        let destdir = destination_filename
            .parent()
            .expect("Destination path did not have a parent");
        if !destdir.is_dir() {
            std::fs::create_dir(destdir)?;
        };
        File::create(destination_filename)?
    };

    let mut content = io::Cursor::new(response.bytes().await?);
    std::io::copy(&mut content, &mut dest)?;
    info!("Download complete");

    Ok(())
}
