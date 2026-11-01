//! `emperor-vikunja`: serve the Vikunja MCP tools, or check the setup with `doctor`.

use std::net::SocketAddr;

use anyhow::Context;
use clap::builder::FalseyValueParser;
use clap::{ArgAction, Args, Parser, Subcommand};
use emperor_mcp::cli::ServeArgs;
use emperor_mcp::serve;
use emperor_vikunja::client::Upstreams;
use emperor_vikunja::server::{TOKEN_ENV, forward_config};
use emperor_vikunja::{Token, Vikunja, VikunjaServer, doctor, init_tracing};
use rmcp::ServiceExt;

/// Environment variable that switches on `--read-only` (for launchers that set only env vars).
const READ_ONLY_ENV: &str = "VIKUNJA_READ_ONLY";

/// Where the HTTP server listens when `--bind` is not given.
const DEFAULT_BIND: SocketAddr =
    SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 8034);

#[derive(Parser)]
#[command(
    name = "emperor-vikunja",
    version,
    about = "MCP server for a Vikunja instance"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serve the tools over Streamable HTTP at /mcp (or over stdio with --stdio).
    Serve(Common),
    /// Check the URL, the Vikunja API, the token, the mode and the listen address; exit 1 on
    /// failure.
    Doctor(Common),
}

#[derive(Args)]
struct Common {
    /// Vikunja address, e.g. https://vikunja.example.com. Required with --stdio; over HTTP a
    /// request can name its instance in the X-Upstream-Url header instead.
    #[arg(long, env = "VIKUNJA_URL")]
    base_url: Option<String>,
    /// Serve only the read tools (list_tasks, get_task, list_projects). From the environment:
    /// `true`, `1` or `yes` turns it on; unset, empty, `false`, `0` or `no` leaves it off.
    #[arg(
        long,
        env = READ_ONLY_ENV,
        action = ArgAction::SetTrue,
        value_parser = FalseyValueParser::new()
    )]
    read_only: bool,
    #[command(flatten)]
    serve: ServeArgs,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Doctor(c) => {
            let report = doctor::run(&doctor::Setup {
                base_url: c.base_url.as_deref(),
                bind: c.serve.bind_or(DEFAULT_BIND),
                stdio: c.serve.stdio,
                read_only: c.read_only,
            })
            .await;
            report.print();
            std::process::exit(report.exit_code());
        }
        Command::Serve(c) => {
            init_tracing();
            let env_token = std::env::var(TOKEN_ENV).ok().and_then(Token::new);
            let mode = if c.read_only {
                "read-only"
            } else {
                "read-write"
            };
            if c.serve.stdio {
                let base = c
                    .base_url
                    .as_deref()
                    .context("--base-url (or VIKUNJA_URL) is required with --stdio")?;
                let client = Vikunja::new(base)?;
                let token = env_token
                    .with_context(|| format!("{TOKEN_ENV} is not set; stdio mode needs it"))?;
                tracing::info!(base = %client.base_url(), mode, "serving Vikunja tools over stdio");
                let running = VikunjaServer::local(client, token)
                    .read_only(c.read_only)
                    .serve(rmcp::transport::stdio())
                    .await?;
                running.waiting().await?;
            } else {
                if env_token.is_some() {
                    tracing::warn!(
                        "{TOKEN_ENV} is ignored in HTTP mode: each request brings its own Authorization header"
                    );
                }
                let upstreams = Upstreams::new(c.base_url.as_deref())?;
                match upstreams.default_instance() {
                    Some(v) => {
                        tracing::info!(base = %v.base_url(), mode, "serving Vikunja tools over HTTP")
                    }
                    None => tracing::warn!(
                        mode,
                        "no --base-url: every request must name its Vikunja in X-Upstream-Url"
                    ),
                }
                let read_only = c.read_only;
                serve(
                    c.serve
                        .serve_options(DEFAULT_BIND)
                        .with_forward(forward_config()),
                    move || VikunjaServer::http(upstreams.clone()).read_only(read_only),
                )
                .await?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_only(args: &[&str]) -> bool {
        let cli = Cli::try_parse_from(
            ["emperor-vikunja", "serve", "--stdio"]
                .iter()
                .chain(args)
                .copied(),
        )
        .unwrap();
        match cli.command {
            Command::Serve(c) | Command::Doctor(c) => c.read_only,
        }
    }

    // One test: it changes the process environment, so nothing may run beside it.
    #[test]
    fn read_only_from_flag_and_environment() {
        let set = |v: Option<&str>| {
            // SAFETY: the only test in this binary that touches the environment, and
            // no other thread reads it while this test runs.
            unsafe {
                match v {
                    Some(v) => std::env::set_var(READ_ONLY_ENV, v),
                    None => std::env::remove_var(READ_ONLY_ENV),
                }
            }
        };
        set(None);
        assert!(!read_only(&[]));
        assert!(read_only(&["--read-only"]));
        for off in ["", "false", "FALSE", "0", "no", "off"] {
            set(Some(off));
            assert!(!read_only(&[]), "{off:?} must leave read-only off");
        }
        for on in ["true", "True", "1", "yes"] {
            set(Some(on));
            assert!(read_only(&[]), "{on:?} must turn read-only on");
        }
        set(Some("false"));
        assert!(
            read_only(&["--read-only"]),
            "the flag wins over a false env"
        );
        set(None);
    }
}
