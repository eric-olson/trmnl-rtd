use std::io::{Cursor, Read};

use worker::*;
use zip::ZipArchive;

const GTFS_ZIP_URL: &str =
    "https://www.rtd-denver.com/files/gtfs/RTD_Denver_Purchased_Transportation_Commuter_Rail_GTFS.zip";

/// The CSV files we need from the GTFS zip.
const NEEDED_FILES: &[&str] = &[
    "routes.txt",
    "trips.txt",
    "stop_times.txt",
    "stops.txt",
    "calendar.txt",
];

/// Downloads the RTD GTFS zip, extracts the files we need, and uploads them to R2.
pub async fn refresh_gtfs(env: &Env) -> Result<()> {
    let bucket = env.bucket("GTFS_BUCKET")?;

    // Download the GTFS zip from RTD
    let req = Request::new(GTFS_ZIP_URL, Method::Get)?;
    let mut resp = Fetch::Request(req).send().await?;
    let bytes = resp.bytes().await?;

    console_log!("Downloaded GTFS zip ({} bytes)", bytes.len());

    // Extract and upload each needed file
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| Error::RustError(format!("Failed to open zip: {}", e)))?;

    for name in NEEDED_FILES {
        let mut file = archive.by_name(name)
            .map_err(|e| Error::RustError(format!("Missing {} in zip: {}", name, e)))?;

        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .map_err(|e| Error::RustError(format!("Failed to read {}: {}", name, e)))?;

        bucket.put(*name, contents).execute().await?;
        console_log!("Uploaded {} to R2", name);
    }

    console_log!("GTFS refresh complete");
    Ok(())
}
