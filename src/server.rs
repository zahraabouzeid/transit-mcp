use std::sync::{Arc, LazyLock};

use chrono::{DateTime, Duration, FixedOffset, Utc};
use chrono_tz::Tz;
use regex::Regex;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::{Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::db_timetables::{DbTimetablesClient, Event, Stop};
use crate::error::{self, TransitError};
use crate::model::{self, BoardEntry, Journey, PlaceInfo, TripStop};
use crate::times::{format_minutes, minutes, now_in, parse_db_time, parse_user_time, to_api_time};
use crate::transitous::{LANGUAGE, Query, TransitousClient};

static COORDINATES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*-?\d{1,3}(\.\d+)?\s*,\s*-?\d{1,3}(\.\d+)?\s*$").expect("valid pattern")
});
static EVA_NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d{7}$").expect("valid pattern"));

const TRANSITOUS_SOURCE: &str = "Transitous, https://transitous.org/sources/";
const DB_SOURCE: &str = "Deutsche Bahn Timetables API, CC BY 4.0";

const INSTRUCTIONS: &str = "\
You answer questions about travelling by public transport, the way a journey planner app does:
how to get from A to B, where to change, which platform a train leaves from, whether it is late,
and what leaves from a stop next. The data covers Germany and many other countries through
Transitous, a free routing service built on open timetables with live updates where operators
publish them. For German railway stations, the DB timetable adds live platform changes.

How to work with the tools:
- Use plan_journey for any trip from A to B. It accepts stop names, addresses, place names or
  coordinates, so you can pass what the user said and do not need find_place first.
- Use find_place when a name is ambiguous, for example \"Hauptbahnhof\" without a city, and show
  the user the options before planning.
- Use departures for \"what leaves from X\" and departures with arrivals=true for \"what arrives\".
- Use trip_details with a trip_id from another result to list every stop of one train or bus.
- Use db_station_board for German railway stations when the user cares about platforms and
  platform changes right now. It covers DB stations only, not bus stops.

When you answer, lead with the departure time, the line and the platform, then each change with
its platform and walking time. Mention delays, cancellations and platform changes clearly,
because those are what users most often miss. Times are local to the stop. A missing platform
means the data source has none, so say that instead of guessing.
";

#[derive(Clone)]
pub struct TransitServer {
    transitous: Arc<TransitousClient>,
    db: Option<Arc<DbTimetablesClient>>,
    zone: Tz,
    tool_router: ToolRouter<Self>,
}

// The doc comments on the argument fields are the parameter descriptions the
// MCP client sees.

#[derive(Deserialize, JsonSchema)]
struct FindPlaceArgs {
    /// Name of a stop, address or place.
    query: String,
    /// Limit results to 'stop', 'address', 'place', or 'any'.
    #[serde(default = "any_kind")]
    kind: String,
    /// Optional 'latitude,longitude' to prefer results near it.
    near: Option<String>,
    #[serde(default = "find_place_limit")]
    #[schemars(range(min = 1, max = 20))]
    limit: u8,
}

