# emperor-vikunja over stdio, for MCP clients that start servers as containers.
#
#   docker build -t emperor-vikunja .
#   docker run -i --rm -e VIKUNJA_URL=https://vikunja.example.com -e VIKUNJA_TOKEN=... emperor-vikunja
#
# Read-only by default (list_tasks, get_task, list_projects). Add the write tools with
# -e VIKUNJA_WRITABLE=true (or --writable after the image name).

FROM rust:1.88-slim-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked --bin emperor-vikunja

FROM debian:bookworm-slim
LABEL org.opencontainers.image.title="emperor-vikunja" \
      org.opencontainers.image.description="MCP server for Vikunja: list, read, create and update tasks, comment, list projects" \
      org.opencontainers.image.source="https://github.com/yarenty/emperor-vikunja" \
      org.opencontainers.image.url="https://github.com/yarenty/emperor-vikunja" \
      org.opencontainers.image.licenses="MIT OR Apache-2.0"
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --no-create-home --shell /usr/sbin/nologin emperor
COPY --from=build /src/target/release/emperor-vikunja /usr/local/bin/emperor-vikunja
USER 10001
# The Vikunja address comes from VIKUNJA_URL, the token from VIKUNJA_TOKEN, the write switch
# from VIKUNJA_WRITABLE.
ENTRYPOINT ["emperor-vikunja", "serve", "--stdio"]
