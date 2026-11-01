//! MCP-over-HTTP helpers: start the server through emperor-mcp `serve`, send JSON-RPC.

use emperor_mcp::{ServeOptions, serve};
use emperor_vikunja::{Vikunja, VikunjaServer};
use serde_json::{Value, json};

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Start the MCP server in front of the Vikunja at `vikunja_url`; returns its base URL.
pub async fn start(vikunja_url: &str) -> String {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], free_port()));
    let client = Vikunja::new(vikunja_url).unwrap();
    tokio::spawn(serve(ServeOptions::new(addr), move || {
        VikunjaServer::http(client.clone())
    }));
    let base = format!("http://{addr}");
    for _ in 0..50 {
        if reqwest::get(format!("{base}/health")).await.is_ok() {
            return base;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("MCP server did not start");
}

pub async fn rpc(base: &str, body: Value, auth: Option<&str>) -> Value {
    let mut req = reqwest::Client::new()
        .post(format!("{base}/mcp"))
        .header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "2025-06-18")
        .json(&body);
    if let Some(a) = auth {
        req = req.header("Authorization", a);
    }
    let resp = req.send().await.unwrap();
    assert!(resp.status().is_success(), "status {}", resp.status());
    let text = resp.text().await.unwrap();
    let json = text
        .lines()
        .find_map(|l| l.strip_prefix("data:"))
        .unwrap_or(&text)
        .trim()
        .to_string();
    serde_json::from_str(&json).unwrap_or_else(|e| panic!("bad body {text}: {e}"))
}

pub struct Call {
    pub text: String,
    pub structured: Value,
    pub is_error: bool,
}

pub async fn call(base: &str, auth: Option<&str>, tool: &str, args: Value) -> Call {
    let v = rpc(
        base,
        json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":tool,"arguments":args}}),
        auth,
    )
    .await;
    let result = &v["result"];
    assert!(result.is_object(), "no result: {v}");
    Call {
        text: result["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        structured: result["structuredContent"].clone(),
        is_error: result["isError"].as_bool().unwrap_or(false),
    }
}