#[derive(Deserialize, JsonSchema)]
struct PlanJourneyArgs {
    /// Start: stop name, address, place, stop id or 'lat,lon'.
    origin: String,
    /// Destination: stop name, address, place, stop id or 'lat,lon'.
    destination: String,
    /// 'HH:MM' for today or ISO date and time. Defaults to now.
    time: Option<String>,
    /// True if time is the latest arrival instead of departure.
    #[serde(default)]
    arrive_by: bool,
    /// Stops the journey must pass through.
    via: Option<Vec<String>>,
    #[schemars(range(min = 0, max = 10))]
    max_transfers: Option<u8>,
    /// Extra buffer for each change.
    #[schemars(range(min = 0, max = 60))]
    min_transfer_minutes: Option<u8>,
    /// Allowed modes, for example ['REGIONAL_RAIL', 'SUBURBAN', 'BUS', 'TRAM', 'SUBWAY',
    /// 'LONG_DISTANCE', 'HIGHSPEED_RAIL', 'FERRY']. Default is all.
    transit_modes: Option<Vec<String>>,
    /// Route walking parts for wheelchair users.
    #[serde(default)]
    wheelchair: bool,
    /// Only use vehicles that allow taking a bike.
    #[serde(default)]
    with_bike: bool,
    /// Add turn-by-turn steps for walking parts. Makes the answer longer.
    #[serde(default)]
    walking_directions: bool,
    /// Cursor from a previous result to get earlier or later journeys.
    page_cursor: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct DeparturesArgs {
    /// Stop name, stop id or 'lat,lon' of a stop.
    stop: String,
    /// 'HH:MM' for today or ISO date and time. Defaults to now.
    time: Option<String>,
    /// True to list arrivals instead of departures.
    #[serde(default)]
    arrivals: bool,
    /// Roughly how many entries to return. The result can hold a few more, so
    /// show the user only as many as they asked for.
    #[serde(default = "departures_limit")]
    #[schemars(range(min = 1, max = 50))]
    limit: u8,
    /// Only these modes, for example ['BUS'] or ['REGIONAL_RAIL'].
    modes: Option<Vec<String>>,
    /// Cursor from a previous result to page further.
    page_cursor: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct TripDetailsArgs {
    /// trip_id from plan_journey or departures.
    trip_id: String,
}

#[derive(Deserialize, JsonSchema)]
struct StationBoardArgs {
    /// German railway station name or 7-digit EVA number.
    station: String,
    /// 'HH:MM' for today or ISO date and time. Defaults to now.
    time: Option<String>,
    /// How many hours to show.
    #[serde(default = "board_hours")]
    #[schemars(range(min = 1, max = 3))]
    hours: u8,
    /// True to list arrivals instead of departures.
    #[serde(default)]
    arrivals: bool,
}

fn any_kind() -> String {
    "any".into()
}

fn find_place_limit() -> u8 {
    8
}

fn departures_limit() -> u8 {
    15
}

fn board_hours() -> u8 {
    1
}

#[derive(Serialize, JsonSchema)]
pub struct FindPlaceResult {
    results: Vec<PlaceInfo>,
    source: String,
}

#[derive(Serialize, JsonSchema)]
pub struct PlanJourneyResult {
    from: PlaceInfo,
    to: PlaceInfo,
    journeys: Vec<Journey>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    walk_only: Vec<Journey>,
    #[serde(skip_serializing_if = "Option::is_none")]
    earlier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    later: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    source: String,
}

#[derive(Serialize, JsonSchema)]
pub struct DeparturesResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    departures: Option<Vec<BoardEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    arrivals: Option<Vec<BoardEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    earlier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    later: Option<String>,
    source: String,
}

#[derive(Serialize, JsonSchema)]
pub struct TripDetailsResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    direction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    operator: Option<String>,
    #[serde(default, skip_serializing_if = "model::is_false")]
    cancelled: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    alerts: Vec<String>,
    stops: Vec<TripStop>,
    source: String,
}

#[derive(Serialize, JsonSchema)]
pub struct StationBoardResult {
    station: String,
    eva: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    departures: Option<Vec<Train>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    arrivals: Option<Vec<Train>>,
    source: String,
}

#[derive(Serialize, JsonSchema)]
pub struct Train {
    #[serde(skip_serializing_if = "Option::is_none")]
    train: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    number: Option<String>,
    planned: String,
    expected: String,
    delay_min: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    platform: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    planned_platform: Option<String>,
    #[serde(default, skip_serializing_if = "model::is_false")]
    platform_changed: bool,
    #[serde(default, skip_serializing_if = "model::is_false")]
    cancelled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    to: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    stops: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    alerts: Vec<String>,
    #[serde(skip)]
    #[schemars(skip)]
    planned_at: DateTime<FixedOffset>,
    #[serde(skip)]
    #[schemars(skip)]
    expected_at: DateTime<FixedOffset>,
}

