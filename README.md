# trmnl-rtd

Real-time RTD train departures for a [TRMNL](https://usetrmnl.com/) E-Ink display.

Fetches GTFS-RT feeds from RTD Denver, merges them with static schedule data, and outputs JSON with upcoming departures and active alerts. Currently configured for the **G Line** (eastbound toward Union Station) from **Wheat Ridge / Ward Road** station.

## Usage

Download the [RTD Commuter Rail GTFS](https://www.rtd-denver.com/open-data) static data and extract it:

```
mkdir rtd-purchased-commuter
unzip RTD_Denver_Purchased_Transportation_Commuter_Rail_GTFS.zip -d rtd-purchased-commuter
```

Run:

```
cargo run
```

### Configuration

All settings have defaults for the G Line at Wheat Ridge / Ward Road. Override via environment variables:

| Variable | Default | Description |
|---|---|---|
| `GTFS_ROUTE_ID` | `113G` | Route ID |
| `GTFS_STOP_ID` | `34510` | Stop ID |
| `GTFS_DIRECTION_ID` | `0` | Direction (0 = eastbound) |
| `GTFS_DIR` | `rtd-purchased-commuter` | Path to static GTFS files |
| `GTFS_DEPARTURE_COUNT` | `5` | Number of upcoming departures |

### Example output

```json
{
  "station": "Wheat Ridge / Ward Road Station",
  "line": "G",
  "line_color": "009B3A",
  "direction": "Union Station",
  "departures": [
    { "scheduled": "07:15", "estimated": null, "status": "on_time" },
    { "scheduled": "07:45", "estimated": "07:48", "status": "delayed" }
  ],
  "alerts": [],
  "updated_at": "2026-03-29T07:10:00-06:00"
}
```

## Roadmap

- [ ] CloudFlare Worker with cron trigger
- [ ] Static GTFS data in R2
- [ ] TRMNL webhook integration
- [ ] Mobile-friendly status page
