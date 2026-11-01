//! `emperor-vikunja doctor`: what the server needs, checked before it starts.

use std::net::SocketAddr;

use emperor_mcp::doctor::{Check, Report, env_set, port_free, tool_budget};

use crate::client::{Token, Vikunja};
use crate::server::{TOKEN_ENV, VikunjaServer};

/// Run every check. `stdio` skips the listen-address check and makes the token mandatory.
pub async fn run(base_url: &str, bind: SocketAddr, stdio: bool) -> Report {
    let mut report = Report::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));

    let client = match Vikunja::new(base_url) {
        Ok(c) => {
            report.push(Check::pass("base url", c.base_url().as_str()));
            Some(c)
        }
        Err(e) => {
            report.push(Check::fail("base url", e.to_string()));
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

    if !stdio {
        report.push(port_free(bind));
    }
    report.push(tool_budget(VikunjaServer::tools().len()));
    report
}