#[tool_router]
impl TransitServer {
    pub fn new(transitous: TransitousClient, db: Option<DbTimetablesClient>, zone: Tz) -> Self {
        Self {
            transitous: Arc::new(transitous),
            db: db.map(Arc::new),
            zone,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Search stops, addresses and places by name.\n\n\
            Use this when a name could mean several places, so the user can pick the right one. \
            Each result has an id and coordinates. Pass a stop id or the coordinates to the other \
            tools to make sure they use exactly this place.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn find_place(
        &self,
        Parameters(args): Parameters<FindPlaceArgs>,
    ) -> Result<Json<FindPlaceResult>, String> {
        within("limit", args.limit, 1, 20)?;
        let types: &[&str] = match args.kind.to_lowercase().as_str() {
            "stop" => &["STOP"],
            "address" => &["ADDRESS"],
            "place" => &["PLACE"],
            _ => &[],
        };
        let matches = self
            .transitous
            .geocode(&args.query, types, args.near.as_deref(), args.limit)
            .await?;
        Ok(Json(FindPlaceResult {
            results: matches.into_iter().map(PlaceInfo::from).collect(),
            source: TRANSITOUS_SOURCE.into(),
        }))
    }

    #[tool(
        description = "Plan a journey from A to B with public transport and walking.\n\n\
            Returns several journey options. Each has its legs in order: walking parts with \
            distance, and rides with line, direction, departure and arrival stop, platform, live \
            delay and alerts. Use 'earlier' or 'later' as page_cursor to see more options around \
            the time.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn plan_journey(
        &self,
        Parameters(args): Parameters<PlanJourneyArgs>,
    ) -> Result<Json<PlanJourneyResult>, String> {
        if let Some(value) = args.max_transfers {
            within("max_transfers", value, 0, 10)?;
        }
        if let Some(value) = args.min_transfer_minutes {
            within("min_transfer_minutes", value, 0, 60)?;
        }
        let moment = self.read_time(args.time.as_deref())?;
        let (from_place, from) = self.resolve(&args.origin, false).await?;
        let (to_place, to) = self.resolve(&args.destination, false).await?;
        let mut via = Vec::new();
        for stop in args.via.unwrap_or_default() {
            via.push(self.resolve(&stop, true).await?.0);
        }

        let mut query = Query::default();
        query
            .set("fromPlace", from_place)
            .set("toPlace", to_place)
            .set_some("time", moment.map(to_api_time))
            .set("arriveBy", args.arrive_by)
            .set_list("via", &via)
            .set_some("maxTransfers", args.max_transfers)
            .set_some("minTransferTime", args.min_transfer_minutes)
            .set_list("transitModes", &args.transit_modes.unwrap_or_default())
            .set_some("pedestrianProfile", args.wheelchair.then_some("WHEELCHAIR"))
            .set_some("requireBikeTransport", args.with_bike.then_some(true))
            .set("detailedLegs", args.walking_directions)
            .set("detailedTransfers", args.walking_directions)
            .set_some("pageCursor", args.page_cursor)
            .set("language", LANGUAGE);
        let result = self.transitous.plan(&query).await?;

        let to_journeys = |itineraries: Vec<_>| -> Vec<Journey> {
            itineraries
                .into_iter()
                .map(|raw| Journey::new(raw, self.zone, args.walking_directions))
                .collect()
        };
        let journeys = to_journeys(result.itineraries);
        let walk_only = to_journeys(result.direct);
        let note = (journeys.is_empty() && walk_only.is_empty())
            .then(|| "No connection found for this time.".to_owned());
        Ok(Json(PlanJourneyResult {
            from,
            to,
            journeys,
            walk_only,
            earlier: result.previous_page_cursor,
            later: result.next_page_cursor,
            note,
            source: TRANSITOUS_SOURCE.into(),
        }))
    }

    #[tool(
        description = "List the next departures or arrivals at a stop, like a departure board.\n\n\
            Each entry has the time with live delay, line, direction, platform, platform changes, \
            cancellations and alerts. The trip_id of an entry works with trip_details.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn departures(
        &self,
        Parameters(args): Parameters<DeparturesArgs>,
    ) -> Result<Json<DeparturesResult>, String> {
        within("limit", args.limit, 1, 50)?;
        let moment = self.read_time(args.time.as_deref())?;
        let (stop, info) = self.resolve(&args.stop, true).await?;

        let mut query = Query::default();
        query
            .set_some("time", moment.map(to_api_time))
            .set("arriveBy", args.arrivals)
            .set("n", args.limit)
            .set_list("mode", &args.modes.unwrap_or_default())
            .set_some("pageCursor", args.page_cursor)
            .set("language", LANGUAGE);
        if COORDINATES.is_match(&stop) {
            query.set("center", stop).set("radius", 150);
        } else {
            query.set("stopId", stop);
        }
        let result = self.transitous.stoptimes(&query).await?;

        let entries = result
            .stop_times
            .into_iter()
            .map(|raw| BoardEntry::new(raw, self.zone, args.arrivals))
            .collect();
        let (departures, arrivals) = if args.arrivals {
            (None, Some(entries))
        } else {
            (Some(entries), None)
        };
        Ok(Json(DeparturesResult {
            stop: result.place.and_then(|place| place.name).or(info.name),
            departures,
            arrivals,
            earlier: result.previous_page_cursor,
            later: result.next_page_cursor,
            source: TRANSITOUS_SOURCE.into(),
        }))
    }

