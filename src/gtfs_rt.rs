use chrono::{DateTime, TimeZone};
use chrono_tz::America::Denver;
use prost::Message;
use worker::*;

use crate::config::Config;

pub mod transit_realtime {
    include!(concat!(env!("OUT_DIR"), "/transit_realtime.rs"));
}

use transit_realtime::FeedMessage;

// Re-alias so we can use both worker::Result and std Result
type StdResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug)]
pub struct RealtimeDeparture {
    pub trip_id: String,
    pub delay_seconds: Option<i32>,
    pub estimated_time: Option<DateTime<chrono_tz::Tz>>,
}

#[derive(Debug)]
pub struct Alert {
    pub header: String,
    pub description: String,
}

pub struct RealtimeData {
    pub departures: Vec<RealtimeDeparture>,
    pub alerts: Vec<Alert>,
}

pub async fn fetch_realtime(config: &Config) -> Result<RealtimeData> {
    let trip_req = Request::new(&config.trip_update_url, Method::Get)?;
    let mut trip_resp = Fetch::Request(trip_req).send().await?;
    let trip_bytes = trip_resp.bytes().await?;

    let alert_req = Request::new(&config.alerts_url, Method::Get)?;
    let mut alert_resp = Fetch::Request(alert_req).send().await?;
    let alert_bytes = alert_resp.bytes().await?;

    let departures = parse_trip_updates(&trip_bytes, config)
        .map_err(|e| Error::RustError(e.to_string()))?;
    let alerts = parse_alerts(&alert_bytes, config)
        .map_err(|e| Error::RustError(e.to_string()))?;

    Ok(RealtimeData {
        departures,
        alerts,
    })
}

fn parse_trip_updates(bytes: &[u8], config: &Config) -> StdResult<Vec<RealtimeDeparture>> {
    let feed = FeedMessage::decode(bytes)?;
    let mut departures = Vec::new();

    for entity in &feed.entity {
        let Some(tu) = &entity.trip_update else {
            continue;
        };
        let trip = &tu.trip;

        // Filter by route_id
        if trip.route_id.as_deref() != Some(config.route_id.as_str()) {
            continue;
        }

        // Filter by direction_id if provided in the RT data
        if let Some(dir) = trip.direction_id {
            if dir != config.direction_id as u32 {
                continue;
            }
        }

        let trip_id = trip.trip_id.clone().unwrap_or_default();

        // Find the stop_time_update for our stop
        for stu in &tu.stop_time_update {
            if stu.stop_id.as_deref() != Some(config.stop_id.as_str()) {
                continue;
            }

            // Prefer departure, fall back to arrival
            let event = stu.departure.as_ref().or(stu.arrival.as_ref());

            let (delay_seconds, estimated_time) = match event {
                Some(e) => {
                    let delay = e.delay;
                    let est = e.time.and_then(|t| {
                        Denver.timestamp_opt(t, 0).single()
                    });
                    (delay, est)
                }
                None => {
                    // Use trip-level delay if no stop-level event
                    (tu.delay, None)
                }
            };

            departures.push(RealtimeDeparture {
                trip_id,
                delay_seconds,
                estimated_time,
            });
            break;
        }
    }

    Ok(departures)
}

fn parse_alerts(bytes: &[u8], config: &Config) -> StdResult<Vec<Alert>> {
    let feed = FeedMessage::decode(bytes)?;
    let mut alerts = Vec::new();

    for entity in &feed.entity {
        let Some(alert) = &entity.alert else {
            continue;
        };

        // Check if this alert applies to our route or stop
        let applies = alert.informed_entity.iter().any(|ie| {
            let route_match = ie.route_id.as_deref() == Some(config.route_id.as_str());
            let stop_match = ie.stop_id.as_deref() == Some(config.stop_id.as_str());
            route_match || stop_match
        });

        if !applies {
            continue;
        }

        let header = extract_translation(alert.header_text.as_ref());
        let description = extract_translation(alert.description_text.as_ref());

        if !header.is_empty() || !description.is_empty() {
            alerts.push(Alert {
                header,
                description,
            });
        }
    }

    Ok(alerts)
}

fn extract_translation(ts: Option<&transit_realtime::TranslatedString>) -> String {
    let Some(ts) = ts else {
        return String::new();
    };

    // Prefer English, then any unspecified language, then first available
    ts.translation
        .iter()
        .find(|t| t.language.as_deref() == Some("en"))
        .or_else(|| ts.translation.iter().find(|t| t.language.is_none()))
        .or_else(|| ts.translation.first())
        .map(|t| t.text.clone())
        .unwrap_or_default()
}
