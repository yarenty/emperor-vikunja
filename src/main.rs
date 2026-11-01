//! `emperor-vikunja`: serve the Vikunja MCP tools, or check the setup with `doctor`.

use std::net::SocketAddr;

use anyhow::Context;
use clap::{ArgAction, Args, Parser, Subcommand};
use emperor_mcp::cli::{ServeArgs, parse_switch};
use emperor_mcp::serve;
use emperor_vikunja::client::Upstreams;
use emperor_vikunja::server::{
    READ_ONLY_DEPRECATED, READ_ONLY_ENV, TOKEN_ENV, WRITABLE_ENV, forward_config, write_mode,
};
use emperor_vikunja::{Token, Vikunja, VikunjaServer, doctor, init_tracing};
use rmcp::ServiceExt;

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
    /// Also serve the write tools (create_task, update_task, comment). Without it the server is
    /// read-only. From the environment: `true`, `1`, `yes` or `on` turns it on; unset, empty,
    /// `false`, `0`, `no` or `off` leaves it off; anything else is refused.
    #[arg(
        long,
        env = WRITABLE_ENV,
        action = ArgAction::SetTrue,
        value_parser = parse_switch
    )]
    writable: bool,
    /// Deprecated, no effect: read-only is the default. Refused together with --writable.
    #[arg(
        long,
        env = READ_ONLY_ENV,
        action = ArgAction::SetTrue,
        value_parser = parse_switch
    )]
    read_only: bool,
    #[command(flatten)]
    serve: ServeArgs,
}

impl Common {
    /// True when writing is switched on; both switches on is refused.
    fn writable(&self) -> anyhow::Result<bool> {
        write_mode(self.writable, self.read_only).map_err(anyhow::Error::msg)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Doctor(c) => {
            let writable = c.writable()?;
            let report = doctor::run(&doctor::Setup {
                base_url: c.base_url.as_deref(),
                bind: c.serve.bind_or(DEFAULT_BIND),
                stdio: c.serve.stdio,
                writable,
                read_only_given: c.read_only,
            })
            .await;
            report.print();
            std::process::exit(report.exit_code());
        }
        Command::Serve(c) => {
            init_tracing();
            let writable = c.writable()?;
            if c.read_only {
                tracing::warn!("{READ_ONLY_DEPRECATED}");
            }
            let env_token = std::env::var(TOKEN_ENV).ok().and_then(Token::new);
            let mode = if writable { "writable" } else { "read-only" };
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
                    .writable(writable)
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
                serve(
                    c.serve
                        .serve_options(DEFAULT_BIND)
                        .with_forward(forward_config()),
                    move || VikunjaServer::http(upstreams.clone()).writable(writable),
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

    /// `(writable, read_only)` as parsed, or the parse error.
    fn switches(args: &[&str]) -> Result<(bool, bool), clap::Error> {
        let cli = Cli::try_parse_from(
            ["emperor-vikunja", "serve", "--stdio"]
                .iter()
                .chain(args)
                .copied(),
        )?;
        match cli.command {
            Command::Serve(c) | Command::Doctor(c) => Ok((c.writable, c.read_only)),
        }
    }

    fn mode(args: &[&str]) -> Result<bool, &'static str> {
        let (w, r) = switches(args).unwrap();
        write_mode(w, r)
    }

    // One test: it changes the process environment, so nothing may run beside it.
    #[test]
    fn switches_from_flags_and_environment() {
        let set = |name: &str, v: Option<&str>| {
            // SAFETY: the only test in this binary that touches the environment, and
            // no other thread reads it while this test runs.
            unsafe {
                match v {
                    Some(v) => std::env::set_var(name, v),
                    None => std::env::remove_var(name),
                }
            }
        };
        set(WRITABLE_ENV, None);
        set(READ_ONLY_ENV, None);

        // Default: read-only.
        assert_eq!(mode(&[]), Ok(false));
        assert_eq!(mode(&["--writable"]), Ok(true));

        for off in ["", "false", "FALSE", "0", "no", "off"] {
            set(WRITABLE_ENV, Some(off));
            assert_eq!(mode(&[]), Ok(false), "{off:?} must leave writing off");
        }
        for on in ["true", "True", "1", "yes", "on"] {
            set(WRITABLE_ENV, Some(on));
            assert_eq!(mode(&[]), Ok(true), "{on:?} must switch writing on");
        }
        set(WRITABLE_ENV, Some("maybe"));
        assert!(switches(&[]).is_err(), "an unknown value is refused");
        set(WRITABLE_ENV, Some("false"));
        assert_eq!(
            mode(&["--writable"]),
            Ok(true),
            "the flag wins over a false env"
        );
        set(WRITABLE_ENV, None);

        // --read-only is accepted and changes nothing.
        assert_eq!(switches(&["--read-only"]).unwrap(), (false, true));
        assert_eq!(mode(&["--read-only"]), Ok(false));
        set(READ_ONLY_ENV, Some("true"));
        assert_eq!(mode(&[]), Ok(false));

        // Both on: refused.
        assert!(mode(&["--writable"]).is_err());
        set(READ_ONLY_ENV, None);
        assert!(mode(&["--writable", "--read-only"]).is_err());
        set(WRITABLE_ENV, Some("1"));
        set(READ_ONLY_ENV, Some("yes"));
        assert!(mode(&[]).is_err());

        // A false read-only beside writable is fine.
        set(READ_ONLY_ENV, Some("false"));
        assert_eq!(mode(&[]), Ok(true));

        set(WRITABLE_ENV, None);
        set(READ_ONLY_ENV, None);
    }
}
