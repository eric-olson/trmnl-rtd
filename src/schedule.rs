use std::collections::{HashMap, HashSet};
use std::path::Path;

use chrono::{NaiveTime, Weekday};
use serde::Deserialize;

use crate::config::Config;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GtfsRoute {
    route_id: String,
    route_short_name: String,
    route_long_name: String,
    route_color: String,
}

#[derive(Debug, Deserialize)]
struct GtfsTrip {
    route_id: String,
    service_id: String,
    trip_id: String,
    trip_headsign: String,
    direction_id: u8,
}

#[derive(Debug, Deserialize)]
struct GtfsStopTime {
    trip_id: String,
    departure_time: String,
    stop_id: String,
}

#[derive(Debug, Deserialize)]
struct GtfsStop {
    stop_id: String,
    stop_name: String,
    #[allow(dead_code)]
    location_type: Option<u8>,
    parent_station: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GtfsCalendar {
    service_id: String,
    monday: u8,
    tuesday: u8,
    wednesday: u8,
    thursday: u8,
    friday: u8,
    saturday: u8,
    sunday: u8,
}

#[derive(Debug, Clone)]
pub struct ScheduledDeparture {
    pub trip_id: String,
    pub departure_time: NaiveTime,
    pub departure_time_str: String,
}

pub struct RouteInfo {
    pub short_name: String,
    pub color: String,
}

pub struct Schedule {
    pub route_info: RouteInfo,
    pub station_name: String,
    pub headsign: String,
    departures_by_weekday: HashMap<Weekday, Vec<ScheduledDeparture>>,
}

impl Schedule {
    pub fn upcoming_departures(
        &self,
        now_time: NaiveTime,
        weekday: Weekday,
        count: usize,
    ) -> Vec<ScheduledDeparture> {
        let Some(departures) = self.departures_by_weekday.get(&weekday) else {
            return Vec::new();
        };

        departures
            .iter()
            .filter(|d| d.departure_time >= now_time)
            .take(count)
            .cloned()
            .collect()
    }
}

fn parse_gtfs_time(s: &str) -> Option<NaiveTime> {
    let parts: Vec<&str> = s.trim().split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    let h: u32 = parts[0].parse().ok()?;
    let m: u32 = parts[1].parse().ok()?;
    let s: u32 = parts[2].parse().ok()?;
    // GTFS times can exceed 24:00 for next-day service
    let h = h % 24;
    NaiveTime::from_hms_opt(h, m, s)
}

fn format_time_hhmm(t: NaiveTime) -> String {
    t.format("%H:%M").to_string()
}

pub fn load_schedule(config: &Config) -> Result<Schedule, Box<dyn std::error::Error>> {
    let dir = &config.gtfs_dir;

    // Load route info
    let route_info = load_route_info(dir, &config.route_id)?;

    // Load station name
    let station_name = load_station_name(dir, &config.stop_id)?;

    // Load calendar to map service_id -> active weekdays
    let service_weekdays = load_calendar(dir)?;

    // Load trips for this route + direction, collecting trip_ids grouped by service_id
    let (trips_by_service, headsign) =
        load_trips(dir, &config.route_id, config.direction_id)?;

    // Build set of all relevant trip_ids
    let all_trip_ids: HashSet<&str> = trips_by_service
        .values()
        .flat_map(|ids| ids.iter().map(String::as_str))
        .collect();

    // Load stop_times for relevant trips at our stop
    let stop_times = load_stop_times(dir, &all_trip_ids, &config.stop_id)?;

    // Build departures grouped by weekday
    let mut departures_by_weekday: HashMap<Weekday, Vec<ScheduledDeparture>> = HashMap::new();

    for (service_id, trip_ids) in &trips_by_service {
        let Some(weekdays) = service_weekdays.get(service_id.as_str()) else {
            continue;
        };

        for trip_id in trip_ids {
            if let Some((time, time_str)) = stop_times.get(trip_id.as_str()) {
                for &wd in weekdays {
                    departures_by_weekday
                        .entry(wd)
                        .or_default()
                        .push(ScheduledDeparture {
                            trip_id: trip_id.clone(),
                            departure_time: *time,
                            departure_time_str: time_str.clone(),
                        });
                }
            }
        }
    }

    // Sort each weekday's departures by time
    for deps in departures_by_weekday.values_mut() {
        deps.sort_by_key(|d| d.departure_time);
    }

    Ok(Schedule {
        route_info,
        station_name,
        headsign,
        departures_by_weekday,
    })
}

fn load_route_info(dir: &Path, route_id: &str) -> Result<RouteInfo, Box<dyn std::error::Error>> {
    let mut rdr = csv::Reader::from_path(dir.join("routes.txt"))?;
    for result in rdr.deserialize() {
        let route: GtfsRoute = result?;
        if route.route_id == route_id {
            return Ok(RouteInfo {
                short_name: route.route_short_name,
                color: format!("#{}", route.route_color),
            });
        }
    }
    Err(format!("Route {} not found", route_id).into())
}

fn load_station_name(dir: &Path, stop_id: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut rdr = csv::Reader::from_path(dir.join("stops.txt"))?;
    // First try to find the stop itself, then use its parent station name if available
    let mut stop_name = None;
    let mut parent_id = None;
    let mut parent_names: HashMap<String, String> = HashMap::new();

    for result in rdr.deserialize() {
        let stop: GtfsStop = result?;
        if stop.stop_id == stop_id {
            stop_name = Some(stop.stop_name.clone());
            parent_id = stop.parent_station.clone();
        }
        parent_names.insert(stop.stop_id.clone(), stop.stop_name);
    }

    // Prefer parent station name (cleaner display)
    if let Some(pid) = parent_id {
        if let Some(name) = parent_names.get(&pid) {
            return Ok(name.clone());
        }
    }

    stop_name.ok_or_else(|| format!("Stop {} not found", stop_id).into())
}

fn load_calendar(
    dir: &Path,
) -> Result<HashMap<String, Vec<Weekday>>, Box<dyn std::error::Error>> {
    let mut rdr = csv::Reader::from_path(dir.join("calendar.txt"))?;
    let mut map = HashMap::new();

    for result in rdr.deserialize() {
        let cal: GtfsCalendar = result?;
        let mut weekdays = Vec::new();
        if cal.monday == 1 { weekdays.push(Weekday::Mon); }
        if cal.tuesday == 1 { weekdays.push(Weekday::Tue); }
        if cal.wednesday == 1 { weekdays.push(Weekday::Wed); }
        if cal.thursday == 1 { weekdays.push(Weekday::Thu); }
        if cal.friday == 1 { weekdays.push(Weekday::Fri); }
        if cal.saturday == 1 { weekdays.push(Weekday::Sat); }
        if cal.sunday == 1 { weekdays.push(Weekday::Sun); }
        map.insert(cal.service_id, weekdays);
    }

    Ok(map)
}

fn load_trips(
    dir: &Path,
    route_id: &str,
    direction_id: u8,
) -> Result<(HashMap<String, Vec<String>>, String), Box<dyn std::error::Error>> {
    let mut rdr = csv::Reader::from_path(dir.join("trips.txt"))?;
    let mut trips_by_service: HashMap<String, Vec<String>> = HashMap::new();
    let mut headsign = String::new();

    for result in rdr.deserialize() {
        let trip: GtfsTrip = result?;
        if trip.route_id == route_id && trip.direction_id == direction_id {
            if headsign.is_empty() {
                headsign = trip.trip_headsign.clone();
            }
            trips_by_service
                .entry(trip.service_id)
                .or_default()
                .push(trip.trip_id);
        }
    }

    Ok((trips_by_service, headsign))
}

fn load_stop_times(
    dir: &Path,
    trip_ids: &HashSet<&str>,
    stop_id: &str,
) -> Result<HashMap<String, (NaiveTime, String)>, Box<dyn std::error::Error>> {
    let mut rdr = csv::Reader::from_path(dir.join("stop_times.txt"))?;
    let mut map = HashMap::new();

    for result in rdr.deserialize() {
        let st: GtfsStopTime = result?;
        if trip_ids.contains(st.trip_id.as_str()) && st.stop_id == stop_id {
            if let Some(time) = parse_gtfs_time(&st.departure_time) {
                let formatted = format_time_hhmm(time);
                map.insert(st.trip_id, (time, formatted));
            }
        }
    }

    Ok(map)
}
