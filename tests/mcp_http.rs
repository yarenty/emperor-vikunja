//! The server end to end: MCP over stateless Streamable HTTP (emperor-mcp `serve`), the token in
//! the forwarded Authorization header, the fake Vikunja upstream.

mod common;

use common::Fake;
use common::mcp::{call, rpc, start};
use serde_json::{Value, json};

fn bearer(fake: &Fake) -> String {
    format!("Bearer {}", fake.token)
}

#[tokio::test]
async fn tools_list_has_exactly_six() {
    let fake = Fake::seeded().await;
    let base = start(&fake.base).await;
    let list = rpc(
        &base,
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
        None,
    )
    .await;
    let mut names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "comment",
            "create_task",
            "get_task",
            "list_projects",
            "list_tasks",
            "update_task"
        ]
    );
    assert!(
        fake.requests().is_empty(),
        "discovery must not call Vikunja"
    );
}

#[tokio::test]
async fn list_tasks_end_to_end() {
    let fake = Fake::seeded().await;
    let base = start(&fake.base).await;
    let auth = bearer(&fake);
    let out = call(&base, Some(&auth), "list_tasks", json!({})).await;
    assert!(!out.is_error, "{}", out.text);
    assert!(
        out.text.contains("computed result"),
        "framed as Computed: {}",
        out.text
    );
    assert!(out.text.contains("BEGIN_DATA_"), "{}", out.text);
    let tasks = out.structured["tasks"].as_array().unwrap();
    let ids: Vec<i64> = tasks.iter().map(|t| t["id"].as_i64().unwrap()).collect();
    // Open tasks only, priority 4 first, then the two priority-3 tasks by due date.
    assert_eq!(ids, vec![2, 4, 1]);
    assert_eq!(tasks[2]["project"], "Work");
    assert_eq!(tasks[2]["labels"], json!(["now"]));
    assert_eq!(tasks[2]["due"], "2026-11-03T00:00:00Z");
    assert_eq!(tasks[0]["due"], Value::Null);
    // The forwarded header reached Vikunja unchanged.
    for r in fake.requests() {
        assert_eq!(
            r.auth.as_deref(),
            Some(auth.as_str()),
            "{} {}",
            r.method,
            r.path
        );
    }
}

#[tokio::test]
async fn list_tasks_filters_and_limit() {
    let fake = Fake::seeded().await;
    let base = start(&fake.base).await;
    let auth = bearer(&fake);
    let out = call(
        &base,
        Some(&auth),
        "list_tasks",
        json!({ "project": "work", "label": "now", "due_before": "2026-11-04" }),
    )
    .await;
    assert!(!out.is_error, "{}", out.text);
    assert_eq!(out.structured["tasks"].as_array().unwrap().len(), 1);
    let req = fake.requests_to("GET", "/api/v1/tasks").pop().unwrap();
    assert_eq!(
        req.param("filter").as_deref(),
        Some("done = false && project = 2 && labels in 1 && due_date < '2026-11-04T00:00:00Z'")
    );

    let done = call(&base, Some(&auth), "list_tasks", json!({ "done": true })).await;
    assert_eq!(done.structured["tasks"][0]["title"], "Buy milk");

    let one = call(&base, Some(&auth), "list_tasks", json!({ "limit": 1 })).await;
    assert_eq!(one.structured["count"], 1);
    assert_eq!(one.structured["total"], 3);

    let none = call(&base, Some(&auth), "list_tasks", json!({ "label": "nope" })).await;
    assert_eq!(none.structured["count"], 0);

    let bad = call(
        &base,
        Some(&auth),
        "list_tasks",
        json!({ "due_before": "soon" }),
    )
    .await;
    assert!(
        bad.is_error && bad.text.contains("YYYY-MM-DD"),
        "{}",
        bad.text
    );
}

