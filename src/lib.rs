//! MCP server for [Vikunja](https://vikunja.io), built on
//! [emperor-mcp](https://github.com/yarenty/emperor-mcp).
//!
//! Six tools over a thin REST client: `list_tasks`, `get_task` and `list_projects` always,
//! `create_task`, `update_task` and `comment` only when writing is switched on
//! ([`VikunjaServer::writable`]; the server is read-only by default). The token is forwarded
//! per request over HTTP or read from `VIKUNJA_TOKEN` in stdio mode, and never stored.
//!
//! - [`client`]: the Vikunja REST client (only the endpoints the tools use)
//! - [`server`]: the rmcp `#[tool_router]` server
//! - [`text`]: Markdown to HTML (in) and HTML to text (out)
//! - [`dates`]: date arguments and Vikunja's zero time
//! - [`doctor`]: the `doctor` subcommand

#![warn(missing_docs)]

pub mod client;
pub mod dates;
pub mod doctor;
pub mod server;
pub mod text;

pub use client::{Token, Upstreams, Vikunja};
pub use server::VikunjaServer;

/// Log to stderr with an `RUST_LOG` filter (default `info`, rmcp at `warn`). Stderr keeps
/// stdout clean for the stdio protocol stream (Emperor Profile E9).
pub fn init_tracing() {
    use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};
    let _ = tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,rmcp=warn".into()))
        .with(fmt::layer().with_writer(std::io::stderr))
        .try_init();
}
