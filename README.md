# transit-mcp

An MCP server that lets an AI assistant plan public transport journeys the way a journey
planner app does. Ask it how to get from A to B, where to change, which platform a train
leaves from, whether it is late, or what leaves from a stop next.

It covers Germany and many other countries through [Transitous](https://transitous.org), a
free routing service built on open timetables with live updates. For German railway stations,
the [DB Timetables API](https://developers.deutschebahn.com) adds live platform changes and
the reasons for delays.

## Tools

- `plan_journey`: journeys from A to B with every leg, platform, delay and alert. Start and
  destination can be stop names, addresses, places or coordinates.
- `departures`: the next departures or arrivals at a stop.
- `trip_details`: every stop of one train or bus run.
- `find_place`: search stops, addresses and places when a name is ambiguous.
- `db_station_board`: the live DB board of a German railway station with platform changes,
  delay reasons and train notices.

Every tool is read-only and returns structured content that matches its output schema.
Tickets and prices are not included.

## Connect an assistant

Add the server as a custom connector with the address `https://<your domain>/mcp`. The
server has no login, so anyone who knows the address can use it.

## Deploy

Every push to `main` runs the tests, builds the image `ghcr.io/zahraabouzeid/transit-mcp` and
deploys it over SSH with Docker Compose. The container joins the Docker network `web`, where
the shared Caddy from the `infra` repository serves it over HTTPS.

The deploy needs these repository settings:

- Variables: `SSH_TARGET` (for example `user@host`), `DEPLOY_FOLDER` and `DOMAIN`.
- Secrets: `SSH_KEY`, `SSH_KNOWN_HOSTS`, `DB_CLIENT_ID` and `DB_API_KEY`.

Without `SSH_TARGET` the deploy step is skipped. Without the DB values the server still runs,
and `db_station_board` explains that it is not configured.

To get the DB values, register at the DB API Marketplace, create an application and
subscribe it to the Timetables API. The portal shows a Client ID and an API key.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The tests run against recorded answers of both services, so they need no network access.

Run the server locally with `cargo run`. It listens on port 8000 and serves the MCP endpoint
at `/mcp` and a health check at `/health`. `transit-mcp health` checks a running server, which
the container uses for its health check. Settings come from environment variables, see
`.env.example`.

## Data and attribution

Journey data comes from Transitous and its [sources](https://transitous.org/sources/),
including OpenStreetMap. Transitous may be used free of charge by open source,
non-commercial projects and asks clients to identify themselves, which this server does
with its User-Agent. Station board data comes from the Deutsche Bahn Timetables API under
CC BY 4.0.

## License

[AGPL-3.0](LICENSE)