#[tokio::test]
async fn get_task_returns_text_and_comments_framed_as_reference() {
    let fake = Fake::seeded().await;
    let base = start(&fake.base).await;
    let out = call(&base, Some(&bearer(&fake)), "get_task", json!({ "id": 1 })).await;
    assert!(!out.is_error, "{}", out.text);
    assert!(
        out.text.contains("internal knowledge base"),
        "TrustedReference: {}",
        out.text
    );
    let t = &out.structured;
    let desc = t["description"].as_str().unwrap();
    assert!(
        desc.contains("new tools") && desc.contains("& the doctor"),
        "{desc}"
    );
    assert!(!desc.contains('<'), "HTML must be gone: {desc}");
    assert_eq!(t["project"]["title"], "Work");
    assert_eq!(t["done_at"], Value::Null);
    assert_eq!(t["comments"][0]["author"], "alice");
    assert_eq!(t["comments"][0]["date"], "2026-10-31T12:00:00Z");
    assert!(t["comments"][0]["text"].as_str().unwrap().contains("good"));
    assert!(t["url"].as_str().unwrap().ends_with("/tasks/1"));

    let missing = call(&base, Some(&bearer(&fake)), "get_task", json!({ "id": 99 })).await;
    assert!(
        missing.is_error && missing.text.contains("not found"),
        "{}",
        missing.text
    );
}

#[tokio::test]
async fn create_task_converts_markdown_and_creates_labels() {
    let fake = Fake::seeded().await;
    let base = start(&fake.base).await;
    let out = call(
        &base,
        Some(&bearer(&fake)),
        "create_task",
        json!({
            "project": "Home", "title": "Paint the fence",
            "description": "Use **white** paint.\n\n- buy brushes\n- tape", "labels": ["now", "weekend"],
            "priority": 2, "due": "2026-11-08"
        }),
    )
    .await;
    assert!(!out.is_error, "{}", out.text);
    assert_eq!(out.structured["id"], 5);
    assert_eq!(out.structured["project"], "Home");
    assert_eq!(out.structured["labels"], json!(["now", "weekend"]));

    let put = fake
        .requests_to("PUT", "/api/v1/projects/3/tasks")
        .pop()
        .unwrap();
    let body = put.body.unwrap();
    assert!(
        body["description"]
            .as_str()
            .unwrap()
            .contains("<strong>white</strong>"),
        "{body}"
    );
    assert!(
        body["description"]
            .as_str()
            .unwrap()
            .contains("<li>buy brushes</li>"),
        "{body}"
    );
    assert_eq!(body["due_date"], "2026-11-08T00:00:00Z");
    assert_eq!(body["priority"], 2);
    let created = fake.requests_to("PUT", "/api/v1/labels");
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].body.as_ref().unwrap()["title"], "weekend");
    assert_eq!(fake.requests_to("PUT", "/api/v1/tasks/5/labels").len(), 2);
    assert_eq!(fake.task_json(5)["labels"].as_array().unwrap().len(), 2);

    let bad = call(
        &base,
        Some(&bearer(&fake)),
        "create_task",
        json!({ "project": "Home", "title": "x", "priority": 9 }),
    )
    .await;
    assert!(bad.is_error && bad.text.contains("0 to 5"), "{}", bad.text);
    let nowhere = call(
        &base,
        Some(&bearer(&fake)),
        "create_task",
        json!({ "project": "Garden", "title": "x" }),
    )
    .await;
    assert!(
        nowhere.is_error && nowhere.text.contains("no project named"),
        "{}",
        nowhere.text
    );
}

