//! `emperor-vikunja doctor`: what the server needs, checked before it starts.

use std::net::SocketAddr;

use emperor_mcp::doctor::{Check, Report, env_set, port_free, tool_budget};

use crate::client::{Token, Vikunja};
use crate::server::{
    READ_ONLY_DEPRECATED, TOKEN_ENV, UPSTREAM_URL_HEADER, VikunjaServer, WRITE_TOOLS,
};

/// What `doctor` checks: the same flags `serve` takes.
#[derive(Debug, Clone, Copy)]
pub struct Setup<'a> {
    /// `--base-url` / `VIKUNJA_URL`, if given.
    pub base_url: Option<&'a str>,
    /// Listen address for HTTP mode.
    pub bind: SocketAddr,
    /// stdio mode: skips the listen-address check, makes the URL and the token mandatory.
    pub stdio: bool,
    /// `--writable` / `VIKUNJA_WRITABLE`: the write tools are served too.
    pub writable: bool,
    /// `--read-only` / `VIKUNJA_READ_ONLY` was given: deprecated, read-only is the default.
    pub read_only_given: bool,
}

/// Run every check.
pub async fn run(setup: &Setup<'_>) -> Report {
    let mut report = Report::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    let stdio = setup.stdio;

    let client = match setup.base_url {
        Some(url) => match Vikunja::new(url) {
            Ok(c) => {
                report.push(Check::pass("base url", c.base_url().as_str()));
                Some(c)
            }
            Err(e) => {
                report.push(Check::fail("base url", e.to_string()));
                None
            }
        },
        None if stdio => {
            report.push(Check::fail(
                "base url",
                "not set; stdio mode needs --base-url or VIKUNJA_URL",
            ));
            None
        }
        None => {
            report.push(Check::warn(
                "base url",
                format!(
                    "not set: every request must name its Vikunja in the {UPSTREAM_URL_HEADER} header"
                ),
            ));
            None
        }
    };

    if let Some(c) = &client {
        report.push(match c.info().await {
            Ok(info) => Check::pass("vikunja api", format!("Vikunja {}", info.version)),
            Err(e) => Check::fail("vikunja api", format!("GET /api/v1/info: {e}")),
        });
    }

    let token = std::env::var(TOKEN_ENV).ok().and_then(Token::new);
    match (&token, stdio) {
        (Some(_), _) => {
            report.push(env_set(TOKEN_ENV, true));
        }
        (None, true) => {
            report.push(Check::fail(
                TOKEN_ENV,
                "not set; stdio mode needs it (Vikunja: Settings > API Tokens)",
            ));
        }
        (None, false) => {
            report.push(Check::warn(
                TOKEN_ENV,
                "not set: fine for HTTP mode, the token comes per request in the Authorization header",
            ));
        }
    }

    if let (Some(c), Some(t)) = (&client, &token) {
        report.push(match c.session(t).user().await {
            Ok(user) => Check::pass(
                "vikunja login",
                format!("token works for user {}", user.username),
            ),
            Err(e) => Check::fail("vikunja login", format!("GET /api/v1/user: {e}")),
        });
    }

    let tools = VikunjaServer::tools(setup.writable);
    let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
    report.push(Check::pass(
        "mode",
        if setup.writable {
            format!("writable: {}", names.join(", "))
        } else {
            format!(
                "read-only: {} (pass --writable or VIKUNJA_WRITABLE=true for {})",
                names.join(", "),
                WRITE_TOOLS.join(", ")
            )
        },
    ));
    if setup.read_only_given {
        report.push(Check::warn("--read-only", READ_ONLY_DEPRECATED));
    }
    if !stdio {
        report.push(port_free(setup.bind));
    }
    report.push(tool_budget(tools.len()));
    report
}
