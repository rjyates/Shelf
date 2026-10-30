use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use tower::ServiceExt;

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    body: Value,
    token: Option<&str>,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn account_catalogue_progress_and_restart() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("test.db");
    let app = shelf_server::app(path.clone()).unwrap();
    assert_eq!(
        call(&app, "GET", "/api/v1/books", Value::Null, None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let credentials = json!({"username":"reader","password":"test-password-long-enough"});
    assert_eq!(
        call(&app, "POST", "/api/v1/setup", credentials.clone(), None)
            .await
            .0,
        StatusCode::CREATED
    );
    assert_eq!(
        call(&app, "POST", "/api/v1/setup", credentials.clone(), None)
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/setup", Value::Null, None)
            .await
            .1["setup_required"],
        false
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/login",
            json!({"username":"reader","password":"wrong"}),
            None
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, session) = call(&app, "POST", "/api/v1/login", credentials, None).await;
    assert_eq!(status, StatusCode::OK);
    let token = session["token"].as_str().unwrap();
    let (status, book) = call(
        &app,
        "POST",
        "/api/v1/books",
        json!({"title":"Piranesi","author":"Susanna Clarke","total_pages":245}),
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let progress = format!("/api/v1/books/{}/progress", book["id"]);
    assert_eq!(
        call(
            &app,
            "POST",
            &progress,
            json!({"pages_read":246}),
            Some(token)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &progress,
            json!({"pages_read":163}),
            Some(token)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let restarted = shelf_server::app(path).unwrap();
    let fixture: Value =
        serde_json::from_str(include_str!("../../protocol/fixtures/book.json")).unwrap();
    assert_eq!(
        call(&restarted, "GET", "/api/v1/books", Value::Null, Some(token))
            .await
            .1[0],
        fixture
    );
    assert_eq!(
        call(&restarted, "GET", "/api/v1/books", Value::Null, Some(token))
            .await
            .1[0]["pages_read"],
        163
    );
    let stats = call(&restarted, "GET", "/api/v1/stats", Value::Null, Some(token))
        .await
        .1;
    assert_eq!(
        stats,
        json!({"books":1,"pages_read":163,"finished":0,"reading":1})
    );
    assert_eq!(
        call(
            &restarted,
            "POST",
            "/api/v1/logout",
            Value::Null,
            Some(token)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&restarted, "GET", "/api/v1/books", Value::Null, Some(token))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn validates_inputs_and_keeps_setup_atomic() {
    let temp = tempfile::tempdir().unwrap();
    let app = shelf_server::app(temp.path().join("test.db")).unwrap();
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/setup",
            json!({"username":"reader","password":"short"}),
            None
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let one = call(
        &app,
        "POST",
        "/api/v1/setup",
        json!({"username":"reader1","password":"valid-password-123"}),
        None,
    );
    let two = call(
        &app,
        "POST",
        "/api/v1/setup",
        json!({"username":"reader2","password":"valid-password-123"}),
        None,
    );
    let (a, b) = tokio::join!(one, two);
    assert!(
        (a.0 == StatusCode::CREATED && b.0 == StatusCode::CONFLICT)
            || (a.0 == StatusCode::CONFLICT && b.0 == StatusCode::CREATED)
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/books",
            json!({"title":" ","author":"","total_pages":0}),
            None
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
}
