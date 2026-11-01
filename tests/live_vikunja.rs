//! Every tool against a real Vikunja in Docker. Ignored by default; run with
//!
//! ```sh
//! cargo test --test live_vikunja -- --ignored --nocapture
//! ```
//!
//! Starts a throwaway `vikunja/vikunja` container (override the image with
//! `EMPEROR_VIKUNJA_IMAGE`), registers a user with a random password, creates an API token
//! with the permissions the README lists, and drives the six tools through the MCP HTTP server.
//! The container is removed afterwards. Skipped when `docker info` fails.

mod common;

use std::process::Command;
use std::time::Duration;

use common::mcp::{call, start};
use serde_json::{Value, json};

const DEFAULT_IMAGE: &str = "vikunja/vikunja:latest";

struct Container(String);

impl Drop for Container {
    fn drop(&mut self) {
        let _ = Command::new("docker").args(["rm", "-f", &self.0]).output();
    }
}

fn docker(args: &[&str]) -> String {
    let out = Command::new("docker")
        .args(args)
        .output()
        .expect("docker runs");
    assert!(
        out.status.success(),
        "docker {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn random_secret() -> String {
    // Throwaway credentials for a throwaway container, made up at run time.
    let a = common::fresh_token();
    let b = format!("{:x}", std::ptr::addr_of!(a) as usize);
    format!("{a}{b}")
}

/// The README's token permissions, as Vikunja's `PUT /tokens` takes them.
fn token_permissions() -> Value {
    json!({
        "projects": ["read_all", "read_one"],
        "tasks": ["read_all", "read_one", "create", "update"],
        "tasks_comments": ["read_all", "create"],
        "tasks_labels": ["read_all", "create", "delete"],
        "labels": ["read_all", "create"],
        "other": ["user"]
    })
}

#[tokio::test]
#[ignore = "needs Docker; starts a vikunja/vikunja container"]
async fn all_tools_against_real_vikunja() {
    if !Command::new("docker")
        .arg("info")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        eprintln!("docker is not available; skipping");
        return;
    }
    let image = std::env::var("EMPEROR_VIKUNJA_IMAGE").unwrap_or_else(|_| DEFAULT_IMAGE.into());
    let secret = random_secret();
    let id = docker(&[
        "run",
        "-d",
        "-p",
        "127.0.0.1::3456",
        "-e",
        &format!("VIKUNJA_SERVICE_SECRET={secret}"),
        "-e",
        "VIKUNJA_SERVICE_PUBLICURL=http://127.0.0.1/",
        "-e",
        "VIKUNJA_DATABASE_PATH=/tmp/vikunja.db",
        "-e",
        "VIKUNJA_FILES_BASEPATH=/tmp/files",
        "--tmpfs",
        "/tmp:uid=1000",
        &image,
    ]);
    let _container = Container(id.clone());
    let port = docker(&["port", &id, "3456/tcp"]);
    let base = format!("http://{}", port.lines().next().unwrap());
    let api = format!("{base}/api/v1");
    let http = reqwest::Client::new();

    let mut up = false;
    for _ in 0..120 {
        if let Ok(r) = http.get(format!("{api}/info")).send().await
            && r.status().is_success()
        {
            up = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert!(up, "Vikunja did not come up: {}", docker(&["logs", &id]));

    // A user, a login, and an API token scoped to what the tools need.
    let password = random_secret();
    let reg = http
        .post(format!("{api}/register"))
        .json(&json!({ "username": "mcpuser", "email": "mcp@example.com", "password": password }))
        .send()
        .await
        .unwrap();
    assert!(
        reg.status().is_success(),
        "register: {}",
        reg.text().await.unwrap()
    );
    let login: Value = http
        .post(format!("{api}/login"))
        .json(&json!({ "username": "mcpuser", "password": password }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let jwt = login["token"].as_str().expect("login token").to_string();
    let made: Value = http
        .put(format!("{api}/tokens"))
        .bearer_auth(&jwt)
        .json(&json!({ "title": "mcp", "expires_at": "2099-01-01T00:00:00Z", "permissions": token_permissions() }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let token = made["token"].as_str().expect("api token").to_string();
    // A second project (projects are not something the tools create).
    let work: Value = http
        .put(format!("{api}/projects"))
        .bearer_auth(&jwt)
        .json(&json!({ "title": "Work" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let work_id = work["id"].as_i64().unwrap();

    let mcp = start(&base).await;
    let auth = format!("Bearer {token}");
    let auth = Some(auth.as_str());

    // list_projects
    let projects = call(&mcp, auth, "list_projects", json!({})).await;
    assert!(!projects.is_error, "{}", projects.text);
    let titles: Vec<&str> = projects.structured["projects"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["title"].as_str())
        .collect();
    assert!(
        titles.contains(&"Inbox") && titles.contains(&"Work"),
        "{titles:?}"
    );

    // create_task, with a new label and Markdown
    let created = call(
        &mcp,
        auth,
        "create_task",
        json!({ "project": "Work", "title": "Ship 0.1", "description": "Check the **gate** first.",
                "labels": ["release"], "priority": 4, "due": "2026-11-05" }),
    )
    .await;
    assert!(!created.is_error, "{}", created.text);
    let id = created.structured["id"].as_i64().unwrap();
    let second = call(
        &mcp,
        auth,
        "create_task",
        json!({ "project": "Inbox", "title": "Later", "priority": 1 }),
    )
    .await;
    assert!(!second.is_error, "{}", second.text);

    // list_tasks: sort and filters, against Vikunja's own filter engine
    let open = call(&mcp, auth, "list_tasks", json!({})).await;
    assert!(!open.is_error, "{}", open.text);
    assert_eq!(open.structured["tasks"][0]["id"], id, "{}", open.text);
    assert_eq!(open.structured["tasks"][0]["labels"], json!(["release"]));
    let filtered = call(
        &mcp,
        auth,
        "list_tasks",
        json!({ "project": work_id, "label": "release", "due_before": "2026-11-06", "search": "Ship" }),
    )
    .await;
    assert_eq!(filtered.structured["count"], 1, "{}", filtered.text);
    let none = call(
        &mcp,
        auth,
        "list_tasks",
        json!({ "due_before": "2026-11-04" }),
    )
    .await;
    assert_eq!(none.structured["count"], 0, "{}", none.text);

    // comment, then get_task reads it back as text
    let c = call(
        &mcp,
        auth,
        "comment",
        json!({ "id": id, "text": "Gate is **green**." }),
    )
    .await;
    assert!(!c.is_error, "{}", c.text);
    let got = call(&mcp, auth, "get_task", json!({ "id": id })).await;
    assert!(!got.is_error, "{}", got.text);
    assert!(
        got.structured["description"]
            .as_str()
            .unwrap()
            .contains("gate"),
        "{}",
        got.text
    );
    assert!(
        !got.structured["description"]
            .as_str()
            .unwrap()
            .contains('<')
    );
    assert_eq!(got.structured["comments"][0]["author"], "mcpuser");
    assert!(
        got.structured["comments"][0]["text"]
            .as_str()
            .unwrap()
            .contains("green")
    );
    assert_eq!(got.structured["due"], "2026-11-05T00:00:00Z");

    // update_task: done, move, labels, clear due
    let upd = call(
        &mcp,
        auth,
        "update_task",
        json!({ "id": id, "done": true, "project": "Inbox", "add_labels": ["shipped"],
                "remove_labels": ["release"], "due": "none", "title": "Shipped 0.1" }),
    )
    .await;
    assert!(!upd.is_error, "{}", upd.text);
    let after = call(&mcp, auth, "get_task", json!({ "id": id })).await;
    assert_eq!(after.structured["done"], true, "{}", after.text);
    assert_eq!(after.structured["title"], "Shipped 0.1");
    assert_eq!(after.structured["project"]["title"], "Inbox");
    assert_eq!(after.structured["labels"], json!(["shipped"]));
    assert_eq!(after.structured["due"], Value::Null, "{}", after.text);
    assert!(
        after.structured["description"]
            .as_str()
            .unwrap()
            .contains("gate"),
        "description kept"
    );
    let done = call(&mcp, auth, "list_tasks", json!({ "done": true })).await;
    assert_eq!(done.structured["count"], 1, "{}", done.text);

    // no token, wrong token
    let anon = call(&mcp, None, "list_tasks", json!({})).await;
    assert!(anon.is_error && anon.text.contains("No Vikunja token"));
    let wrong = call(&mcp, Some("Bearer tk_wrong"), "list_projects", json!({})).await;
    assert!(
        wrong.is_error && wrong.text.contains("refused the token"),
        "{}",
        wrong.text
    );

    // doctor, as an operator would run it
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_emperor-vikunja"))
        .args(["doctor", "--base-url", &base, "--stdio"])
        .env("VIKUNJA_TOKEN", &token)
        .output()
        .unwrap();
    let report = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{report}");
    assert!(report.contains("token works for user mcpuser"), "{report}");
    assert!(!report.contains(&token), "doctor must not print the token");
    eprintln!("{report}");
}
