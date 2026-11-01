//! A thin Vikunja REST client: only the endpoints the tools need.
//!
//! [`Vikunja`] holds the base URL and a connection pool and nothing else. Every authenticated
//! call goes through a [`Session`], which borrows a [`Token`] for the length of one tool call:
//! the client never keeps a token (Emperor Profile E6).

use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::{Method, RequestBuilder, Response, StatusCode, Url};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::dates;

/// Timeout for one upstream HTTP call.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// Items requested per page; Vikunja caps this at its own `maxitemsperpage` (50 by default).
const PER_PAGE: u32 = 50;

/// Most pages read for one listing, so a huge instance cannot keep a tool call busy forever.
const MAX_PAGES: u32 = 40;

/// Pagination header Vikunja sets on list responses.
const TOTAL_PAGES_HEADER: &str = "x-pagination-total-pages";

/// What can go wrong talking to Vikunja.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The base URL is not a usable http(s) URL.
    #[error("invalid Vikunja URL `{0}`: use something like https://vikunja.example.com")]
    BadUrl(String),
    /// A caller-supplied upstream URL that is plain `http` to a host that is not loopback.
    #[error("refusing X-Upstream-Url `{0}`: use https (plain http is allowed only for localhost)")]
    InsecureUpstream(String),
    /// Neither the request nor the server configuration names a Vikunja instance.
    #[error(
        "no Vikunja URL: send the X-Upstream-Url header with the request, or start the server with --base-url"
    )]
    NoUpstream,
    /// Vikunja refused the token.
    #[error("Vikunja refused the token (HTTP {status}): {message}")]
    Unauthorized {
        /// HTTP status (401 or 403).
        status: u16,
        /// Vikunja's message.
        message: String,
    },
    /// The task, project or comment does not exist (or the token cannot see it).
    #[error("not found: {0}")]
    NotFound(String),
    /// Any other non-success answer.
    #[error("Vikunja answered HTTP {status}: {message}")]
    Status {
        /// HTTP status.
        status: u16,
        /// Vikunja's message, or the start of the body.
        message: String,
    },
    /// Network failure, timeout, or a body that is not the expected JSON.
    #[error("cannot talk to Vikunja: {0}")]
    Transport(String),
    /// The caller asked for something that cannot work (unknown project name, bad date...).
    #[error("{0}")]
    Invalid(String),
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        // reqwest's message names the URL, never the Authorization header.
        Self::Transport(e.to_string())
    }
}

/// Result alias for this module.
pub type Result<T> = std::result::Result<T, Error>;

/// A Vikunja API token for one call. `Debug` and `Display` never show it.
#[derive(Clone)]
pub struct Token(String);

impl Token {
    /// A token from its raw value (as created in Vikunja's settings). Empty means none.
    pub fn new(raw: impl Into<String>) -> Option<Self> {
        let raw = raw.into();
        let raw = raw.trim();
        (!raw.is_empty()).then(|| Self(raw.to_string()))
    }

    /// A token from an `Authorization` header value (`Bearer <token>`; the scheme is
    /// case-insensitive, a bare token is accepted too).
    pub fn from_authorization(header: &str) -> Option<Self> {
        let header = header.trim();
        match header.split_once(' ') {
            Some((scheme, rest)) if scheme.eq_ignore_ascii_case("bearer") => Self::new(rest),
            Some(_) => None,
            None if header.eq_ignore_ascii_case("bearer") => None,
            None => Self::new(header),
        }
    }

    fn bearer(&self) -> String {
        format!("Bearer {}", self.0)
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(***)")
    }
}

/* --- API shapes -------------------------------------------------------------------------------- */

fn null_as_empty<'de, D, T>(d: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
}

/// `GET /info`.
#[derive(Debug, Clone, Deserialize)]
pub struct Info {
    /// Vikunja version, e.g. `v1.0.0`.
    #[serde(default)]
    pub version: String,
}

