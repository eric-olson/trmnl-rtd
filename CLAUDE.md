# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Cloudflare Worker (Rust/WASM) that serves real-time RTD train departure data for a TRMNL E-Ink display. Fetches GTFS-RT protobuf feeds from RTD Denver, merges with static schedule CSVs stored in R2, and outputs JSON. Configured for the G Line (eastbound) from Wheat Ridge / Ward Road station.

## Build & Dev

```bash
# Local dev server (requires wrangler)
npx wrangler dev

# Build only (installs worker-build if needed)
cargo install -q worker-build && worker-build --release

# Deploy to Cloudflare
npx wrangler deploy

# Protobuf codegen runs automatically via build.rs
```

There are no tests yet. The project has no linter configured.

## Architecture

**Cloudflare Worker with two event handlers** (`src/lib.rs`):
- `fetch` — HTTP GET returns departure JSON (for `wrangler dev` testing)
- `scheduled` — cron dispatcher: `*/15 * * * *` runs departure logic, `0 3 * * 1` triggers weekly GTFS refresh

**Data flow:**
1. `refresh.rs` — weekly cron downloads RTD's GTFS zip, extracts 5 CSV files, uploads to R2 bucket
2. `schedule.rs` — parses GTFS CSVs (routes, trips, stop_times, stops, calendar) into a `Schedule` with departures indexed by weekday
3. `gtfs_rt.rs` — fetches live protobuf feeds (TripUpdate + Alerts) from RTD, parses via prost-generated types from `proto/gtfs-realtime.proto`
4. `lib.rs` — merges scheduled departures with realtime data (delay/estimated time), produces final JSON output

**Key types:**
- `Config` (config.rs) — all settings from wrangler.toml `[vars]`, with hardcoded defaults
- `Schedule` / `ScheduledDeparture` (schedule.rs) — static timetable data
- `RealtimeData` / `RealtimeDeparture` / `Alert` (gtfs_rt.rs) — live feed data

**Protobuf:** `proto/gtfs-realtime.proto` is compiled by `build.rs` using prost-build. Generated code is included at `gtfs_rt::transit_realtime`.

**R2 bucket:** `trmnl-rtd-gtfs` (binding: `GTFS_BUCKET`) stores the extracted GTFS CSV files.

## GTFS quirks

- GTFS departure times can exceed 24:00:00 for next-day service; `parse_gtfs_time` handles this with `h % 24`
- Stop IDs may reference child stops; `load_station_name` resolves to parent station name when available
- Realtime stop events prefer departure over arrival; fall back to trip-level delay if no stop-level event exists
