use chrono_tz::Tz;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};

use crate::error::{Result, TransitError};

pub const LANGUAGE: &str = "de";

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Match {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(deserialize_with = "non_empty")]
    pub name: Option<String>,
    #[serde(deserialize_with = "non_empty")]
    pub id: Option<String>,
    pub lat: f64,
    pub lon: f64,
    #[serde(deserialize_with = "non_empty")]
    pub street: Option<String>,
    #[serde(deserialize_with = "non_empty")]
    pub house_number: Option<String>,
    pub areas: Vec<Area>,
    pub modes: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Area {
    #[serde(deserialize_with = "non_empty")]
    pub name: Option<String>,
    pub default: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Place {
    #[serde(deserialize_with = "non_empty")]
    pub name: Option<String>,
    #[serde(deserialize_with = "non_empty")]
    pub stop_id: Option<String>,
    #[serde(deserialize_with = "non_empty")]
    pub tz: Option<String>,
    #[serde(deserialize_with = "non_empty")]
    pub arrival: Option<String>,
    #[serde(deserialize_with = "non_empty")]
    pub departure: Option<String>,
    #[serde(deserialize_with = "non_empty")]
    pub scheduled_arrival: Option<String>,
    #[serde(deserialize_with = "non_empty")]
    pub scheduled_departure: Option<String>,
    #[serde(deserialize_with = "non_empty")]
    pub track: Option<String>,
    #[serde(deserialize_with = "non_empty")]
    pub scheduled_track: Option<String>,
    pub cancelled: bool,
    pub alerts: Vec<Alert>,
}

impl Place {
    pub fn zone(&self) -> Option<Tz> {
        self.tz.as_deref()?.parse().ok()
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Alert {
    pub header_text: String,
    pub description_text: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Itinerary {
    pub duration: i64,
    #[serde(deserialize_with = "non_empty")]
    pub start_time: Option<String>,
    #[serde(deserialize_with = "non_empty")]
    pub end_time: Option<String>,
    pub transfers: i64,
    pub legs: Vec<Leg>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Leg {
    pub mode: String,
    pub from: Place,
    pub to: Place,
    pub duration: i64,
    pub distance: Option<f64>,
    #[serde(deserialize_with = "non_empty")]
    pub headsign: Option<String>,
    #[serde(deserialize_with = "non_empty")]
    pub agency_name: Option<String>,
    #[serde(deserialize_with = "non_empty")]
    pub trip_id: Option<String>,
    #[serde(deserialize_with = "non_empty")]
    pub route_short_name: Option<String>,
    #[serde(deserialize_with = "non_empty")]
    pub display_name: Option<String>,
    pub real_time: bool,
    pub cancelled: bool,
    pub intermediate_stops: Vec<Place>,
    pub steps: Vec<Step>,
    pub bikes_allowed: Option<bool>,
    #[serde(deserialize_with = "non_empty")]
    pub wheelchair_accessible: Option<String>,
    #[serde(deserialize_with = "non_empty")]
    pub reservation: Option<String>,
    pub alerts: Vec<Alert>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Step {
    #[serde(deserialize_with = "non_empty")]
    pub relative_direction: Option<String>,
    pub distance: f64,
    pub from_level: f64,
    pub to_level: f64,
    #[serde(deserialize_with = "non_empty")]
    pub street_name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PlanResponse {
    pub itineraries: Vec<Itinerary>,
    pub direct: Vec<Itinerary>,
    #[serde(deserialize_with = "non_empty")]
    pub previous_page_cursor: Option<String>,
    #[serde(deserialize_with = "non_empty")]
    pub next_page_cursor: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StopTime {
    pub place: Place,
    #[serde(deserialize_with = "non_empty")]
    pub mode: Option<String>,
    pub real_time: bool,
    #[serde(deserialize_with = "non_empty")]
    pub headsign: Option<String>,
    pub trip_from: Option<Place>,
    pub trip_to: Option<Place>,
    #[serde(deserialize_with = "non_empty")]
    pub agency_name: Option<String>,
    #[serde(deserialize_with = "non_empty")]
    pub trip_id: Option<String>,
    #[serde(deserialize_with = "non_empty")]
    pub route_short_name: Option<String>,
    #[serde(deserialize_with = "non_empty")]
    pub display_name: Option<String>,
    pub cancelled: bool,
    pub trip_cancelled: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StopTimesResponse {
    pub place: Option<Place>,
    pub stop_times: Vec<StopTime>,
    #[serde(deserialize_with = "non_empty")]
    pub previous_page_cursor: Option<String>,
    #[serde(deserialize_with = "non_empty")]
    pub next_page_cursor: Option<String>,
}

#[derive(Debug, Default)]
pub struct Query(Vec<(&'static str, String)>);

impl Query {
    pub fn set(&mut self, key: &'static str, value: impl ToString) -> &mut Self {
        self.0.push((key, value.to_string()));
        self
    }

    pub fn set_some(&mut self, key: &'static str, value: Option<impl ToString>) -> &mut Self {
        if let Some(value) = value {
            self.set(key, value);
        }
        self
    }

    pub fn set_list(&mut self, key: &'static str, values: &[String]) -> &mut Self {
        if !values.is_empty() {
            self.set(key, values.join(","));
        }
        self
    }
}

pub struct TransitousClient {
    http: reqwest::Client,
    base_url: String,
}

impl TransitousClient {
    pub fn new(http: reqwest::Client, base_url: impl Into<String>) -> Self {
        Self {
            http,
            base_url: base_url.into(),
        }
    }

    pub async fn geocode(
        &self,
        text: &str,
        types: &[&str],
        near: Option<&str>,
        limit: u8,
    ) -> Result<Vec<Match>> {
        let mut query = Query::default();
        query
            .set("text", text)
            .set("numResults", limit)
            .set("language", LANGUAGE)
            .set_some("place", near);
        if !types.is_empty() {
            query.set("type", types.join(","));
        }
        self.get("/api/v1/geocode", &query).await
    }

    pub async fn plan(&self, query: &Query) -> Result<PlanResponse> {
        self.get("/api/v6/plan", query).await
    }

    pub async fn stoptimes(&self, query: &Query) -> Result<StopTimesResponse> {
        self.get("/api/v6/stoptimes", query).await
    }

    pub async fn trip(&self, trip_id: &str) -> Result<Itinerary> {
        let mut query = Query::default();
        query
            .set("tripId", trip_id)
            .set("detailedLegs", false)
            .set("language", LANGUAGE);
        self.get("/api/v6/trip", &query).await
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, query: &Query) -> Result<T> {
        let response = self
            .http
            .get(format!("{}{path}", self.base_url))
            .query(&query.0)
            .send()
            .await
            .map_err(|error| {
                TransitError::new(format!("The routing service could not be reached: {error}"))
            })?;
        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(TransitError::new(
                "The routing service is rate limiting requests. Try again later.",
            ));
        }
        let body = response.bytes().await.map_err(|error| {
            TransitError::new(format!("The routing service could not be reached: {error}"))
        })?;
        if status.is_client_error() || status.is_server_error() {
            return Err(TransitError::new(format!(
                "The routing service answered with HTTP {}: {}",
                status.as_u16(),
                error_text(&body)
            )));
        }
        serde_json::from_slice(&body)
            .map_err(|_| TransitError::new("The routing service sent an unreadable answer."))
    }
}

fn error_text(body: &[u8]) -> String {
    #[derive(Deserialize)]
    struct ErrorBody {
        error: serde_json::Value,
    }
    match serde_json::from_slice::<ErrorBody>(body) {
        Ok(ErrorBody {
            error: serde_json::Value::String(text),
        }) => text,
        Ok(ErrorBody { error }) => error.to_string(),
        Err(_) => String::from_utf8_lossy(body).chars().take(300).collect(),
    }
}

fn non_empty<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error> {
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(value.filter(|text| !text.is_empty()))
}
