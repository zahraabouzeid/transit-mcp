FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked \
    && cp target/release/transit-mcp /usr/local/bin/transit-mcp

FROM gcr.io/distroless/cc-debian12:nonroot
LABEL org.opencontainers.image.source="https://github.com/zahraabouzeid/transit-mcp"
COPY --from=build /usr/local/bin/transit-mcp /usr/local/bin/transit-mcp
EXPOSE 8000
HEALTHCHECK --interval=30s --timeout=5s CMD ["/usr/local/bin/transit-mcp", "health"]
ENTRYPOINT ["/usr/local/bin/transit-mcp"]