    #[tool(
        description = "Show every stop of one train or bus run with times, platforms and live \
            delays.\n\n\
            Use it to answer questions like \"where does this train stop\" or \"when does it \
            reach X\".",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn trip_details(
        &self,
        Parameters(args): Parameters<TripDetailsArgs>,
    ) -> Result<Json<TripDetailsResult>, String> {
        let result = self.transitous.trip(&args.trip_id).await?;
        let legs: Vec<_> = result
            .legs
            .into_iter()
            .filter(|leg| leg.trip_id.is_some())
            .collect();
        let Some(first) = legs.first() else {
            return Err("This trip was not found. It may have ended or the id is old.".into());
        };
        let stops = legs
            .iter()
            .flat_map(|leg| {
                std::iter::once(&leg.from)
                    .chain(&leg.intermediate_stops)
                    .chain(std::iter::once(&leg.to))
            })
            .map(|place| TripStop::new(place, self.zone))
            .collect();
        Ok(Json(TripDetailsResult {
            line: first
                .display_name
                .clone()
                .or_else(|| first.route_short_name.clone()),
            direction: first.headsign.clone(),
            operator: first.agency_name.clone(),
            cancelled: first.cancelled,
            alerts: model::alert_texts(&first.alerts),
            stops,
            source: TRANSITOUS_SOURCE.into(),
        }))
    }

    #[tool(
        description = "Show the live DB board of a German railway station with platforms.\n\n\
            This comes straight from Deutsche Bahn and is the most reliable source for current \
            platforms, platform changes, delays and cancellations of trains in Germany. It does \
            not include buses or trams. Use departures or plan_journey for those.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn db_station_board(
        &self,
        Parameters(args): Parameters<StationBoardArgs>,
    ) -> Result<Json<StationBoardResult>, String> {
        let Some(db) = &self.db else {
            return Err(
                "The DB timetable is not configured on this server. Use departures instead.".into(),
            );
        };
        within("hours", args.hours, 1, 3)?;
        let start = self
            .read_time(args.time.as_deref())?
            .unwrap_or_else(|| now_in(self.zone));
        let (name, eva) = find_station(db, &args.station).await?;
        let stops = db.board(&eva, start, args.hours, self.zone).await?;
        let end = start + Duration::hours(i64::from(args.hours));

        let mut trains: Vec<Train> = stops
            .iter()
            .filter_map(|stop| Train::new(stop, args.arrivals, self.zone))
            .filter(|train| {
                train.expected_at >= start - Duration::minutes(1) && train.planned_at < end
            })
            .collect();
        trains.sort_by_key(|train| train.planned_at);
        let (departures, arrivals) = if args.arrivals {
            (None, Some(trains))
        } else {
            (Some(trains), None)
        };
        Ok(Json(StationBoardResult {
            station: name,
            eva,
            departures,
            arrivals,
            source: DB_SOURCE.into(),
        }))
    }
}

impl TransitServer {
    fn read_time(&self, value: Option<&str>) -> error::Result<Option<DateTime<FixedOffset>>> {
        parse_user_time(value, self.zone, Utc::now())
    }