/// A user (the token's owner, a comment author, an assignee).
#[derive(Debug, Clone, Deserialize)]
pub struct User {
    /// User id.
    pub id: i64,
    /// Login name.
    #[serde(default)]
    pub username: String,
    /// Display name.
    #[serde(default)]
    pub name: String,
}

/// A project.
#[derive(Debug, Clone, Deserialize)]
pub struct Project {
    /// Project id. Pseudo projects (favourites, saved filters) have ids below 1.
    pub id: i64,
    /// Title.
    pub title: String,
    /// Parent project id, `0` for a top-level project.
    #[serde(default)]
    pub parent_project_id: i64,
    /// Archived flag.
    #[serde(default)]
    pub is_archived: bool,
}

/// A label.
#[derive(Debug, Clone, Deserialize)]
pub struct Label {
    /// Label id.
    pub id: i64,
    /// Title.
    pub title: String,
}

/// A task.
#[derive(Debug, Clone, Deserialize)]
pub struct Task {
    /// Task id (global).
    pub id: i64,
    /// Title.
    pub title: String,
    /// Description, HTML.
    #[serde(default)]
    pub description: String,
    /// Done flag.
    #[serde(default)]
    pub done: bool,
    /// When it was marked done (zero time when open).
    #[serde(default)]
    pub done_at: Option<String>,
    /// Priority 0 (unset) to 5 (do now).
    #[serde(default)]
    pub priority: i64,
    /// Due date (zero time when unset).
    #[serde(default)]
    pub due_date: Option<String>,
    /// Start date.
    #[serde(default)]
    pub start_date: Option<String>,
    /// End date.
    #[serde(default)]
    pub end_date: Option<String>,
    /// Owning project.
    #[serde(default)]
    pub project_id: i64,
    /// Labels.
    #[serde(default, deserialize_with = "null_as_empty")]
    pub labels: Vec<Label>,
    /// Assignees.
    #[serde(default, deserialize_with = "null_as_empty")]
    pub assignees: Vec<User>,
    /// Progress, 0.0 to 1.0.
    #[serde(default)]
    pub percent_done: f64,
    /// Project-scoped identifier such as `#12`.
    #[serde(default)]
    pub identifier: String,
    /// Created timestamp.
    #[serde(default)]
    pub created: Option<String>,
    /// Updated timestamp.
    #[serde(default)]
    pub updated: Option<String>,
}

impl Task {
    /// The due date when set.
    pub fn due(&self) -> Option<DateTime<Utc>> {
        dates::parsed(self.due_date.as_deref())
    }

    /// True when the task carries the label `label_id`.
    pub fn has_label(&self, label_id: i64) -> bool {
        self.labels.iter().any(|l| l.id == label_id)
    }
}

/// A task comment.
#[derive(Debug, Clone, Deserialize)]
pub struct Comment {
    /// Comment id.
    pub id: i64,
    /// Body, HTML.
    #[serde(default)]
    pub comment: String,
    /// Author.
    #[serde(default)]
    pub author: Option<User>,
    /// Created timestamp.
    #[serde(default)]
    pub created: Option<String>,
}

/// Filters for [`Session::tasks`]. Everything is optional; `done: Some(false)` is the usual
/// "open tasks" view.
#[derive(Debug, Clone, Default)]
pub struct TaskQuery {
    /// Only this project.
    pub project_id: Option<i64>,
    /// Only tasks with this label.
    pub label_id: Option<i64>,
    /// Done flag.
    pub done: Option<bool>,
    /// Only tasks due strictly before this instant (tasks without a due date are left out).
    pub due_before: Option<DateTime<Utc>>,
    /// Free-text search.
    pub search: Option<String>,
}

impl TaskQuery {
    /// The Vikunja filter expression (`done = false && project = 3 && ...`), if any.
    pub fn filter_expression(&self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(done) = self.done {
            parts.push(format!("done = {done}"));
        }
        if let Some(p) = self.project_id {
            parts.push(format!("project = {p}"));
        }
        if let Some(l) = self.label_id {
            parts.push(format!("labels in {l}"));
        }
        if let Some(d) = self.due_before {
            parts.push(format!("due_date < '{}'", dates::format(d)));
        }
        (!parts.is_empty()).then(|| parts.join(" && "))
    }

