mod config;
mod gtfs_rt;
mod schedule;

use std::collections::HashMap;

use chrono::{Datelike, Utc};
use chrono_tz::America::Denver;
use serde::Serialize;

use config::Config;

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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env();

    let schedule = schedule::load_schedule(&config)?;

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

    let output = Output {
        station: schedule.station_name,
        line: schedule.route_info.short_name,
        line_color: schedule.route_info.color,
        direction: schedule.headsign,
        departures,
        alerts,
        updated_at: now_denver.to_rfc3339(),
    };

    println!("{}", serde_json::to_string_pretty(&output)?);

    Ok(())
}