    async fn resolve(&self, value: &str, stops_only: bool) -> error::Result<(String, PlaceInfo)> {
        let value = value.trim();
        if COORDINATES.is_match(value) {
            let info = PlaceInfo {
                name: Some(value.to_owned()),
                kind: "COORDINATES".into(),
                ..PlaceInfo::default()
            };
            return Ok((value.replace(' ', ""), info));
        }
        if looks_like_stop_id(value) {
            let info = PlaceInfo {
                id: Some(value.to_owned()),
                kind: "STOP".into(),
                ..PlaceInfo::default()
            };
            return Ok((value.to_owned(), info));
        }
        let types: &[&str] = if stops_only { &["STOP"] } else { &[] };
        let found = self
            .transitous
            .geocode(value, types, None, 1)
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| {
                TransitError::new(format!(
                    "No place found for {value:?}. Try a more specific name."
                ))
            })?;
        let info = PlaceInfo::from(found);
        let place = match (&info.id, info.kind.as_str()) {
            (Some(id), "STOP") => id.clone(),
            _ => info.coordinates.clone().unwrap_or_default(),
        };
        Ok((place, info))
    }
}

impl Train {
    fn new(stop: &Stop, arrivals: bool, zone: Tz) -> Option<Self> {
        let event: &Event = if arrivals {
            stop.arrival.as_ref()?
        } else {
            stop.departure.as_ref()?
        };
        if event.hidden {
            return None;
        }
        let planned_at = parse_db_time(event.planned_time.as_deref(), zone)?;
        let expected_at = parse_db_time(event.changed_time.as_deref(), zone).unwrap_or(planned_at);
        let stations = event.stations();
        let (from, to) = if arrivals {
            (stations.first().cloned(), None)
        } else {
            (None, stations.last().cloned())
        };
        Some(Self {
            train: train_name(
                stop.category.as_deref(),
                event.line.as_deref(),
                stop.number.as_deref(),
            ),
            category: stop.category.clone(),
            number: stop.number.clone(),
            planned: format_minutes(planned_at),
            expected: format_minutes(expected_at),
            delay_min: minutes(expected_at - planned_at),
            platform: event.platform().map(str::to_owned),
            planned_platform: event.planned_platform.clone(),
            platform_changed: event.platform_changed(),
            cancelled: event.is_cancelled(),
            from,
            to,
            stops: stations,
            alerts: stop.message_texts(),
            planned_at,
            expected_at,
        })
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for TransitServer {
    fn get_info(&self) -> ServerConfig {
        let capabilities = ServerCapabilities::builder().enable_tools().build();
        ServerConfig::new(capabilities)
            .with_server_info(
                Implementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
                    .with_description(env!("CARGO_PKG_DESCRIPTION"))
                    .with_website_url(env!("CARGO_PKG_REPOSITORY")),
            )
            .with_instructions(INSTRUCTIONS)
    }
}

async fn find_station(db: &DbTimetablesClient, station: &str) -> error::Result<(String, String)> {
    let value = station.trim();
    if EVA_NUMBER.is_match(value) {
        return Ok((value.to_owned(), value.to_owned()));
    }
    let found = db
        .find_stations(value)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| TransitError::new(format!("No DB station found for {station:?}.")))?;
    Ok((found.name, found.eva))
}

fn train_name(category: Option<&str>, line: Option<&str>, number: Option<&str>) -> Option<String> {
    let Some(category) = category.filter(|category| !category.is_empty()) else {
        return line.or(number).map(str::to_owned);
    };
    match (line, number) {
        (Some(line), _) if line.starts_with(category) => Some(line.to_owned()),
        (Some(line), _) => Some(format!("{category} {line}")),
        (None, Some(number)) => Some(format!("{category} {number}")),
        (None, None) => Some(category.to_owned()),
    }
}

fn looks_like_stop_id(value: &str) -> bool {
    !value.contains(' ') && value.contains('_') && value.contains(':')
}

fn within(name: &str, value: u8, min: u8, max: u8) -> Result<(), String> {
    if (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(format!(
            "{name} must be between {min} and {max}, not {value}."
        ))
    }
}
