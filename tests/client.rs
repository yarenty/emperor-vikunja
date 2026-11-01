//! The REST client against the fake Vikunja: real HTTP, recorded requests.

mod common;

use common::{Fake, task};
use emperor_vikunja::client::{Error, TaskQuery, Token, Vikunja};
use emperor_vikunja::dates;

fn client(fake: &Fake) -> Vikunja {
    Vikunja::new(&fake.base).unwrap()
}

#[tokio::test]
async fn token_travels_as_bearer_header() {
    let fake = Fake::seeded().await;
    let token = Token::new(fake.token.clone()).unwrap();
    let user = client(&fake).session(&token).user().await.unwrap();
    assert_eq!(user.username, "alice");
    let reqs = fake.requests_to("GET", "/api/v1/user");
    assert_eq!(reqs.len(), 1);
    assert_eq!(
        reqs[0].auth.as_deref(),
        Some(format!("Bearer {}", fake.token).as_str())
    );
}

#[tokio::test]
async fn info_needs_no_token() {
    let fake = Fake::seeded().await;
    let info = client(&fake).info().await.unwrap();
    assert_eq!(info.version, "v2.7.1-fake");
    assert_eq!(fake.requests_to("GET", "/api/v1/info")[0].auth, None);
}

#[tokio::test]
async fn wrong_token_is_unauthorized_with_vikunjas_message() {
    let fake = Fake::seeded().await;
    let wrong = Token::new("not-the-token").unwrap();
    let err = client(&fake).session(&wrong).user().await.unwrap_err();
    assert!(
        matches!(err, Error::Unauthorized { status: 401, .. }),
        "{err:?}"
    );
    assert!(err.to_string().contains("invalid token"), "{err}");
    assert!(
        !err.to_string().contains("not-the-token"),
        "token must not leak: {err}"
    );
}

#[tokio::test]
async fn missing_task_is_not_found() {
    let fake = Fake::seeded().await;
    let token = Token::new(fake.token.clone()).unwrap();
    let err = client(&fake).session(&token).task(999).await.unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "{err:?}");
}

#[tokio::test]
async fn filters_become_query_parameters() {
    let fake = Fake::seeded().await;
    let token = Token::new(fake.token.clone()).unwrap();
    let query = TaskQuery {
        project_id: Some(2),
        label_id: Some(1),
        done: Some(false),
        due_before: Some(dates::parse("2026-11-05").unwrap()),
        search: Some("release".into()),
    };
    let tasks = client(&fake).session(&token).tasks(&query).await.unwrap();
    // The fake ignores filters; the client applies them again, so only task 1 is left.
    assert_eq!(tasks.iter().map(|t| t.id).collect::<Vec<_>>(), vec![1]);

    let req = &fake.requests_to("GET", "/api/v1/tasks")[0];
    assert_eq!(
        req.param("filter").as_deref(),
        Some("done = false && project = 2 && labels in 1 && due_date < '2026-11-05T00:00:00Z'")
    );
    assert_eq!(req.param("filter_timezone").as_deref(), Some("UTC"));
    assert_eq!(req.param("s").as_deref(), Some("release"));
    let sorts: Vec<String> = req
        .query_pairs()
        .into_iter()
        .filter(|(k, _)| k == "sort_by[]")
        .map(|(_, v)| v)
        .collect();
    assert_eq!(sorts, vec!["priority", "due_date", "id"]);
    assert_eq!(req.param("page").as_deref(), Some("1"));
}

#[tokio::test]
async fn every_page_is_read_and_results_are_sorted() {
    let fake = Fake::start().await;
    {
        let mut d = fake.data.lock().unwrap();
        d.projects = vec![serde_json::json!({ "id": 1, "title": "Inbox" })];
        d.tasks = (1..=120)
            .map(|i| {
                let mut t = task(i, &format!("task {i}"), 1);
                t["priority"] = serde_json::json!(i % 5);
                t
            })
            .collect();
    }
    let token = Token::new(fake.token.clone()).unwrap();
    let tasks = client(&fake)
        .session(&token)
        .tasks(&TaskQuery::default())
        .await
        .unwrap();
    assert_eq!(tasks.len(), 120);
    assert_eq!(
        fake.requests_to("GET", "/api/v1/tasks").len(),
        3,
        "50 + 50 + 20"
    );
    assert!(tasks.windows(2).all(|w| w[0].priority >= w[1].priority));
    assert_eq!(tasks[0].id, 4, "highest priority, lowest id first");
}

#[tokio::test]
async fn new_labels_are_created_existing_ones_reused() {
    let fake = Fake::seeded().await;
    let token = Token::new(fake.token.clone()).unwrap();
    let labels = client(&fake)
        .session(&token)
        .ensure_labels(&["NOW".into(), "release".into(), "release".into(), " ".into()])
        .await
        .unwrap();
    let titles: Vec<&str> = labels.iter().map(|l| l.title.as_str()).collect();
    assert_eq!(titles, vec!["now", "release"]);
    let created = fake.requests_to("PUT", "/api/v1/labels");
    assert_eq!(created.len(), 1, "only the missing label is created");
    assert_eq!(created[0].body.as_ref().unwrap()["title"], "release");
}

#[tokio::test]
async fn unreachable_instance_is_a_transport_error() {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let err = Vikunja::new(&format!("http://127.0.0.1:{port}"))
        .unwrap()
        .info()
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Transport(_)), "{err:?}");
}
