use chrono::{DateTime, FixedOffset};
use chrono_tz::Tz;
use futures::future::try_join_all;
use reqwest::Url;
use roxmltree::{Document, Node};

use crate::db_messages::message_text;
use crate::error::{Result, TransitError};
use crate::times::{db_slot, hour_slots};

const CANCELLED: &str = "c";

#[derive(Debug, Clone, Default)]
pub struct Event {
    pub planned_time: Option<String>,
    pub changed_time: Option<String>,
    pub planned_platform: Option<String>,
    pub changed_platform: Option<String>,
    pub planned_path: Option<String>,
    pub changed_path: Option<String>,
    pub line: Option<String>,
    pub status: Option<String>,
    pub hidden: bool,
}

impl Event {
    pub fn is_cancelled(&self) -> bool {
        self.status.as_deref() == Some(CANCELLED)
    }

    pub fn platform(&self) -> Option<&str> {
        self.changed_platform
            .as_deref()
            .or(self.planned_platform.as_deref())
    }

    pub fn platform_changed(&self) -> bool {
        self.changed_platform.is_some() && self.changed_platform != self.planned_platform
    }

    pub fn stations(&self) -> Vec<String> {
        self.changed_path
            .as_deref()
            .or(self.planned_path.as_deref())
            .unwrap_or_default()
            .split('|')
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Stop {
    pub id: String,
    pub category: Option<String>,
    pub number: Option<String>,
    pub arrival: Option<Event>,
    pub departure: Option<Event>,
    messages: Vec<(String, &'static str)>,
}

impl Stop {
    pub fn message_texts(&self) -> Vec<String> {
        let mut texts: Vec<String> = Vec::new();
        for (_, text) in &self.messages {
            if !texts.iter().any(|known| known == text) {
                texts.push((*text).to_owned());
            }
        }
        texts
    }

    fn add_message(&mut self, id: String, text: &'static str) {
        match self.messages.iter_mut().find(|(known, _)| *known == id) {
            Some(message) => message.1 = text,
            None => self.messages.push((id, text)),
        }
    }
}

pub struct Station {
    pub name: String,
    pub eva: String,
}

pub struct DbTimetablesClient {
    http: reqwest::Client,
    base_url: String,
    client_id: String,
    api_key: String,
}

impl DbTimetablesClient {
    pub fn new(
        http: reqwest::Client,
        base_url: impl Into<String>,
        client_id: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Self {
        Self {
            http,
            base_url: base_url.into(),
            client_id: client_id.into(),
            api_key: api_key.into(),
        }
    }

    pub async fn find_stations(&self, pattern: &str) -> Result<Vec<Station>> {
        let Some(body) = self.get(&["station", pattern]).await? else {
            return Ok(Vec::new());
        };
        let document = parse(&body)?;
        Ok(elements(&document, "station")
            .map(|station| Station {
                name: attribute(station, "name").unwrap_or_default(),
                eva: attribute(station, "eva").unwrap_or_default(),
            })
            .collect())
    }

    pub async fn board(
        &self,
        eva: &str,
        start: DateTime<FixedOffset>,
        hours: u8,
        zone: Tz,
    ) -> Result<Vec<Stop>> {
        let plans = try_join_all(hour_slots(start, hours).into_iter().map(|slot| {
            let (day, hour) = db_slot(slot, zone);
            async move { self.get(&["plan", eva, &day, &hour]).await }
        }))
        .await?;
        let changes = self.get(&["fchg", eva]).await?;

        let mut stops: Vec<Stop> = Vec::new();
        for body in plans.iter().flatten() {
            for element in elements(&parse(body)?, "s") {
                let stop = plan_stop(element);
                match stops.iter_mut().find(|known| known.id == stop.id) {
                    Some(known) => *known = stop,
                    None => stops.push(stop),
                }
            }
        }
        if let Some(body) = changes {
            for element in elements(&parse(&body)?, "s") {
                apply_changes(&mut stops, element);
            }
        }
        Ok(stops)
    }

    async fn get(&self, segments: &[&str]) -> Result<Option<String>> {
        let mut url = Url::parse(&self.base_url).map_err(|_| {
            TransitError::new("The DB timetable address on this server is not a valid URL.")
        })?;
        url.path_segments_mut()
            .map_err(|_| {
                TransitError::new("The DB timetable address on this server is not a valid URL.")
            })?
            .extend(segments);
        let response = self
            .http
            .get(url)
            .header("DB-Client-Id", &self.client_id)
            .header("DB-Api-Key", &self.api_key)
            .header(reqwest::header::ACCEPT, "application/xml")
            .send()
            .await
            .map_err(|error| {
                TransitError::new(format!(
                    "The DB timetable service could not be reached: {error}"
                ))
            })?;
        let status = response.status();
        match status.as_u16() {
            404 => return Ok(None),
            401 | 403 => {
                return Err(TransitError::new(
                    "The DB timetable service rejected the credentials. Check that DB_CLIENT_ID \
                     and DB_API_KEY belong together and that the application subscribes to \
                     Timetables.",
                ));
            }
            429 => {
                return Err(TransitError::new(
                    "The DB timetable service is rate limiting requests. Try again later.",
                ));
            }
            code if code >= 400 => {
                return Err(TransitError::new(format!(
                    "The DB timetable service answered with HTTP {code}."
                )));
            }
            _ => {}
        }
        let body = response.text().await.map_err(|error| {
            TransitError::new(format!(
                "The DB timetable service could not be reached: {error}"
            ))
        })?;
        Ok((!body.trim().is_empty()).then_some(body))
    }
}

fn parse(body: &str) -> Result<Document<'_>> {
    Document::parse(body)
        .map_err(|_| TransitError::new("The DB timetable service sent an unreadable answer."))
}

fn elements<'a, 'input>(
    document: &'a Document<'input>,
    name: &'static str,
) -> impl Iterator<Item = Node<'a, 'input>> {
    document
        .descendants()
        .filter(move |node| node.has_tag_name(name))
}

fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children().find(|child| child.has_tag_name(name))
}

fn attribute(node: Node<'_, '_>, name: &str) -> Option<String> {
    node.attribute(name).map(str::to_owned)
}

fn plan_stop(element: Node<'_, '_>) -> Stop {
    let label = child(element, "tl");
    Stop {
        id: attribute(element, "id").unwrap_or_default(),
        category: label.and_then(|label| attribute(label, "c")),
        number: label.and_then(|label| attribute(label, "n")),
        arrival: child(element, "ar").map(event),
        departure: child(element, "dp").map(event),
        messages: Vec::new(),
    }
}

fn event(element: Node<'_, '_>) -> Event {
    Event {
        planned_time: attribute(element, "pt"),
        changed_time: attribute(element, "ct"),
        planned_platform: attribute(element, "pp"),
        changed_platform: attribute(element, "cp"),
        planned_path: attribute(element, "ppth"),
        changed_path: attribute(element, "cpth"),
        line: attribute(element, "l"),
        status: attribute(element, "cs"),
        hidden: element.attribute("hi") == Some("1"),
    }
}

fn apply_changes(stops: &mut Vec<Stop>, element: Node<'_, '_>) {
    let id = attribute(element, "id").unwrap_or_default();
    let index = match stops.iter().position(|stop| stop.id == id) {
        Some(index) => index,
        None => {
            let stop = plan_stop(element);
            if stop.category.is_none() {
                return;
            }
            stops.push(stop);
            stops.len() - 1
        }
    };
    let stop = &mut stops[index];
    for message in element.descendants().filter(|node| node.has_tag_name("m")) {
        let Some(text) = message.attribute("c").and_then(message_text) else {
            continue;
        };
        if message.attribute("del") != Some("1") {
            let id = attribute(message, "id").unwrap_or_else(|| text.to_owned());
            stop.add_message(id, text);
        }
    }
    for (tag, current) in [("ar", &mut stop.arrival), ("dp", &mut stop.departure)] {
        let Some(change) = child(element, tag) else {
            continue;
        };
        match current {
            None => *current = Some(event(change)),
            Some(known) => {
                let update = |field: &mut Option<String>, name: &str| {
                    if let Some(value) = attribute(change, name) {
                        *field = Some(value);
                    }
                };
                update(&mut known.changed_time, "ct");
                update(&mut known.changed_platform, "cp");
                update(&mut known.changed_path, "cpth");
                update(&mut known.status, "cs");
            }
        }
    }
}
