use chrono_tz::Tz;
use schemars::JsonSchema;
use serde::Serialize;

use crate::times::{delay_minutes, local_minutes};
use crate::transitous::{self, Alert, Place};

const WALKING_MODES: [&str; 6] = [
    "WALK",
    "BIKE",
    "CAR",
    "RENTAL",
    "CAR_PARKING",
    "CAR_DROPOFF",
];

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct PlaceInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coordinates: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub area: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modes: Vec<String>,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct StopEvent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay_min: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_platform: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub platform_changed: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub cancelled: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alerts: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Journey {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub departure: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrival: Option<String>,
    pub duration_min: i64,
    pub transfers: i64,
    pub legs: Vec<JourneyLeg>,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct JourneyLeg {
    pub mode: String,
    pub duration_min: i64,
    pub from: StopEvent,
    pub to: StopEvent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance_m: Option<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<WalkStep>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operator: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trip_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub realtime: Option<bool>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub cancelled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stops_between: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bikes_allowed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wheelchair: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub reservation_required: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alerts: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct WalkStep {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub street: Option<String>,
    pub distance_m: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level_change: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BoardEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay_min: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_platform: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub platform_changed: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub cancelled: bool,
    pub realtime: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operator: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trip_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alerts: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TripStop {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrival: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrival_delay_min: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub departure: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub departure_delay_min: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub platform_changed: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub cancelled: bool,
}

#[derive(Clone, Copy)]
pub enum EventKind {
    Arrival,
    Departure,
}

impl From<transitous::Match> for PlaceInfo {
    fn from(found: transitous::Match) -> Self {
        let address = found.street.map(|street| match found.house_number {
            Some(number) => format!("{street} {number}"),
            None => street,
        });
        Self {
            id: found.id,
            name: found.name,
            kind: found.kind,
            coordinates: Some(format!("{},{}", found.lat, found.lon)),
            area: found
                .areas
                .into_iter()
                .find(|area| area.default)
                .and_then(|area| area.name),
            address,
            modes: found.modes,
        }
    }
}

impl StopEvent {
    pub fn new(place: &Place, zone: Tz, kind: EventKind) -> Self {
        let zone = place.zone().unwrap_or(zone);
        let (actual, scheduled) = match kind {
            EventKind::Arrival => (&place.arrival, &place.scheduled_arrival),
            EventKind::Departure => (&place.departure, &place.scheduled_departure),
        };
        let (actual, scheduled) = (actual.as_deref(), scheduled.as_deref());
        Self {
            name: place.name.clone(),
            stop_id: place.stop_id.clone(),
            time: local_minutes(actual, zone),
            scheduled_time: local_minutes(scheduled, zone),
            delay_min: delay_minutes(actual, scheduled),
            platform: place.track.clone(),
            scheduled_platform: place.scheduled_track.clone(),
            platform_changed: matches!(
                (&place.track, &place.scheduled_track),
                (Some(now), Some(planned)) if now != planned
            ),
            cancelled: place.cancelled,
            alerts: alert_texts(&place.alerts),
        }
    }
}

impl Journey {
    pub fn new(raw: transitous::Itinerary, zone: Tz, with_steps: bool) -> Self {
        let start_zone = raw.legs.first().and_then(|leg| leg.from.zone());
        let end_zone = raw.legs.last().and_then(|leg| leg.to.zone());
        Self {
            departure: local_minutes(raw.start_time.as_deref(), start_zone.unwrap_or(zone)),
            arrival: local_minutes(raw.end_time.as_deref(), end_zone.unwrap_or(zone)),
            duration_min: whole_minutes(raw.duration),
            transfers: raw.transfers,
            legs: raw
                .legs
                .into_iter()
                .map(|leg| JourneyLeg::new(leg, zone, with_steps))
                .collect(),
        }
    }
}

impl JourneyLeg {
    fn new(raw: transitous::Leg, zone: Tz, with_steps: bool) -> Self {
        let common = Self {
            duration_min: whole_minutes(raw.duration),
            from: StopEvent::new(&raw.from, zone, EventKind::Departure),
            to: StopEvent::new(&raw.to, zone, EventKind::Arrival),
            ..Self::default()
        };
        if WALKING_MODES.contains(&raw.mode.as_str()) {
            let steps = if with_steps {
                raw.steps.into_iter().map(WalkStep::from).collect()
            } else {
                Vec::new()
            };
            return Self {
                mode: raw.mode,
                distance_m: raw.distance.map(|distance| distance.round() as i64),
                steps,
                ..common
            };
        }
        Self {
            mode: raw.mode,
            line: raw.display_name.or(raw.route_short_name),
            direction: raw.headsign,
            operator: raw.agency_name,
            trip_id: raw.trip_id,
            realtime: Some(raw.real_time),
            cancelled: raw.cancelled,
            stops_between: Some(raw.intermediate_stops.len()),
            bikes_allowed: raw.bikes_allowed,
            wheelchair: raw.wheelchair_accessible,
            reservation_required: raw.reservation.as_deref() == Some("COMPULSORY"),
            alerts: alert_texts(&raw.alerts),
            ..common
        }
    }
}

impl From<transitous::Step> for WalkStep {
    fn from(raw: transitous::Step) -> Self {
        Self {
            direction: raw.relative_direction,
            street: raw.street_name,
            distance_m: raw.distance.round() as i64,
            level_change: (raw.from_level != raw.to_level)
                .then(|| format!("{} -> {}", raw.from_level, raw.to_level)),
        }
    }
}

impl BoardEntry {
    pub fn new(raw: transitous::StopTime, zone: Tz, arrivals: bool) -> Self {
        let kind = if arrivals {
            EventKind::Arrival
        } else {
            EventKind::Departure
        };
        let event = StopEvent::new(&raw.place, zone, kind);
        let (from, direction) = if arrivals {
            (raw.trip_from.and_then(|place| place.name), None)
        } else {
            let destination = raw.trip_to.and_then(|place| place.name);
            (None, raw.headsign.or(destination))
        };
        Self {
            time: event.time,
            scheduled_time: event.scheduled_time,
            delay_min: event.delay_min,
            line: raw.display_name.or(raw.route_short_name),
            mode: raw.mode,
            from,
            direction,
            stop: event.name,
            platform: event.platform,
            scheduled_platform: event.scheduled_platform,
            platform_changed: event.platform_changed,
            cancelled: raw.cancelled || raw.trip_cancelled,
            realtime: raw.real_time,
            operator: raw.agency_name,
            trip_id: raw.trip_id,
            alerts: event.alerts,
        }
    }
}

impl TripStop {
    pub fn new(place: &Place, zone: Tz) -> Self {
        let arrival = StopEvent::new(place, zone, EventKind::Arrival);
        let departure = StopEvent::new(place, zone, EventKind::Departure);
        Self {
            name: arrival.name,
            arrival: arrival.time,
            arrival_delay_min: arrival.delay_min,
            platform_changed: departure.platform_changed || arrival.platform_changed,
            platform: departure.platform.or(arrival.platform),
            departure: departure.time,
            departure_delay_min: departure.delay_min,
            cancelled: place.cancelled,
        }
    }
}

pub fn alert_texts(alerts: &[Alert]) -> Vec<String> {
    let mut texts: Vec<String> = Vec::new();
    for alert in alerts {
        let (header, description) = (&alert.header_text, &alert.description_text);
        let text = if description.is_empty() || description == header {
            header.clone()
        } else {
            format!("{header}: {description}")
        };
        if !text.is_empty() && !texts.contains(&text) {
            texts.push(text);
        }
    }
    texts
}

fn whole_minutes(seconds: i64) -> i64 {
    (seconds as f64 / 60.0).round() as i64
}

pub fn is_false(value: &bool) -> bool {
    !*value
}
