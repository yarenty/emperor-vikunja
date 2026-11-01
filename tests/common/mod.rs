//! A small fake Vikunja for tests: the handful of endpoints the client uses, JSON shaped like
//! the real API (v2.x), every request recorded. Tasks are kept as JSON objects; filters are
//! not evaluated (the client filters again locally), pagination is.

#![allow(dead_code)]

pub mod mcp;

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

pub const ZERO_TIME: &str = "0001-01-01T00:00:00Z";

/// One request as the fake saw it.
#[derive(Debug, Clone)]
pub struct Recorded {
    pub method: String,
    pub path: String,
    pub query: String,
    pub auth: Option<String>,
    pub body: Option<Value>,
}

impl Recorded {
    /// Query pairs, decoded.
    pub fn query_pairs(&self) -> Vec<(String, String)> {
        reqwest::Url::parse(&format!("http://x/?{}", self.query))
            .unwrap()
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect()
    }

    pub fn param(&self, name: &str) -> Option<String> {
        self.query_pairs()
            .into_iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v)
    }
}

#[derive(Default)]
pub struct Data {
    pub projects: Vec<Value>,
    pub tasks: Vec<Value>,
    pub labels: Vec<Value>,
    /// (task id, comment)
    pub comments: Vec<(i64, Value)>,
    pub requests: Vec<Recorded>,
}

#[derive(Clone)]
pub struct Fake {
    pub base: String,
    pub token: String,
    /// A token Vikunja knows but that may not do anything: answered with 403.
    pub forbidden_token: String,
    pub data: Arc<Mutex<Data>>,
}

/// A token for this test run only: random enough, never a fixed string.
pub fn fresh_token() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("tk_{nanos:x}_{}", std::process::id())
}

fn user() -> Value {
    json!({ "id": 1, "name": "Alice", "username": "alice", "created": "2026-10-01T09:00:00Z" })
}

/// The instance's users: alice (the token's owner) and bob.
fn users() -> Vec<Value> {
    vec![
        user(),
        json!({ "id": 2, "name": "Bob", "username": "bob", "created": "2026-10-01T09:00:00Z" }),
    ]
}

pub fn task(id: i64, title: &str, project_id: i64) -> Value {
    json!({
        "id": id, "title": title, "description": "", "project_id": project_id,
        "done": false, "done_at": ZERO_TIME, "due_date": ZERO_TIME, "priority": 0,
        "start_date": ZERO_TIME, "end_date": ZERO_TIME, "assignees": null, "labels": null,
        "percent_done": 0, "identifier": format!("#{id}"), "index": id,
        "created": "2026-10-30T08:00:00Z", "updated": "2026-10-30T08:00:00Z",
    })
}

