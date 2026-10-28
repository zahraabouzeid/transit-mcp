use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use reqwest::Url;
use serde_json::{Value, json};
use tower::ServiceExt;
use transit_mcp::config::Settings;
use transit_mcp::{app, build_server, http_client};

const GEOCODE_AACHEN: &str = include_str!("fixtures/geocode_aachen.json");
const GEOCODE_ADDRESS: &str = include_str!("fixtures/geocode_address.json");
const PLAN: &str = include_str!("fixtures/plan.json");
const STOPTIMES: &str = include_str!("fixtures/stoptimes.json");

type Recorded = Arc<Mutex<Vec<(String, HashMap<String, String>)>>>;

struct Apis {
    base: String,
    requests: Recorded,
}

impl Apis {
    async fn start() -> Self {
        let requests = Recorded::default();
        let router = Router::new().fallback(answer).with_state(requests.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a free port");
        let base = format!("http://{}", listener.local_addr().expect("an address"));
        tokio::spawn(async move { axum::serve(listener, router).await });
        Self { base, requests }
    }

    fn paths(&self) -> Vec<String> {
        self.recorded().into_iter().map(|(path, _)| path).collect()
    }

    fn last(&self, path: &str) -> HashMap<String, String> {
        self.recorded()
            .into_iter()
            .rev()
            .find(|(known, _)| known == path)
            .map(|(_, query)| query)
            .unwrap_or_else(|| panic!("no request to {path}"))
    }

    fn recorded(&self) -> Vec<(String, HashMap<String, String>)> {
        self.requests.lock().expect("not poisoned").clone()
    }
}

async fn answer(State(requests): State<Recorded>, request: Request) -> Response {
    let url = Url::parse(&format!("http://apis{}", request.uri())).expect("a valid URL");
    let path = url.path().to_owned();
    let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
    requests
        .lock()
        .expect("not poisoned")
        .push((path.clone(), query.clone()));
    let json = |body: &'static str| ([(header::CONTENT_TYPE, "application/json")], body);
    match path.as_str() {
        "/api/v1/geocode" => {
            let text = query.get("text").map(String::as_str).unwrap_or_default();
            if text.contains("Templergraben") {
                json(GEOCODE_ADDRESS).into_response()
            } else if text.contains("Nowhere") {
                json("[]").into_response()
            } else {
                json(GEOCODE_AACHEN).into_response()
            }
        }
        "/api/v6/plan" => json(PLAN).into_response(),
        "/api/v6/stoptimes" => json(STOPTIMES).into_response(),
        "/api/v6/trip" => {
            let plan: Value = serde_json::from_str(PLAN).expect("valid fixture");
            (
                [(header::CONTENT_TYPE, "application/json")],
                plan["itineraries"][0].to_string(),
            )
                .into_response()
        }
        "/api/v6/broken" => StatusCode::BAD_REQUEST.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn settings(apis: &Apis, public_host: &str) -> Settings {
    let env = HashMap::from([
        ("TRANSITOUS_URL".to_owned(), apis.base.clone()),
        ("PUBLIC_HOST".to_owned(), public_host.to_owned()),
    ]);
    Settings::from_map(&env).expect("valid settings")
}

fn router(settings: &Settings) -> Router {
    app(settings, build_server(settings, http_client()))
}

async fn post(router: &Router, host: &str, body: Value) -> (StatusCode, Value) {
    let request = axum::http::Request::post("/mcp")
        .header(header::HOST, host)
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(header::CONTENT_TYPE, "application/json")
        .header("mcp-protocol-version", "2025-06-18")
        .body(Body::from(body.to_string()))
        .expect("a valid request");
    let response = router.clone().oneshot(request).await.expect("a response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

struct Server {
    apis: Apis,
    router: Router,
    output_schemas: HashMap<String, Value>,
}

impl Server {
    async fn start() -> Self {
        let apis = Apis::start().await;
        let router = router(&settings(&apis, "testserver"));
        let (_, listed) = post(
            &router,
            "testserver",
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
        )
        .await;
        let output_schemas = listed["result"]["tools"]
            .as_array()
            .expect("a tool list")
            .iter()
            .map(|tool| {
                let name = tool["name"].as_str().expect("a name").to_owned();
                (name, tool["outputSchema"].clone())
            })
            .collect();
        Self {
            apis,
            router,
            output_schemas,
        }
    }

    async fn call_raw(&self, name: &str, arguments: Value) -> Value {
        let (status, response) = post(
            &self.router,
            "testserver",
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {"name": name, "arguments": arguments},
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{response}");
        response["result"].clone()
    }

    async fn call(&self, name: &str, arguments: Value) -> Value {
        let result = self.call_raw(name, arguments).await;
        assert_eq!(result["isError"], false, "{result}");
        let content = result["structuredContent"].clone();
        let schema = &self.output_schemas[name];
        let validator = jsonschema::validator_for(schema).expect("a valid output schema");
        if let Err(error) = validator.validate(&content) {
            panic!("{name} answered outside its output schema: {error}\n{content:#}");
        }
        let text: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().expect("a text block"))
                .expect("the text block is the same JSON");
        assert_eq!(text, content);
        content
    }

    async fn error(&self, name: &str, arguments: Value) -> String {
        let result = self.call_raw(name, arguments).await;
        assert_eq!(result["isError"], true, "{result}");
        result["content"][0]["text"]
            .as_str()
            .expect("an error text")
            .to_owned()
    }
}

#[tokio::test]
async fn plan_journey_resolves_names_and_reports_platforms() {
    let server = Server::start().await;
    let result = server
        .call(
            "plan_journey",
            json!({"origin": "Templergraben 55", "destination": "Aachen Hbf"}),
        )
        .await;

    let query = server.apis.last("/api/v6/plan");
    assert_eq!(query["fromPlace"], "50.7779,6.0776");
    assert_eq!(query["toPlace"], "de-DELFI_de:05334:1001");
    assert_eq!(query["arriveBy"], "false");
    assert_eq!(query["detailedLegs"], "false");
    assert_eq!(query["language"], "de");
    assert!(!query.contains_key("time"));
    assert_eq!(result["from"]["address"], "Templergraben 55");
    assert_eq!(result["from"]["area"], "Aachen");
    assert_eq!(result["to"]["name"], "Aachen Hbf");
    assert_eq!(result["later"], "next");
    assert_eq!(result["earlier"], "prev");

    let journey = &result["journeys"][0];
    assert_eq!(journey["transfers"], 1);
    assert_eq!(journey["duration_min"], 45);
    assert_eq!(journey["departure"], "2026-10-28T17:00+01:00");
    assert_eq!(journey["arrival"], "2026-10-28T17:45+01:00");
    let legs = journey["legs"].as_array().expect("legs");
    let (bus, walk, train) = (&legs[0], &legs[1], &legs[2]);
    assert_eq!(bus["line"], "132");
    assert_eq!(bus["operator"], "KVB");
    assert_eq!(bus["realtime"], true);
    assert_eq!(bus["from"]["delay_min"], 2);
    assert_eq!(bus["from"]["time"], "2026-10-28T17:02+01:00");
    assert_eq!(bus["stops_between"], 1);
    assert_eq!(walk["mode"], "WALK");
    assert_eq!(walk["distance_m"], 140);
    assert!(walk.get("line").is_none());
    assert_eq!(train["from"]["platform"], "3");
    assert_eq!(train["from"]["scheduled_platform"], "2");
    assert_eq!(train["from"]["platform_changed"], true);
    assert_eq!(train["from"]["delay_min"], 0);
    assert_eq!(train["alerts"], json!(["Bauarbeiten"]));
    assert!(train["to"].get("platform_changed").is_none());
}

#[tokio::test]
async fn plan_journey_passes_coordinates_and_options() {
    let server = Server::start().await;
    let result = server
        .call(
            "plan_journey",
            json!({
                "origin": "50.77, 6.08",
                "destination": "de-DELFI_de:05334:1001",
                "time": "2026-10-28T08:15",
                "arrive_by": true,
                "max_transfers": 2,
                "transit_modes": ["BUS", "TRAM"],
                "wheelchair": true,
            }),
        )
        .await;

    assert!(!server.apis.paths().contains(&"/api/v1/geocode".to_owned()));
    let query = server.apis.last("/api/v6/plan");
    assert_eq!(query["fromPlace"], "50.77,6.08");
    assert_eq!(query["time"], "2026-10-28T07:15:00Z");
    assert_eq!(query["arriveBy"], "true");
    assert_eq!(query["maxTransfers"], "2");
    assert_eq!(query["transitModes"], "BUS,TRAM");
    assert_eq!(query["pedestrianProfile"], "WHEELCHAIR");
    assert!(!query.contains_key("requireBikeTransport"));
    assert_eq!(
        result["from"],
        json!({"name": "50.77, 6.08", "type": "COORDINATES"})
    );
    assert_eq!(
        result["to"],
        json!({"id": "de-DELFI_de:05334:1001", "type": "STOP"})
    );
}

#[tokio::test]
async fn plan_journey_resolves_via_stops() {
    let server = Server::start().await;
    server
        .call(
            "plan_journey",
            json!({"origin": "Aachen Hbf", "destination": "Aachen Hbf", "via": ["Aachen"], "with_bike": true}),
        )
        .await;

    let query = server.apis.last("/api/v6/plan");
    assert_eq!(query["via"], "de-DELFI_de:05334:1001");
    assert_eq!(query["requireBikeTransport"], "true");
    assert_eq!(server.apis.last("/api/v1/geocode")["type"], "STOP");
}

#[tokio::test]
async fn plan_journey_adds_walking_directions_on_request() {
    let server = Server::start().await;
    server
        .call(
            "plan_journey",
            json!({"origin": "Aachen Hbf", "destination": "Aachen Hbf", "walking_directions": true}),
        )
        .await;

    let query = server.apis.last("/api/v6/plan");
    assert_eq!(query["detailedLegs"], "true");
    assert_eq!(query["detailedTransfers"], "true");
}

#[tokio::test]
async fn plan_journey_reports_unknown_place() {
    let server = Server::start().await;
    let error = server
        .error(
            "plan_journey",
            json!({"origin": "Nowhere at all", "destination": "Aachen Hbf"}),
        )
        .await;
    assert_eq!(
        error,
        "No place found for \"Nowhere at all\". Try a more specific name."
    );
}

#[tokio::test]
async fn plan_journey_rejects_unreadable_time_before_any_request() {
    let server = Server::start().await;
    let error = server
        .error(
            "plan_journey",
            json!({"origin": "Aachen Hbf", "destination": "Aachen Hbf", "time": "soon"}),
        )
        .await;
    assert!(error.contains("HH:MM"), "{error}");
    assert!(server.apis.paths().is_empty());
}

#[tokio::test]
async fn out_of_range_arguments_are_explained() {
    let server = Server::start().await;
    let error = server
        .error("departures", json!({"stop": "Aachen Hbf", "limit": 0}))
        .await;
    assert_eq!(error, "limit must be between 1 and 50, not 0.");
}

#[tokio::test]
async fn departures_show_delay_and_platform_change() {
    let server = Server::start().await;
    let result = server
        .call("departures", json!({"stop": "Aachen Hbf", "limit": 5}))
        .await;

    let query = server.apis.last("/api/v6/stoptimes");
    assert_eq!(query["stopId"], "de-DELFI_de:05334:1001");
    assert_eq!(query["n"], "5");
    assert_eq!(query["arriveBy"], "false");
    assert_eq!(server.apis.last("/api/v1/geocode")["type"], "STOP");
    assert_eq!(result["stop"], "Aachen Hbf");
    assert_eq!(result["later"], "n");
    assert!(result.get("arrivals").is_none());
    let entry = &result["departures"][0];
    assert_eq!(entry["line"], "RE 4");
    assert_eq!(entry["mode"], "REGIONAL_RAIL");
    assert_eq!(entry["direction"], "Düsseldorf Hbf");
    assert_eq!(entry["delay_min"], 5);
    assert_eq!(entry["platform"], "9");
    assert_eq!(entry["scheduled_platform"], "8");
    assert_eq!(entry["platform_changed"], true);
    assert_eq!(entry["realtime"], true);
    assert_eq!(entry["time"], "2026-10-28T17:10+01:00");
    assert_eq!(entry["trip_id"], "trip-re4");
    assert!(entry.get("cancelled").is_none());
}

#[tokio::test]
async fn arrivals_name_where_the_trip_comes_from() {
    let server = Server::start().await;
    let result = server
        .call(
            "departures",
            json!({"stop": "Aachen Hbf", "arrivals": true, "time": "2026-10-28T17:30"}),
        )
        .await;

    let query = server.apis.last("/api/v6/stoptimes");
    assert_eq!(query["arriveBy"], "true");
    assert_eq!(query["time"], "2026-10-28T16:30:00Z");
    assert!(result.get("departures").is_none());
    let entry = &result["arrivals"][0];
    assert_eq!(entry["from"], "Aachen Hbf");
    assert!(entry.get("direction").is_none());
}

#[tokio::test]
async fn departures_near_coordinates_search_around_them() {
    let server = Server::start().await;
    server
        .call("departures", json!({"stop": "50.7678, 6.0912"}))
        .await;

    let query = server.apis.last("/api/v6/stoptimes");
    assert_eq!(query["center"], "50.7678,6.0912");
    assert_eq!(query["radius"], "150");
    assert!(!query.contains_key("stopId"));
}

#[tokio::test]
async fn trip_details_lists_all_stops() {
    let server = Server::start().await;
    let result = server
        .call("trip_details", json!({"trip_id": "trip-bus"}))
        .await;

    let query = server.apis.last("/api/v6/trip");
    assert_eq!(query["tripId"], "trip-bus");
    assert_eq!(query["detailedLegs"], "false");
    assert_eq!(result["line"], "132");
    assert_eq!(result["direction"], "Köln Hbf");
    let stops = result["stops"].as_array().expect("stops");
    let names: Vec<_> = stops.iter().map(|stop| &stop["name"]).collect();
    assert_eq!(
        names,
        [
            "Köln Neumarkt",
            "Köln Appellhofplatz",
            "Köln Hbf",
            "Köln Hbf",
            "Aachen Hbf"
        ]
    );
    assert_eq!(stops[0]["departure"], "2026-10-28T17:02+01:00");
    assert_eq!(stops[0]["departure_delay_min"], 2);
    assert_eq!(stops[3]["platform"], "3");
    assert_eq!(stops[3]["platform_changed"], true);
}

#[tokio::test]
async fn find_place_passes_filters() {
    let server = Server::start().await;
    let result = server
        .call(
            "find_place",
            json!({"query": "Aachen", "kind": "Stop", "near": "50.7,6.1", "limit": 3}),
        )
        .await;

    let query = server.apis.last("/api/v1/geocode");
    assert_eq!(query["type"], "STOP");
    assert_eq!(query["place"], "50.7,6.1");
    assert_eq!(query["numResults"], "3");
    assert_eq!(
        result["results"][0],
        json!({
            "id": "de-DELFI_de:05334:1001",
            "name": "Aachen Hbf",
            "type": "STOP",
            "coordinates": "50.7678,6.0912",
            "area": "Aachen",
            "modes": ["REGIONAL_RAIL", "BUS"],
        })
    );
}

#[tokio::test]
async fn tools_are_listed_read_only_with_instructions() {
    let apis = Apis::start().await;
    let router = router(&settings(&apis, "testserver"));
    let (status, initialized) = post(
        &router,
        "testserver",
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "0"},
            },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let instructions = initialized["result"]["instructions"]
        .as_str()
        .expect("instructions");
    assert!(instructions.starts_with("You answer questions about travelling by public transport"));

    let (_, listed) = post(
        &router,
        "testserver",
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    )
    .await;
    let tools = listed["result"]["tools"].as_array().expect("tools");
    let mut names: Vec<_> = tools
        .iter()
        .map(|tool| tool["name"].as_str().expect("a name"))
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["departures", "find_place", "plan_journey", "trip_details"]
    );
    for tool in tools {
        assert_eq!(tool["annotations"]["readOnlyHint"], true, "{tool}");
        assert_eq!(tool["annotations"]["openWorldHint"], true, "{tool}");
        assert!(tool["outputSchema"].is_object(), "{tool}");
    }
}

#[tokio::test]
async fn health_answers_ok() {
    let apis = Apis::start().await;
    let request = axum::http::Request::get("/health")
        .body(Body::empty())
        .expect("a valid request");
    let response = router(&settings(&apis, ""))
        .oneshot(request)
        .await
        .expect("a response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    assert_eq!(&body[..], b"ok");
}

#[tokio::test]
async fn unknown_host_and_origin_are_rejected() {
    let apis = Apis::start().await;
    let router = router(&settings(&apis, "transit.example.org"));
    let list = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"});

    let (status, _) = post(&router, "evil.example", list.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = post(&router, "transit.example.org", list).await;
    assert_eq!(status, StatusCode::OK);
}
