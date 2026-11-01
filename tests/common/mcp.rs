//! MCP-over-HTTP helpers: start the server through emperor-mcp `serve`, send JSON-RPC.

use emperor_mcp::{ServeOptions, serve};
use emperor_vikunja::server::forward_config;
use emperor_vikunja::{Upstreams, VikunjaServer};
use reqwest::StatusCode;
use reqwest::header::HeaderMap;
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
    start_with(Some(vikunja_url), false).await
}

/// Start the MCP server with an optional default Vikunja and optionally read-only.
pub async fn start_with(vikunja_url: Option<&str>, read_only: bool) -> String {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], free_port()));
    let upstreams = Upstreams::new(vikunja_url).unwrap();
    tokio::spawn(serve(
        ServeOptions::new(addr).with_forward(forward_config()),
        move || VikunjaServer::http(upstreams.clone()).read_only(read_only),
    ));
    let base = format!("http://{addr}");
    for _ in 0..50 {
        if reqwest::get(format!("{base}/health")).await.is_ok() {
            return base;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("MCP server did not start");
}

/// One JSON-RPC exchange: HTTP status, response headers, decoded body.
pub async fn rpc_raw(
    base: &str,
    body: Value,
    headers: &[(&str, &str)],
) -> (StatusCode, HeaderMap, Value) {
    let mut req = reqwest::Client::new()
        .post(format!("{base}/mcp"))
        .header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "2025-06-18")
        .json(&body);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let resp = req.send().await.unwrap();
    let status = resp.status();
    let resp_headers = resp.headers().clone();
    let text = resp.text().await.unwrap();
    let json = text
        .lines()
        .find_map(|l| l.strip_prefix("data:"))
        .unwrap_or(&text)
        .trim()
        .to_string();
    let value = serde_json::from_str(&json).unwrap_or_else(|e| panic!("bad body {text}: {e}"));
    (status, resp_headers, value)
}

pub async fn rpc(base: &str, body: Value, auth: Option<&str>) -> Value {
    let headers: Vec<(&str, &str)> = auth.map(|a| ("Authorization", a)).into_iter().collect();
    let (status, _, value) = rpc_raw(base, body, &headers).await;
    assert!(status.is_success(), "status {status}: {value}");
    value
}

pub fn tool_call(tool: &str, args: Value) -> Value {
    json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":tool,"arguments":args}})
}

pub struct Call {
    pub text: String,
    pub structured: Value,
    pub is_error: bool,
}

fn to_call(v: &Value) -> Call {
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

pub async fn call(base: &str, auth: Option<&str>, tool: &str, args: Value) -> Call {
    to_call(&rpc(base, tool_call(tool, args), auth).await)
}

/// A tool call with any request headers (e.g. `X-Upstream-Url`); the HTTP answer must be 2xx.
pub async fn call_with(base: &str, headers: &[(&str, &str)], tool: &str, args: Value) -> Call {
    let (status, _, v) = rpc_raw(base, tool_call(tool, args), headers).await;
    assert!(status.is_success(), "status {status}: {v}");
    to_call(&v)
}
