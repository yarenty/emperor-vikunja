//! Stdio mode: the real binary, the token from `VIKUNJA_TOKEN`, the same tools and client.

mod common;

use std::process::Stdio;

use common::Fake;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

async fn exchange(fake: &Fake, token: Option<&str>) -> (Option<Value>, String) {
    exchange_with(
        fake,
        token,
        &[],
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_projects","arguments":{}}}),
    )
    .await
}

async fn exchange_with(
    fake: &Fake,
    token: Option<&str>,
    extra_args: &[&str],
    request: Value,
) -> (Option<Value>, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_emperor-vikunja"));
    cmd.args(["serve", "--stdio", "--base-url", &fake.base])
        .args(extra_args)
        .env_remove("VIKUNJA_TOKEN")
        .env_remove("VIKUNJA_URL")
        .env_remove("VIKUNJA_READ_ONLY")
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
        request,
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

#[tokio::test]
async fn stdio_rejected_token_is_a_jsonrpc_error() {
    let fake = Fake::seeded().await;
    let (reply, stderr) = exchange(&fake, Some("wrong")).await;
    let reply = reply.unwrap_or_else(|| panic!("no reply; stderr: {stderr}"));
    assert_eq!(reply["error"]["code"], -32040, "{reply}");
    assert_eq!(reply["error"]["data"]["httpStatus"], 401, "{reply}");
}

#[tokio::test]
async fn stdio_read_only_lists_three_tools() {
    let fake = Fake::seeded().await;
    let (reply, stderr) = exchange_with(
        &fake,
        Some(&fake.token),
        &["--read-only"],
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
    )
    .await;
    let reply = reply.unwrap_or_else(|| panic!("no reply; stderr: {stderr}"));
    emperor_mcp::testkit::assert_tools_json(&reply);
    let mut names: Vec<&str> = reply["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    names.sort();
    assert_eq!(names, vec!["get_task", "list_projects", "list_tasks"]);
}

fn doctor(args: &[&str], token: Option<&str>) -> (bool, String) {
    doctor_env(args, token, None)
}

fn doctor_env(args: &[&str], token: Option<&str>, read_only_env: Option<&str>) -> (bool, String) {
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_emperor-vikunja"));
    cmd.arg("doctor")
        .args(args)
        .env_remove("VIKUNJA_TOKEN")
        .env_remove("VIKUNJA_URL")
        .env_remove("VIKUNJA_READ_ONLY");
    if let Some(t) = token {
        cmd.env("VIKUNJA_TOKEN", t);
    }
    if let Some(v) = read_only_env {
        cmd.env("VIKUNJA_READ_ONLY", v);
    }
    let out = cmd.output().unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

#[tokio::test]
async fn doctor_reports_the_mode() {
    let fake = Fake::seeded().await;
    let base = fake.base.clone();
    let token = fake.token.clone();
    let (ok, report) = tokio::task::spawn_blocking(move || {
        doctor(
            &["--base-url", &base, "--stdio", "--read-only"],
            Some(&token),
        )
    })
    .await
    .unwrap();
    assert!(ok, "{report}");
    assert!(report.contains("read-only: "), "{report}");
    assert!(report.contains("write tools removed"), "{report}");
    assert!(report.contains("3 of at most 8"), "{report}");
    assert!(!report.contains(&fake.token), "{report}");

    let base = fake.base.clone();
    let token = fake.token.clone();
    let (ok, report) = tokio::task::spawn_blocking(move || {
        doctor(&["--base-url", &base, "--stdio"], Some(&token))
    })
    .await
    .unwrap();
    assert!(ok, "{report}");
    assert!(report.contains("read-write: "), "{report}");
    assert!(report.contains("6 of at most 8"), "{report}");
}

#[tokio::test]
async fn doctor_reads_read_only_from_the_environment() {
    let fake = Fake::seeded().await;
    for (value, mode) in [
        ("true", "read-only: "),
        ("", "read-write: "),
        ("false", "read-write: "),
    ] {
        let base = fake.base.clone();
        let token = fake.token.clone();
        let (ok, report) = tokio::task::spawn_blocking(move || {
            doctor_env(&["--base-url", &base, "--stdio"], Some(&token), Some(value))
        })
        .await
        .unwrap();
        assert!(ok, "{report}");
        assert!(
            report.contains(mode),
            "VIKUNJA_READ_ONLY={value:?}: {report}"
        );
    }
}

#[test]
fn doctor_without_base_url() {
    // HTTP mode: a warning, the header can supply it.
    let (ok, report) = doctor(&["--bind", "127.0.0.1:0"], None);
    assert!(ok, "{report}");
    assert!(report.contains("X-Upstream-Url"), "{report}");
    // stdio mode: a failure.
    let (ok, report) = doctor(&["--stdio"], Some("t"));
    assert!(!ok, "{report}");
    assert!(report.contains("stdio mode needs --base-url"), "{report}");
}