impl Fake {
    /// Start an empty fake on a free port.
    pub async fn start() -> Self {
        let fake = Fake {
            base: String::new(),
            token: fresh_token(),
            forbidden_token: format!("{}_ro", fresh_token()),
            data: Arc::new(Mutex::new(Data::default())),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new().fallback(handle).with_state(fake.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Fake {
            base: format!("http://{addr}"),
            ..fake
        }
    }

    /// Start with a small board: Inbox(1) > Work(2), Home(3); labels `now`(1); four tasks.
    pub async fn seeded() -> Self {
        let fake = Self::start().await;
        {
            let mut d = fake.data.lock().unwrap();
            d.projects = vec![
                json!({ "id": -1, "title": "Favorites", "parent_project_id": 0, "is_archived": false }),
                json!({ "id": 1, "title": "Inbox", "parent_project_id": 0, "is_archived": false }),
                json!({ "id": 2, "title": "Work", "parent_project_id": 1, "is_archived": false }),
                json!({ "id": 3, "title": "Home", "parent_project_id": 0, "is_archived": false }),
            ];
            d.labels = vec![json!({ "id": 1, "title": "now" })];
            let mut t1 = task(1, "Write release notes", 2);
            t1["priority"] = json!(3);
            t1["due_date"] = json!("2026-11-03T00:00:00Z");
            t1["labels"] = json!([{ "id": 1, "title": "now" }]);
            t1["description"] = json!(
                "<h2>Notes</h2><p>Cover the <strong>new tools</strong> &amp; the doctor.</p><ul><li>one</li><li>two</li></ul>"
            );
            let mut t2 = task(2, "Fix the gate", 2);
            t2["priority"] = json!(4);
            let mut t3 = task(3, "Buy milk", 3);
            t3["done"] = json!(true);
            t3["done_at"] = json!("2026-10-31T10:00:00Z");
            let mut t4 = task(4, "Plan the week", 1);
            t4["priority"] = json!(3);
            t4["due_date"] = json!("2026-11-02T00:00:00Z");
            d.tasks = vec![t1, t2, t3, t4];
            d.comments = vec![(
                1,
                json!({ "id": 1, "comment": "<p>Looks <em>good</em> to me.</p>", "author": user(),
                        "created": "2026-10-31T12:00:00Z" }),
            )];
        }
        fake
    }

    pub fn requests(&self) -> Vec<Recorded> {
        self.data.lock().unwrap().requests.clone()
    }

    pub fn requests_to(&self, method: &str, path: &str) -> Vec<Recorded> {
        self.requests()
            .into_iter()
            .filter(|r| r.method == method && r.path == path)
            .collect()
    }

    pub fn task_json(&self, id: i64) -> Value {
        self.data
            .lock()
            .unwrap()
            .tasks
            .iter()
            .find(|t| t["id"] == id)
            .cloned()
            .unwrap()
    }
}

fn json_response(status: StatusCode, body: Value) -> Response {
    (status, axum::Json(body)).into_response()
}

fn not_found() -> Response {
    json_response(
        StatusCode::NOT_FOUND,
        json!({ "code": 4002, "message": "The task does not exist." }),
    )
}

fn paginate(items: Vec<Value>, q: &Recorded) -> Response {
    let per_page: usize = q
        .param("per_page")
        .and_then(|v| v.parse().ok())
        .unwrap_or(50)
        .min(50);
    let page: usize = q
        .param("page")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
        .max(1);
    let total_pages = items.len().div_ceil(per_page).max(1);
    let slice: Vec<Value> = items
        .into_iter()
        .skip((page - 1) * per_page)
        .take(per_page)
        .collect();
    let mut resp = json_response(StatusCode::OK, Value::Array(slice));
    resp.headers_mut().insert(
        "x-pagination-total-pages",
        total_pages.to_string().parse().unwrap(),
    );
    resp
}

async fn handle(
    State(fake): State<Fake>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let rec = Recorded {
        method: method.to_string(),
        path: uri.path().to_string(),
        query: uri.query().unwrap_or_default().to_string(),
        auth: headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(String::from),
        body: serde_json::from_slice(&body).ok(),
    };
    fake.data.lock().unwrap().requests.push(rec.clone());

    let Some(path) = rec.path.strip_prefix("/api/v1/") else {
        return not_found();
    };
    if path == "info" {
        return json_response(StatusCode::OK, json!({ "version": "v2.7.1-fake" }));
    }
    if rec.auth.as_deref() == Some(&format!("Bearer {}", fake.forbidden_token)) {
        return json_response(
            StatusCode::FORBIDDEN,
            json!({ "code": 11, "message": "Forbidden" }),
        );
    }
    if rec.auth.as_deref() != Some(&format!("Bearer {}", fake.token)) {
        return json_response(
            StatusCode::UNAUTHORIZED,
            json!({ "code": 11, "message": "missing, malformed, expired or otherwise invalid token provided" }),
        );
    }

    let segs: Vec<&str> = path.split('/').collect();
    let body = rec.body.clone().unwrap_or(Value::Null);
    let mut d = fake.data.lock().unwrap();
    match (method.as_str(), segs.as_slice()) {
        ("GET", ["user"]) => json_response(StatusCode::OK, user()),
        ("GET", ["users"]) => {
            // Vikunja matches a username exactly (case-insensitive) and answers null for none.
            let s = rec.param("s").unwrap_or_default();
            let hits: Vec<Value> = users()
                .into_iter()
                .filter(|u| u["username"].as_str().unwrap().eq_ignore_ascii_case(&s))
                .collect();
            if hits.is_empty() {
                json_response(StatusCode::OK, Value::Null)
            } else {
                json_response(StatusCode::OK, Value::Array(hits))
            }
        }
        ("PUT", ["tasks", id, "assignees"]) => {
            let uid = body["user_id"].as_i64().unwrap_or(0);
            let Some(u) = users().into_iter().find(|u| u["id"] == uid) else {
                return not_found();
            };
            let Some(t) = d.tasks.iter_mut().find(|t| same_id(&t["id"], id)) else {
                return not_found();
            };
            let mut list = t["assignees"].as_array().cloned().unwrap_or_default();
            list.push(u);
            t["assignees"] = Value::Array(list);
            json_response(StatusCode::CREATED, json!({ "user_id": uid }))
        }
        ("DELETE", ["tasks", id, "assignees", uid]) => {
            let Some(t) = d.tasks.iter_mut().find(|t| same_id(&t["id"], id)) else {
                return not_found();
            };
            let mut list = t["assignees"].as_array().cloned().unwrap_or_default();
            list.retain(|u| !same_id(&u["id"], uid));
            t["assignees"] = Value::Array(list);
            json_response(
                StatusCode::OK,
                json!({ "message": "Successfully deleted." }),
            )
        }
        ("GET", ["projects"]) => paginate(d.projects.clone(), &rec),
        ("GET", ["labels"]) => paginate(d.labels.clone(), &rec),
        ("PUT", ["labels"]) => {
            let id = d.labels.len() as i64 + 1;
            let label = json!({ "id": id, "title": body["title"] });
            d.labels.push(label.clone());
            json_response(StatusCode::CREATED, label)
        }
        ("GET", ["tasks"]) => paginate(d.tasks.clone(), &rec),
        ("PUT", ["projects", pid, "tasks"]) => {
            let pid: i64 = pid.parse().unwrap_or(0);
            if !d.projects.iter().any(|p| p["id"] == pid) {
                return not_found();
            }
            let id = d
                .tasks
                .iter()
                .filter_map(|t| t["id"].as_i64())
                .max()
                .unwrap_or(0)
                + 1;
            let mut t = task(id, body["title"].as_str().unwrap_or_default(), pid);
            for k in ["description", "priority", "due_date"] {
                if !body[k].is_null() {
                    t[k] = body[k].clone();
                }
            }
            d.tasks.push(t.clone());
            json_response(StatusCode::CREATED, t)
        }
        ("GET", ["tasks", id]) => match d.tasks.iter().find(|t| same_id(&t["id"], id)) {
            Some(t) => json_response(StatusCode::OK, t.clone()),
            None => not_found(),
        },
        ("POST", ["tasks", id]) => {
            let Some(t) = d.tasks.iter_mut().find(|t| same_id(&t["id"], id)) else {
                return not_found();
            };
            for k in ["title", "description", "done", "priority", "project_id"] {
                t[k] = body[k].clone();
            }
            t["due_date"] = if body["due_date"].is_null() {
                json!(ZERO_TIME)
            } else {
                body["due_date"].clone()
            };
            json_response(StatusCode::OK, t.clone())
        }
        ("PUT", ["tasks", id, "labels"]) => {
            let lid = body["label_id"].as_i64().unwrap_or(0);
            let Some(label) = d.labels.iter().find(|l| l["id"] == lid).cloned() else {
                return not_found();
            };
            let Some(t) = d.tasks.iter_mut().find(|t| same_id(&t["id"], id)) else {
                return not_found();
            };
            let mut labels = t["labels"].as_array().cloned().unwrap_or_default();
            labels.push(label);
            t["labels"] = Value::Array(labels);
            json_response(StatusCode::CREATED, json!({ "label_id": lid }))
        }
        ("DELETE", ["tasks", id, "labels", lid]) => {
            let Some(t) = d.tasks.iter_mut().find(|t| same_id(&t["id"], id)) else {
                return not_found();
            };
            let mut labels = t["labels"].as_array().cloned().unwrap_or_default();
            labels.retain(|l| !same_id(&l["id"], lid));
            t["labels"] = Value::Array(labels);
            json_response(
                StatusCode::OK,
                json!({ "message": "Successfully deleted." }),
            )
        }
        ("GET", ["tasks", id, "comments"]) => {
            let tid: i64 = id.parse().unwrap_or(0);
            let list: Vec<Value> = d
                .comments
                .iter()
                .filter(|(t, _)| *t == tid)
                .map(|(_, c)| c.clone())
                .collect();
            json_response(StatusCode::OK, Value::Array(list))
        }
        ("PUT", ["tasks", id, "comments"]) => {
            let tid: i64 = id.parse().unwrap_or(0);
            if !d.tasks.iter().any(|t| t["id"] == tid) {
                return not_found();
            }
            let c = json!({ "id": d.comments.len() as i64 + 1, "comment": body["comment"],
                            "author": user(), "created": "2026-11-01T09:00:00Z" });
            d.comments.push((tid, c.clone()));
            json_response(StatusCode::CREATED, c)
        }
        _ => not_found(),
    }
}

fn same_id(v: &Value, id: &str) -> bool {
    id.parse::<i64>().ok() == v.as_i64()
}