#[tokio::test]
async fn update_task_changes_fields_labels_and_project() {
    let fake = Fake::seeded().await;
    let base = start(&fake.base).await;
    let out = call(
        &base,
        Some(&bearer(&fake)),
        "update_task",
        json!({
            "id": 1, "title": "Write the release notes", "done": true, "priority": 5,
            "due": "none", "add_labels": ["review"], "remove_labels": ["now"], "project": 3
        }),
    )
    .await;
    assert!(!out.is_error, "{}", out.text);
    assert_eq!(out.structured["done"], true);
    assert_eq!(out.structured["project"], "Home");
    assert_eq!(out.structured["labels"], json!(["review"]));
    assert_eq!(out.structured["due"], Value::Null);

    // The update is a full object built from the current task (Vikunja replaces every field).
    let post = fake.requests_to("POST", "/api/v1/tasks/1").pop().unwrap();
    let body = post.body.unwrap();
    assert_eq!(body["title"], "Write the release notes");
    assert_eq!(body["project_id"], 3);
    assert_eq!(body["due_date"], Value::Null);
    assert!(
        body["description"]
            .as_str()
            .unwrap()
            .contains("<h2>Notes</h2>"),
        "untouched description is sent back as it was: {body}"
    );
    assert_eq!(
        fake.requests_to("DELETE", "/api/v1/tasks/1/labels/1").len(),
        1
    );

    let nothing = call(
        &base,
        Some(&bearer(&fake)),
        "update_task",
        json!({ "id": 1 }),
    )
    .await;
    assert!(
        nothing.is_error && nothing.text.contains("nothing to change"),
        "{}",
        nothing.text
    );
}

#[tokio::test]
async fn comment_posts_html() {
    let fake = Fake::seeded().await;
    let base = start(&fake.base).await;
    let out = call(
        &base,
        Some(&bearer(&fake)),
        "comment",
        json!({ "id": 2, "text": "Done in `ci.yml`, see **logs**." }),
    )
    .await;
    assert!(!out.is_error, "{}", out.text);
    assert_eq!(out.structured["task_id"], 2);
    let put = fake
        .requests_to("PUT", "/api/v1/tasks/2/comments")
        .pop()
        .unwrap();
    let html = put.body.unwrap()["comment"].as_str().unwrap().to_string();
    assert!(
        html.contains("<code>ci.yml</code>") && html.contains("<strong>logs</strong>"),
        "{html}"
    );
}

#[tokio::test]
async fn list_projects_counts_open_tasks() {
    let fake = Fake::seeded().await;
    let base = start(&fake.base).await;
    let out = call(&base, Some(&bearer(&fake)), "list_projects", json!({})).await;
    assert!(!out.is_error, "{}", out.text);
    let ps = out.structured["projects"].as_array().unwrap();
    assert_eq!(ps.len(), 3, "pseudo projects are left out: {ps:?}");
    let work = ps.iter().find(|p| p["title"] == "Work").unwrap();
    assert_eq!(work["open_tasks"], 2);
    assert_eq!(work["parent"]["title"], "Inbox");
    let home = ps.iter().find(|p| p["title"] == "Home").unwrap();
    assert_eq!(home["open_tasks"], 0, "the done task does not count");
    assert_eq!(home["parent"], Value::Null);
}

#[tokio::test]
async fn missing_token_is_a_clear_error_and_vikunja_is_not_called() {
    let fake = Fake::seeded().await;
    let base = start(&fake.base).await;
    for tool in ["list_tasks", "list_projects"] {
        let out = call(&base, None, tool, json!({})).await;
        assert!(out.is_error, "{tool}: {}", out.text);
        assert!(out.text.contains("No Vikunja token"), "{}", out.text);
        assert!(out.text.contains("Authorization: Bearer"), "{}", out.text);
    }
    let basic = call(
        &base,
        Some("Basic YWxpY2U6eA=="),
        "get_task",
        json!({ "id": 1 }),
    )
    .await;
    assert!(
        basic.is_error && basic.text.contains("No Vikunja token"),
        "{}",
        basic.text
    );
    assert!(
        fake.requests().is_empty(),
        "no upstream call without a token"
    );
}

#[tokio::test]
async fn wrong_token_reports_vikunjas_refusal() {
    let fake = Fake::seeded().await;
    let base = start(&fake.base).await;
    let out = call(&base, Some("Bearer wrong"), "list_projects", json!({})).await;
    assert!(out.is_error);
    assert!(out.text.contains("refused the token"), "{}", out.text);
}
