# Transit MCP Server

A Model Context Protocol server that provides public transport journeys,
departures and platforms for Germany and many other countries. This server enables
LLMs to plan trips and check live delays.

## Available Tools

- `plan_journey` - Plans journeys from A to B with every leg, platform, delay and alert.
- `departures` - Lists the next departures or arrivals at a stop.
- `trip_details` - Lists every stop of one train or bus run.
- `find_place` - Searches stops, addresses and places.
- `db_station_board` - Shows the live board of a German railway station with
  platform changes and delay reasons.

All tools are read-only. Tickets and prices are not included.

## Usage

Add the server as a custom connector in an MCP client:

`https://transit.mcp.zahraabouzeid.me/mcp`

No sign-in is needed.

## Data

Journey data comes from [Transitous](https://transitous.org) and its
[sources](https://transitous.org/sources/), including OpenStreetMap. Station boards
come from the Deutsche Bahn Timetables API under CC BY 4.0.

## License

This project is licensed under the [AGPL-3.0](LICENSE).
