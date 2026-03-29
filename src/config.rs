use std::env;
use std::path::PathBuf;

pub struct Config {
    pub route_id: String,
    pub stop_id: String,
    pub direction_id: u8,
    pub gtfs_dir: PathBuf,
    pub trip_update_url: String,
    pub alerts_url: String,
    pub departure_count: usize,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            route_id: env::var("GTFS_ROUTE_ID").unwrap_or_else(|_| "113G".into()),
            stop_id: env::var("GTFS_STOP_ID").unwrap_or_else(|_| "34510".into()),
            direction_id: env::var("GTFS_DIRECTION_ID")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            gtfs_dir: env::var("GTFS_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("rtd-purchased-commuter")),
            trip_update_url: env::var("GTFS_RT_TRIP_UPDATE_URL").unwrap_or_else(|_| {
                "https://open-data.rtd-denver.com/files/gtfs-rt/rtd/TripUpdate.pb".into()
            }),
            alerts_url: env::var("GTFS_RT_ALERTS_URL").unwrap_or_else(|_| {
                "https://open-data.rtd-denver.com/files/gtfs-rt/rtd/Alerts.pb".into()
            }),
            departure_count: env::var("GTFS_DEPARTURE_COUNT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5),
        }
    }
}
