use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use tower::ServiceExt;

fn sample_epub(unsafe_path: bool) -> Vec<u8> {
    use std::io::{Cursor, Write};
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, content) in [
        ("mimetype", "application/epub+zip"),
        ("META-INF/container.xml", r#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><rootfiles><rootfile full-path="package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#),
        ("package.opf", r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">shelf-test</dc:identifier><dc:title>A test chapter</dc:title><dc:creator>Shelf tests</dc:creator><dc:language>en</dc:language></metadata><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/></manifest><spine><itemref idref="chapter"/></spine></package>"#),
        ("chapter.xhtml", "<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>Chapter</title></head><body><h1>A test chapter</h1><p>Original synthetic test text.</p></body></html>"),
        ("nav.xhtml", "<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\"><head><title>Contents</title></head><body><nav epub:type=\"toc\"><ol><li><a href=\"chapter.xhtml\">Chapter</a></li></ol></nav></body></html>"),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(content.as_bytes()).unwrap();
    }
    if unsafe_path {
        zip.start_file("../escape.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"Must never be extracted").unwrap();
    }
    zip.finish().unwrap().into_inner()
}

async fn upload(app: &Router, bytes: Vec<u8>, token: Option<&str>) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method("POST")
        .uri("/api/v1/books/import")
        .header("content-type", "application/epub+zip");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(bytes)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 65536).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

#[tokio::test]
async fn accepts_books_above_fifty_mib_and_rejects_extreme_compression() {
    use std::io::{Cursor, Write};
    fn padded_epub(size: usize, method: zip::CompressionMethod) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new_append(Cursor::new(sample_epub(false))).unwrap();
        zip.start_file(
            "padding.bin",
            zip::write::SimpleFileOptions::default().compression_method(method),
        )
        .unwrap();
        let chunk = [0u8; 8192];
        for _ in 0..size.div_ceil(chunk.len()) {
            zip.write_all(&chunk).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }
    let temp = tempfile::tempdir().unwrap();
    let app = shelf_server::app(temp.path().join("large.db")).unwrap();
    let credentials = json!({"username":"reader","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let login = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1;
    let token = login["token"].as_str().unwrap();
    let payload = padded_epub(51 * 1024 * 1024, zip::CompressionMethod::Stored);
    assert!(payload.len() > 50 * 1024 * 1024);
    let (status, result) = upload(&app, payload, Some(token)).await;
    assert_eq!(status, StatusCode::CREATED, "{result}");
    let (status, result) = upload(
        &app,
        padded_epub(2 * 1024 * 1024, zip::CompressionMethod::Deflated),
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        result["error"],
        "This EPUB contains an unusually compressed resource."
    );
}

#[tokio::test]
async fn epub_import_duplicate_access_and_saved_position_survive_restart() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("epub.db");
    let app = shelf_server::app(path.clone()).unwrap();
    assert_eq!(
        upload(&app, sample_epub(false), None).await.0,
        StatusCode::UNAUTHORIZED
    );
    let credentials = json!({"username":"reader","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let login = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1;
    let token = login["token"].as_str().unwrap();
    assert_eq!(
        upload(&app, b"invalid".to_vec(), Some(token)).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        upload(&app, sample_epub(true), Some(token)).await.0,
        StatusCode::BAD_REQUEST
    );
    let (status, book) = upload(&app, sample_epub(false), Some(token)).await;
    assert_eq!(status, StatusCode::CREATED, "{book}");
    let id = book["id"].as_i64().unwrap();
    let duplicate = upload(&app, sample_epub(false), Some(token)).await;
    assert_eq!(duplicate.0, StatusCode::OK);
    assert_eq!(duplicate.1["id"], id);
    assert_eq!(duplicate.1["duplicate"], true);
    let location = format!("/api/v1/books/{id}/location");
    assert_eq!(
        call(
            &app,
            "POST",
            &location,
            json!({"position":"bad","percentage":0.5}),
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
            &location,
            json!({"position":"epubcfi(/6/2!/4/2)","percentage":1.1}),
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
            &location,
            json!({"position":"epubcfi(/6/2!/4/2)","percentage":0.5}),
            Some(token)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let restarted = shelf_server::app(path.clone()).unwrap();
    let catalogue = call(&restarted, "GET", "/api/v1/books", Value::Null, Some(token))
        .await
        .1;
    let fixture: Value =
        serde_json::from_str(include_str!("../../protocol/fixtures/epub-book.json")).unwrap();
    assert_eq!(catalogue[0], fixture);
    assert_eq!(catalogue[0]["total_pages"], Value::Null);
    assert_eq!(catalogue[0]["percentage"], 0.5);
    assert_eq!(catalogue[0]["position"], "epubcfi(/6/2!/4/2)");
    let file = format!("/api/v1/books/{id}/file");
    assert_eq!(
        call(&restarted, "GET", &file, Value::Null, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    let response = restarted
        .clone()
        .oneshot(
            Request::builder()
                .uri(&file)
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.headers()["content-type"], "application/epub+zip");
    assert_eq!(
        to_bytes(response.into_body(), 1024 * 1024).await.unwrap(),
        sample_epub(false)
    );
    let reader = restarted
        .clone()
        .oneshot(
            Request::builder()
                .uri("/reader?id=1")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(reader.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .contains("HttpOnly"));
    assert!(reader.headers()["content-security-policy"]
        .to_str()
        .unwrap()
        .contains("frame-src blob:"));
    let stats = call(&restarted, "GET", "/api/v1/stats", Value::Null, Some(token))
        .await
        .1;
    assert_eq!(stats["reading"], 1);
    assert_eq!(stats["pages_read"], 0);
    use sha2::{Digest, Sha256};
    let db = rusqlite::Connection::open(path).unwrap();
    db.execute(
        "INSERT INTO users(id,username,password_hash) VALUES (2,'other','synthetic-test-only')",
        [],
    )
    .unwrap();
    let other = "synthetic-other-session";
    db.execute(
        "INSERT INTO sessions(token_hash,user_id,expires_at) VALUES (?,2,unixepoch()+600)",
        [format!("{:x}", Sha256::digest(other.as_bytes()))],
    )
    .unwrap();
    assert_eq!(
        call(&restarted, "GET", &file, Value::Null, Some(other))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &restarted,
            "POST",
            &location,
            json!({"position":"epubcfi(/6/2!/4/2)","percentage":0.9}),
            Some(other)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

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
