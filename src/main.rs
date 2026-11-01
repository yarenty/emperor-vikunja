//! `emperor-vikunja`: serve the Vikunja MCP tools, or check the setup with `doctor`.

use std::net::SocketAddr;

use anyhow::Context;
use clap::{Args, Parser, Subcommand};
use emperor_mcp::{ServeOptions, serve};
use emperor_vikunja::server::TOKEN_ENV;
use emperor_vikunja::{Token, Vikunja, VikunjaServer, doctor, init_tracing};
use rmcp::ServiceExt;

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
    /// Check the URL, the Vikunja API, the token and the listen address; exit 1 on failure.
    Doctor(Common),
}

#[derive(Args)]
struct Common {
    /// Vikunja address, e.g. https://vikunja.example.com
    #[arg(long, env = "VIKUNJA_URL")]
    base_url: String,
    /// Address to listen on in HTTP mode.
    #[arg(long, default_value = "127.0.0.1:8034")]
    bind: SocketAddr,
    /// Speak MCP over stdin/stdout (local use); the token comes from VIKUNJA_TOKEN.
    #[arg(long)]
    stdio: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Doctor(c) => {
            let report = doctor::run(&c.base_url, c.bind, c.stdio).await;
            report.print();
            std::process::exit(report.exit_code());
        }
        Command::Serve(c) => {
            init_tracing();
            let client = Vikunja::new(&c.base_url)?;
            let env_token = std::env::var(TOKEN_ENV).ok().and_then(Token::new);
            if c.stdio {
                let token = env_token
                    .with_context(|| format!("{TOKEN_ENV} is not set; stdio mode needs it"))?;
                tracing::info!(base = %client.base_url(), "serving Vikunja tools over stdio");
                let running = VikunjaServer::local(client, token)
                    .serve(rmcp::transport::stdio())
                    .await?;
                running.waiting().await?;
            } else {
                if env_token.is_some() {
                    tracing::warn!(
                        "{TOKEN_ENV} is ignored in HTTP mode: each request brings its own Authorization header"
                    );
                }
                tracing::info!(base = %client.base_url(), "serving Vikunja tools over HTTP");
                serve(ServeOptions::new(c.bind), move || {
                    VikunjaServer::http(client.clone())
                })
                .await?;
            }
            Ok(())
        }
    }
}
