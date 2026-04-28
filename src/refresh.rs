use std::io::{Cursor, Read};

use chrono::{NaiveDate, Utc};
use worker::*;
use zip::ZipArchive;

const GTFS_ZIP_URL: &str =
    "https://www.rtd-denver.com/files/gtfs/RTD_Denver_Purchased_Transportation_Commuter_Rail_GTFS.zip";

const NEEDED_FILES: &[&str] = &[
    "routes.txt",
    "trips.txt",
    "stop_times.txt",
    "stops.txt",
    "calendar.txt",
];

/// Scans calendar.txt content for the earliest start_date across all service rows.
fn earliest_start_date(csv: &str) -> Option<NaiveDate> {
    let mut lines = csv.lines();
    let header = lines.next()?;
    let start_idx = header
        .split(',')
        .map(|c| c.trim().trim_matches('"'))
        .position(|c| c == "start_date")?;

    let mut earliest: Option<NaiveDate> = None;
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split(',').map(|f| f.trim().trim_matches('"')).collect();
        if let Some(ds) = fields.get(start_idx) {
            if let Ok(d) = NaiveDate::parse_from_str(ds, "%Y%m%d") {
                earliest = Some(earliest.map_or(d, |e: NaiveDate| e.min(d)));
            }
        }
    }
    earliest
}

/// Downloads the RTD GTFS zip, extracts needed files, and writes them to R2.
///
/// Slot selection:
/// - If the new schedule's start_date is in the future → write to `pending/`
///   (keeps the existing `active/` data in place during the gap)
/// - If the new schedule is current → write to `active/` and delete `pending/`
pub async fn refresh_gtfs(env: &Env) -> Result<()> {
    let bucket = env.bucket("GTFS_BUCKET")?;

    let req = Request::new(GTFS_ZIP_URL, Method::Get)?;
    let mut resp = Fetch::Request(req).send().await?;
    let bytes = resp.bytes().await?;
    console_log!("Downloaded GTFS zip ({} bytes)", bytes.len());

    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| Error::RustError(format!("Failed to open zip: {}", e)))?;

    // Extract all needed files into memory first so we can inspect calendar.txt
    // before deciding which slot to write to.
    let mut file_contents: Vec<(&str, String)> = Vec::new();
    for name in NEEDED_FILES {
        let mut file = archive
            .by_name(name)
            .map_err(|e| Error::RustError(format!("Missing {} in zip: {}", name, e)))?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .map_err(|e| Error::RustError(format!("Failed to read {}: {}", name, e)))?;
        file_contents.push((name, contents));
    }

    let today = Utc::now().date_naive();
    let calendar_csv = file_contents
        .iter()
        .find(|(n, _)| *n == "calendar.txt")
        .map(|(_, c)| c.as_str())
        .unwrap_or("");

    let slot = match earliest_start_date(calendar_csv) {
        Some(start) if start > today => "pending",
        _ => "active",
    };

    for (name, contents) in &file_contents {
        let key = format!("{}/{}", slot, name);
        bucket.put(&key, contents.clone()).execute().await?;
        console_log!("Uploaded {} to R2", key);
    }

    // New schedule is active — pending data is no longer needed.
    if slot == "active" {
        for name in NEEDED_FILES {
            let _ = bucket.delete(&format!("pending/{}", name)).await;
        }
    }

    console_log!("GTFS refresh complete (slot: {})", slot);
    Ok(())
}
