//! Stdio mode: the real binary, the token from `VIKUNJA_TOKEN`, the same tools and client.

mod common;

use std::process::Stdio;

use common::Fake;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

async fn exchange(fake: &Fake, token: Option<&str>) -> (Option<Value>, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_emperor-vikunja"));
    cmd.args(["serve", "--stdio", "--base-url", &fake.base])
        .env_remove("VIKUNJA_TOKEN")
        .env_remove("VIKUNJA_URL")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(t) = token {
        cmd.env("VIKUNJA_TOKEN", t);
    }
    let mut child = cmd.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();

    let messages = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-06-18","capabilities":{},
            "clientInfo":{"name":"test","version":"0"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_projects","arguments":{}}}),
    ];
    let mut reply = None;
    for m in messages {
        let _ = stdin.write_all(format!("{m}\n").as_bytes()).await;
    }
    let _ = stdin.flush().await;
    let read = async {
        while let Ok(Some(line)) = lines.next_line().await {
            let v: Value = serde_json::from_str(&line).expect("stdout carries only JSON-RPC");
            if v["id"] == 2 {
                return Some(v);
            }
        }
        None
    };
    if let Ok(v) = tokio::time::timeout(std::time::Duration::from_secs(10), read).await {
        reply = v;
    }
    drop(stdin);
    let _ = child.kill().await;
    let mut stderr = String::new();
    if let Some(mut e) = child.stderr.take() {
        use tokio::io::AsyncReadExt;
        let _ = e.read_to_string(&mut stderr).await;
    }
    (reply, stderr)
}

#[tokio::test]
async fn stdio_uses_the_env_token() {
    let fake = Fake::seeded().await;
    let (reply, stderr) = exchange(&fake, Some(&fake.token)).await;
    let reply = reply.unwrap_or_else(|| panic!("no reply; stderr: {stderr}"));
    let result = &reply["result"];
    assert_eq!(result["isError"], false, "{reply}");
    assert_eq!(result["structuredContent"]["count"], 3, "{reply}");
    let auth = fake.requests_to("GET", "/api/v1/projects")[0].auth.clone();
    assert_eq!(auth, Some(format!("Bearer {}", fake.token)));
}

#[tokio::test]
async fn stdio_without_token_refuses_to_start() {
    let fake = Fake::seeded().await;
    let (reply, stderr) = exchange(&fake, None).await;
    assert!(reply.is_none());
    assert!(stderr.contains("VIKUNJA_TOKEN is not set"), "{stderr}");
}