    /// True when `task` passes every filter (applied again locally, so results stay right
    /// even on an instance that ignores part of the filter expression).
    pub fn matches(&self, task: &Task) -> bool {
        if self.done.is_some_and(|d| d != task.done) {
            return false;
        }
        if self.project_id.is_some_and(|p| p != task.project_id) {
            return false;
        }
        if self.label_id.is_some_and(|l| !task.has_label(l)) {
            return false;
        }
        if let Some(limit) = self.due_before {
            match task.due() {
                Some(due) if due < limit => {}
                _ => return false,
            }
        }
        true
    }
}

/// Sort tasks by priority (highest first), then due date (soonest first, undated last), then id.
pub fn sort_tasks(tasks: &mut [Task]) {
    tasks.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| match (a.due(), b.due()) {
                (Some(x), Some(y)) => x.cmp(&y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            })
            .then_with(|| a.id.cmp(&b.id))
    });
}

/* --- client ------------------------------------------------------------------------------------ */

fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent(concat!("emperor-vikunja/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| Error::Transport(e.to_string()))
}

/// Where tool calls go: the instance a caller names in the `X-Upstream-Url` header, else the
/// operator's `--base-url`. One connection pool serves both.
#[derive(Clone, Debug)]
pub struct Upstreams {
    http: reqwest::Client,
    default: Option<Vikunja>,
}

impl Upstreams {
    /// Upstreams with an optional default instance (`--base-url`).
    pub fn new(default_base_url: Option<&str>) -> Result<Self> {
        let http = http_client()?;
        let default = default_base_url
            .map(|b| Vikunja::with_http(http.clone(), b))
            .transpose()?;
        Ok(Self { http, default })
    }

    /// Upstreams that always use `client` (stdio mode, tests).
    pub fn fixed(client: Vikunja) -> Self {
        Self {
            http: client.http.clone(),
            default: Some(client),
        }
    }

    /// The default instance, if one was configured.
    pub fn default_instance(&self) -> Option<&Vikunja> {
        self.default.as_ref()
    }

    /// The instance for one call. A caller-named URL must be `https`, except for a loopback
    /// host (`localhost`, `127.0.0.0/8`, `::1`), where `http` is allowed too.
    pub fn resolve(&self, requested: Option<&str>) -> Result<Vikunja> {
        match requested.map(str::trim).filter(|r| !r.is_empty()) {
            Some(raw) => {
                let base = parse_base(raw)?;
                if base.scheme() != "https" && !is_loopback(&base) {
                    return Err(Error::InsecureUpstream(raw.to_string()));
                }
                Vikunja::with_http(self.http.clone(), raw)
            }
            None => self.default.clone().ok_or(Error::NoUpstream),
        }
    }
}

/// Parse and normalise a base URL: http(s) with a host, trailing `/` and `/api/v1` dropped.
fn parse_base(base_url: &str) -> Result<Url> {
    let mut trimmed = base_url.trim().trim_end_matches('/').to_string();
    if let Some(stripped) = trimmed.strip_suffix("/api/v1") {
        trimmed = stripped.to_string();
    }
    let base = Url::parse(&format!("{trimmed}/")).map_err(|_| Error::BadUrl(base_url.into()))?;
    if !matches!(base.scheme(), "http" | "https") || base.host_str().is_none() {
        return Err(Error::BadUrl(base_url.into()));
    }
    Ok(base)
}

/// True for `localhost`, `127.0.0.0/8` and `::1`.
fn is_loopback(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// A Vikunja instance: base URL plus a connection pool. Holds no credentials.
#[derive(Clone, Debug)]
pub struct Vikunja {
    http: reqwest::Client,
    base: Url,
    api: Url,
}

impl Vikunja {
    /// A client for the instance at `base_url` (`https://vikunja.example.com`; a trailing
    /// `/api/v1` is accepted and dropped).
    pub fn new(base_url: &str) -> Result<Self> {
        Self::with_http(http_client()?, base_url)
    }

    fn with_http(http: reqwest::Client, base_url: &str) -> Result<Self> {
        let base = parse_base(base_url)?;
        let api = base
            .join("api/v1/")
            .map_err(|_| Error::BadUrl(base_url.into()))?;
        Ok(Self { http, base, api })
    }

    /// The instance's web address, e.g. `https://vikunja.example.com/`.
    pub fn base_url(&self) -> &Url {
        &self.base
    }

    /// Link to a task in the web UI.
    pub fn task_url(&self, id: i64) -> String {
        format!("{}tasks/{id}", self.base)
    }

    /// `GET /info` (no token needed).
    pub async fn info(&self) -> Result<Info> {
        let resp = self.http.get(self.url("info")?).send().await?;
        decode(resp, "info").await
    }

    /// Authenticated calls with `token`, for the length of one tool call.
    pub fn session<'a>(&'a self, token: &'a Token) -> Session<'a> {
        Session {
            client: self,
            token,
        }
    }

    fn url(&self, path: &str) -> Result<Url> {
        self.api
            .join(path)
            .map_err(|e| Error::Invalid(format!("bad API path {path}: {e}")))
    }
}

/// Authenticated access to Vikunja with one borrowed [`Token`].
pub struct Session<'a> {
    client: &'a Vikunja,
    token: &'a Token,
}

impl Session<'_> {
    fn request(&self, method: Method, path: &str) -> Result<RequestBuilder> {
        Ok(self
            .client
            .http
            .request(method, self.client.url(path)?)
            .header(reqwest::header::AUTHORIZATION, self.token.bearer()))
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, what: &str) -> Result<T> {
        decode(self.request(Method::GET, path)?.send().await?, what).await
    }

    async fn send<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: &Value,
        what: &str,
    ) -> Result<T> {
        let resp = self.request(method, path)?.json(body).send().await?;
        decode(resp, what).await
    }

    /// Every page of a list endpoint.
    async fn all_pages<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
        what: &str,
    ) -> Result<Vec<T>> {
        let mut out = Vec::new();
        for page in 1..=MAX_PAGES {
            let resp = self
                .request(Method::GET, path)?
                .query(query)
                .query(&[("page", page), ("per_page", PER_PAGE)])
                .send()
                .await?;
            let total_pages = resp
                .headers()
                .get(TOTAL_PAGES_HEADER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u32>().ok());
            let items: Option<Vec<T>> = decode(resp, what).await?;
            let items = items.unwrap_or_default();
            let last = items.is_empty()
                || match total_pages {
                    Some(total) => page >= total,
                    None => items.len() < PER_PAGE as usize,
                };
            out.extend(items);
            if last {
                break;
            }
        }
        Ok(out)
    }

    /// `GET /user`: the token's owner.
    pub async fn user(&self) -> Result<User> {
        self.get("user", "user").await
    }

    /// `GET /projects`, every page, archived ones left out.
    pub async fn projects(&self) -> Result<Vec<Project>> {
        self.all_pages("projects", &[], "projects").await
    }

    /// `GET /labels`, every page.
    pub async fn labels(&self) -> Result<Vec<Label>> {
        self.all_pages("labels", &[], "labels").await
    }

    /// `PUT /labels`: create a label.
    pub async fn create_label(&self, title: &str) -> Result<Label> {
        self.send(Method::PUT, "labels", &json!({ "title": title }), "label")
            .await
    }

    /// Labels by title (case-insensitive). Titles with no label are created when
    /// `create_missing` is true, and refused (nothing created) when it is false.
    pub async fn resolve_labels(
        &self,
        titles: &[String],
        create_missing: bool,
    ) -> Result<Vec<Label>> {
        let wanted: Vec<&str> = titles
            .iter()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .collect();
        if wanted.is_empty() {
            return Ok(Vec::new());
        }
        let mut known = self.labels().await?;
        if !create_missing {
            let missing: Vec<&str> = wanted
                .iter()
                .copied()
                .filter(|w| !known.iter().any(|l| l.title.eq_ignore_ascii_case(w)))
                .collect();
            if !missing.is_empty() {
                return Err(Error::Invalid(format!(
                    "no label named {}; pass create_missing_labels: true to create them",
                    missing
                        .iter()
                        .map(|m| format!("`{m}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
        }
        let mut out: Vec<Label> = Vec::new();
        for title in wanted {
            let found = known
                .iter()
                .find(|l| l.title.eq_ignore_ascii_case(title))
                .cloned();
            let label = match found {
                Some(l) => l,
                None => {
                    let created = self.create_label(title).await?;
                    known.push(created.clone());
                    created
                }
            };
            if !out.iter().any(|l| l.id == label.id) {
                out.push(label);
            }
        }
        Ok(out)
    }

    /// Users by username (case-insensitive exact match, through `GET /users?s=`). An unknown
    /// name is refused with every unknown name listed.
    pub async fn resolve_users(&self, usernames: &[String]) -> Result<Vec<User>> {
        let mut out: Vec<User> = Vec::new();
        let mut unknown: Vec<String> = Vec::new();
        for name in usernames.iter().map(|n| n.trim()).filter(|n| !n.is_empty()) {
            let name = name.trim_start_matches('@');
            let hits: Option<Vec<User>> = decode(
                self.request(Method::GET, "users")?
                    .query(&[("s", name)])
                    .send()
                    .await?,
                "users",
            )
            .await?;
            match hits
                .unwrap_or_default()
                .into_iter()
                .find(|u| u.username.eq_ignore_ascii_case(name))
            {
                Some(u) if !out.iter().any(|o| o.id == u.id) => out.push(u),
                Some(_) => {}
                None => unknown.push(format!("`{name}`")),
            }
        }
        if unknown.is_empty() {
            Ok(out)
        } else {
            Err(Error::Invalid(format!(
                "no user named {} (assignees are usernames)",
                unknown.join(", ")
            )))
        }
    }

    /// `PUT /tasks/{id}/assignees`: assign a user to a task.
    pub async fn add_assignee(&self, task_id: i64, user_id: i64) -> Result<()> {
        let _: Value = self
            .send(
                Method::PUT,
                &format!("tasks/{task_id}/assignees"),
                &json!({ "user_id": user_id }),
                &format!("task {task_id}"),
            )
            .await?;
        Ok(())
    }

    /// `DELETE /tasks/{id}/assignees/{user}`: unassign a user.
    pub async fn remove_assignee(&self, task_id: i64, user_id: i64) -> Result<()> {
        let resp = self
            .request(
                Method::DELETE,
                &format!("tasks/{task_id}/assignees/{user_id}"),
            )?
            .send()
            .await?;
        let _: Value = decode(resp, &format!("assignee {user_id} on task {task_id}")).await?;
        Ok(())
    }

    /// `GET /tasks` with the query's filter, every page, then filtered and sorted locally.
    pub async fn tasks(&self, query: &TaskQuery) -> Result<Vec<Task>> {
        let mut params: Vec<(&str, String)> = vec![
            ("sort_by[]", "priority".into()),
            ("order_by[]", "desc".into()),
            ("sort_by[]", "due_date".into()),
            ("order_by[]", "asc".into()),
            ("sort_by[]", "id".into()),
            ("order_by[]", "asc".into()),
        ];
        if let Some(filter) = query.filter_expression() {
            params.push(("filter", filter));
            params.push(("filter_timezone", "UTC".into()));
        }
        if let Some(s) = query
            .search
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            params.push(("s", s.to_string()));
        }
        // Vikunja 1.x and 2.x list tasks at `/tasks`; releases before that at `/tasks/all`.
        let mut tasks: Vec<Task> = match self.all_pages("tasks", &params, "tasks").await {
            Err(Error::NotFound(_) | Error::Status { status: 405, .. }) => {
                self.all_pages("tasks/all", &params, "tasks").await?
            }
            other => other?,
        };
        tasks.retain(|t| query.matches(t));
        sort_tasks(&mut tasks);
        Ok(tasks)
    }

    /// `GET /tasks/{id}`.
    pub async fn task(&self, id: i64) -> Result<Task> {
        self.get(&format!("tasks/{id}"), &format!("task {id}"))
            .await
    }

    /// `GET /tasks/{id}` as raw JSON (the base for a full-object update).
    pub async fn task_json(&self, id: i64) -> Result<Value> {
        self.get(&format!("tasks/{id}"), &format!("task {id}"))
            .await
    }

    /// `PUT /projects/{project}/tasks`: create a task. `fields` are merged into the body.
    pub async fn create_task(&self, project_id: i64, fields: Value) -> Result<Task> {
        self.send(
            Method::PUT,
            &format!("projects/{project_id}/tasks"),
            &fields,
            &format!("project {project_id}"),
        )
        .await
    }

    /// `POST /tasks/{id}`: update a task. Vikunja replaces every field, so `full` must be the
    /// whole task object (start from [`Self::task_json`]).
    pub async fn update_task(&self, id: i64, full: &Value) -> Result<Task> {
        self.send(
            Method::POST,
            &format!("tasks/{id}"),
            full,
            &format!("task {id}"),
        )
        .await
    }

    /// `PUT /tasks/{id}/labels`: put a label on a task.
    pub async fn add_label(&self, task_id: i64, label_id: i64) -> Result<()> {
        let _: Value = self
            .send(
                Method::PUT,
                &format!("tasks/{task_id}/labels"),
                &json!({ "label_id": label_id }),
                &format!("task {task_id}"),
            )
            .await?;
        Ok(())
    }

    /// `DELETE /tasks/{id}/labels/{label}`: take a label off a task.
    pub async fn remove_label(&self, task_id: i64, label_id: i64) -> Result<()> {
        let resp = self
            .request(
                Method::DELETE,
                &format!("tasks/{task_id}/labels/{label_id}"),
            )?
            .send()
            .await?;
        let _: Value = decode(resp, &format!("label {label_id} on task {task_id}")).await?;
        Ok(())
    }

    /// `GET /tasks/{id}/comments`.
    pub async fn comments(&self, task_id: i64) -> Result<Vec<Comment>> {
        let list: Option<Vec<Comment>> = self
            .get(
                &format!("tasks/{task_id}/comments"),
                &format!("task {task_id}"),
            )
            .await?;
        Ok(list.unwrap_or_default())
    }

    /// `PUT /tasks/{id}/comments`: add a comment (HTML body).
    pub async fn add_comment(&self, task_id: i64, html: &str) -> Result<Comment> {
        self.send(
            Method::PUT,
            &format!("tasks/{task_id}/comments"),
            &json!({ "comment": html }),
            &format!("task {task_id}"),
        )
        .await
    }
}

/// Turn a response into `T`, or a typed [`Error`] carrying Vikunja's message.
async fn decode<T: DeserializeOwned>(resp: Response, what: &str) -> Result<T> {
    let status = resp.status();
    let body = resp.text().await?;
    if status.is_success() {
        let body = if body.trim().is_empty() {
            "null"
        } else {
            &body
        };
        return serde_json::from_str(body)
            .map_err(|e| Error::Transport(format!("unexpected answer for {what}: {e}")));
    }
    let message = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| v.get("message").and_then(Value::as_str).map(String::from))
        .unwrap_or_else(|| body.chars().take(200).collect());
    Err(match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Error::Unauthorized {
            status: status.as_u16(),
            message,
        },
        StatusCode::NOT_FOUND => Error::NotFound(what.to_string()),
        _ => Error::Status {
            status: status.as_u16(),
            message,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: i64, priority: i64, due: Option<&str>) -> Task {
        serde_json::from_value(json!({
            "id": id, "title": format!("t{id}"), "priority": priority,
            "due_date": due.unwrap_or("0001-01-01T00:00:00Z"), "labels": null
        }))
        .unwrap()
    }

    #[test]
    fn sorted_by_priority_then_due_then_id() {
        let mut ts = vec![
            task(1, 1, None),
            task(2, 3, None),
            task(3, 3, Some("2026-11-03T00:00:00Z")),
            task(4, 3, Some("2026-11-02T00:00:00Z")),
            task(5, 1, None),
        ];
        sort_tasks(&mut ts);
        let ids: Vec<i64> = ts.iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![4, 3, 2, 1, 5]);
    }

    #[test]
    fn filter_expression_and_local_match() {
        let q = TaskQuery {
            project_id: Some(3),
            label_id: Some(7),
            done: Some(false),
            due_before: Some(dates::parse("2026-11-05").unwrap()),
            search: None,
        };
        assert_eq!(
            q.filter_expression().unwrap(),
            "done = false && project = 3 && labels in 7 && due_date < '2026-11-05T00:00:00Z'"
        );
        let mut t = task(1, 0, Some("2026-11-04T00:00:00Z"));
        t.project_id = 3;
        assert!(!q.matches(&t), "label missing");
        t.labels.push(Label {
            id: 7,
            title: "x".into(),
        });
        assert!(q.matches(&t));
        t.due_date = None;
        assert!(!q.matches(&t), "undated tasks are not due before anything");
        assert_eq!(TaskQuery::default().filter_expression(), None);
    }

    #[test]
    fn token_parsing_and_redaction() {
        let t = Token::from_authorization("bearer abc").unwrap();
        assert_eq!(t.bearer(), "Bearer abc");
        assert_eq!(format!("{t:?}"), "Token(***)");
        assert!(Token::from_authorization("Basic dXNlcg==").is_none());
        assert!(Token::from_authorization("Bearer   ").is_none());
        assert_eq!(
            Token::from_authorization("raw").unwrap().bearer(),
            "Bearer raw"
        );
        assert!(Token::new("").is_none());
    }

    #[test]
    fn base_url_normalisation() {
        let v = Vikunja::new("https://tasks.example.com/api/v1/").unwrap();
        assert_eq!(v.base_url().as_str(), "https://tasks.example.com/");
        assert_eq!(
            v.url("info").unwrap().as_str(),
            "https://tasks.example.com/api/v1/info"
        );
        assert_eq!(v.task_url(9), "https://tasks.example.com/tasks/9");
        let sub = Vikunja::new("http://host:3456/vikunja").unwrap();
        assert_eq!(
            sub.url("user").unwrap().as_str(),
            "http://host:3456/vikunja/api/v1/user"
        );
        assert!(Vikunja::new("ftp://x").is_err());

        // A caller-named upstream: https anywhere, http only on loopback.
        let up = Upstreams::fixed(v.clone());
        assert_eq!(
            up.resolve(Some("https://other.example.org/api/v1"))
                .unwrap()
                .base_url()
                .as_str(),
            "https://other.example.org/"
        );
        for ok in [
            "http://localhost:3456",
            "http://127.0.0.1:3456",
            "http://127.9.9.9",
            "http://[::1]:3456",
        ] {
            assert!(up.resolve(Some(ok)).is_ok(), "{ok}");
        }
        for bad in [
            "http://tasks.example.com",
            "http://10.0.0.5:3456",
            "http://localhost.example.com",
            "ftp://localhost",
            "not a url",
        ] {
            assert!(up.resolve(Some(bad)).is_err(), "{bad}");
        }
        assert!(matches!(
            up.resolve(Some("http://tasks.example.com")),
            Err(Error::InsecureUpstream(_))
        ));
        assert_eq!(
            up.resolve(None).unwrap().base_url().as_str(),
            "https://tasks.example.com/"
        );
        assert_eq!(
            up.resolve(Some("  ")).unwrap().base_url().as_str(),
            "https://tasks.example.com/",
            "an empty header falls back to the default"
        );
        let none = Upstreams::new(None).unwrap();
        assert!(matches!(none.resolve(None), Err(Error::NoUpstream)));
        assert!(Vikunja::new("not a url").is_err());
    }
}
