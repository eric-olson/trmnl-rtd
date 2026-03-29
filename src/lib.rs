mod config;
mod gtfs_rt;
mod refresh;
mod schedule;

use std::collections::HashMap;

use chrono::{Datelike, Utc};
use chrono_tz::America::Denver;
use serde::Serialize;
use worker::*;

use config::Config;
use schedule::GtfsCsvs;

#[derive(Serialize)]
struct Output {
    station: String,
    line: String,
    line_color: String,
    direction: String,
    departures: Vec<DepartureOutput>,
    alerts: Vec<AlertOutput>,
    updated_at: String,
}

#[derive(Serialize)]
struct DepartureOutput {
    scheduled: String,
    estimated: Option<String>,
    status: String,
}

#[derive(Serialize)]
struct AlertOutput {
    header: String,
    description: String,
}

/// Load the GTFS CSV files from the R2 bucket.
async fn load_gtfs_csvs(bucket: &Bucket) -> Result<GtfsCsvs> {
    async fn get_csv(bucket: &Bucket, name: &str) -> Result<String> {
        let obj = bucket.get(name).execute().await?
            .ok_or_else(|| Error::RustError(format!("{} not found in R2 — run GTFS refresh first", name)))?;
        obj.body()
            .ok_or_else(|| Error::RustError(format!("{} has no body", name)))?
            .text()
            .await
    }

    Ok(GtfsCsvs {
        routes: get_csv(bucket, "routes.txt").await?,
        trips: get_csv(bucket, "trips.txt").await?,
        stop_times: get_csv(bucket, "stop_times.txt").await?,
        stops: get_csv(bucket, "stops.txt").await?,
        calendar: get_csv(bucket, "calendar.txt").await?,
    })
}

/// Main logic: load schedule from R2, fetch realtime data, merge and return JSON output.
async fn run(env: &Env) -> Result<Output> {
    let config = Config::from_env(env);
    let bucket = env.bucket("GTFS_BUCKET")?;

    let csvs = load_gtfs_csvs(&bucket).await?;
    let schedule = schedule::load_schedule(&config, &csvs)
        .map_err(|e| Error::RustError(e.to_string()))?;

    let now_denver = Utc::now().with_timezone(&Denver);
    let now_time = now_denver.time();
    let weekday = now_denver.weekday();

    let upcoming = schedule.upcoming_departures(now_time, weekday, config.departure_count);

    let realtime = gtfs_rt::fetch_realtime(&config).await?;

    // Index RT departures by trip_id for quick lookup
    let rt_by_trip: HashMap<&str, &gtfs_rt::RealtimeDeparture> = realtime
        .departures
        .iter()
        .map(|d| (d.trip_id.as_str(), d))
        .collect();

    let departures: Vec<DepartureOutput> = upcoming
        .iter()
        .map(|dep| {
            let (estimated, status) = match rt_by_trip.get(dep.trip_id.as_str()) {
                Some(rt) => {
                    // If we have an absolute estimated time, use it
                    if let Some(est) = rt.estimated_time {
                        let est_str = est.format("%H:%M").to_string();
                        let status = if est_str == dep.departure_time_str {
                            "on_time"
                        } else {
                            "delayed"
                        };
                        (Some(est_str), status.to_string())
                    } else if let Some(delay) = rt.delay_seconds {
                        if delay == 0 {
                            (None, "on_time".to_string())
                        } else {
                            // Compute estimated from scheduled + delay
                            let scheduled = dep.departure_time;
                            let est = scheduled
                                + chrono::Duration::seconds(delay as i64);
                            let est_str = est.format("%H:%M").to_string();
                            (Some(est_str), "delayed".to_string())
                        }
                    } else {
                        (None, "on_time".to_string())
                    }
                }
                None => (None, "scheduled".to_string()),
            };

            DepartureOutput {
                scheduled: dep.departure_time_str.clone(),
                estimated,
                status,
            }
        })
        .collect();

    let alerts: Vec<AlertOutput> = realtime
        .alerts
        .into_iter()
        .map(|a| AlertOutput {
            header: a.header,
            description: a.description,
        })
        .collect();

    Ok(Output {
        station: schedule.station_name,
        line: schedule.route_info.short_name,
        line_color: schedule.route_info.color,
        direction: schedule.headsign,
        departures,
        alerts,
        updated_at: now_denver.to_rfc3339(),
    })
}

/// HTTP handler — returns departure JSON (useful for testing with `wrangler dev`).
#[event(fetch)]
async fn fetch(_req: Request, env: Env, _ctx: Context) -> Result<Response> {
    console_error_panic_hook::set_once();

    let output = run(&env).await?;
    let json = serde_json::to_string_pretty(&output)
        .map_err(|e| Error::RustError(e.to_string()))?;

    let mut headers = Headers::new();
    headers.set("Content-Type", "application/json")?;

    Ok(Response::ok(json)?.with_headers(headers))
}

/// Cron handler — dispatches based on which cron pattern triggered.
/// - `*/15 * * * *` → compute departures (will POST to TRMNL webhook later)
/// - `0 3 * * 1`   → refresh GTFS static data from RTD
#[event(scheduled)]
async fn scheduled(event: ScheduledEvent, env: Env, _ctx: ScheduleContext) {
    console_error_panic_hook::set_once();

    match event.cron().as_str() {
        "0 3 * * 1" => {
            if let Err(e) = refresh::refresh_gtfs(&env).await {
                console_error!("GTFS refresh failed: {}", e);
            }
        }
        _ => {
            match run(&env).await {
                Ok(output) => {
                    match serde_json::to_string(&output) {
                        Ok(json) => console_log!("{}", json),
                        Err(e) => console_error!("JSON serialization error: {}", e),
                    }
                }
                Err(e) => console_error!("Departure fetch error: {}", e),
            }
        }
    }
}
