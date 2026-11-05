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

fn sample_pdf() -> Vec<u8> {
    br#"%PDF-1.1
1 0 obj<< /Type /Catalog /Pages 2 0 R >>endobj
2 0 obj<< /Type /Pages /Kids [3 0 R] /Count 1 >>endobj
3 0 obj<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>endobj
4 0 obj<< /Title (Shelf PDF Fixture) >>endobj
trailer<< /Root 1 0 R /Info 4 0 R >>
%%EOF
"#
    .to_vec()
}

fn sample_pdf_without_title() -> Vec<u8> {
    br#"%PDF-1.1
1 0 obj<< /Type /Catalog /Pages 2 0 R >>endobj
2 0 obj<< /Type /Pages /Kids [3 0 R] /Count 1 >>endobj
3 0 obj<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>endobj
trailer<< /Root 1 0 R >>
%%EOF
"#
    .to_vec()
}

/// Minimal 1×1 PNG used as CBZ page bytes.
fn tiny_png() -> &'static [u8] {
    &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8,
        0xCF, 0xC0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x00, 0x05, 0xFE, 0xD4, 0xEF, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ]
}

fn sample_cbz() -> Vec<u8> {
    use std::io::{Cursor, Write};
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for name in ["002.png", "001.png"] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(tiny_png()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn sample_cbr() -> Vec<u8> {
    let mut builder = rars::Builder::new(rars::ArchiveVersion::Rar50);
    builder
        .add_bytes(b"002.png".to_vec(), tiny_png().to_vec(), None, None)
        .unwrap();
    builder
        .add_bytes(b"001.png".to_vec(), tiny_png().to_vec(), None, None)
        .unwrap();
    builder.to_bytes().unwrap()
}

/// Minimal single-page DjVu from `cjb2` (1×1 bitmap).
fn sample_djvu() -> Vec<u8> {
    vec![
        0x41, 0x54, 0x26, 0x54, 0x46, 0x4F, 0x52, 0x4D, 0x00, 0x00, 0x00, 0x22, 0x44, 0x4A, 0x56,
        0x55, 0x49, 0x4E, 0x46, 0x4F, 0x00, 0x00, 0x00, 0x0A, 0x00, 0x01, 0x00, 0x01, 0x18, 0x00,
        0x64, 0x00, 0x16, 0x01, 0x53, 0x6A, 0x62, 0x7A, 0x00, 0x00, 0x00, 0x04, 0xBC, 0x73, 0x1B,
        0xD7,
    ]
}

async fn upload(app: &Router, bytes: Vec<u8>, token: Option<&str>) -> (StatusCode, Value) {
    upload_named(app, bytes, token, None).await
}

async fn upload_named(
    app: &Router,
    bytes: Vec<u8>,
    token: Option<&str>,
    filename: Option<&str>,
) -> (StatusCode, Value) {
    let content_type = if bytes.starts_with(b"%PDF") {
        "application/pdf"
    } else if bytes.starts_with(b"Rar!") || bytes.starts_with(b"AT&TFORM") {
        "application/octet-stream"
    } else if bytes.starts_with(b"PK") && !bytes.windows(19).any(|w| w == b"application/epub+zi") {
        // CBZ (or other ZIP) — EPUB fixtures always embed the mimetype string.
        "application/vnd.comicbook+zip"
    } else {
        "application/epub+zip"
    };
    let mut request = Request::builder()
        .method("POST")
        .uri("/api/v1/books/import")
        .header("content-type", content_type);
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    if let Some(filename) = filename {
        request = request.header("x-shelf-filename", filename);
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
    let csp = reader.headers()["content-security-policy"]
        .to_str()
        .unwrap();
    assert!(csp.contains("frame-src blob:"));
    assert!(csp.contains("wasm-unsafe-eval"));
    let stats = call(&restarted, "GET", "/api/v1/stats", Value::Null, Some(token))
        .await
        .1;
    assert_eq!(stats["reading"], 1);
    assert_eq!(stats["pages_read"], 0);
    assert_eq!(stats["average_progress"], 0.5);
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

#[tokio::test]
async fn pdf_import_serve_and_page_location() {
    let temp = tempfile::tempdir().unwrap();
    let app = shelf_server::app(temp.path().join("pdf.db")).unwrap();
    let credentials = json!({"username":"reader","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let (status, book) = upload(&app, sample_pdf(), Some(&token)).await;
    assert_eq!(status, StatusCode::CREATED, "{book}");
    let id = book["id"].as_i64().unwrap();
    let catalogue = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
        .await
        .1;
    assert_eq!(catalogue[0]["format"], "pdf");
    assert_eq!(catalogue[0]["title"], "Shelf PDF Fixture");
    assert_eq!(catalogue[0]["total_pages"], Value::Null);
    let location = format!("/api/v1/books/{id}/location");
    assert_eq!(
        call(
            &app,
            "POST",
            &location,
            json!({"position":"pdfpage(0)","percentage":0.1}),
            Some(&token)
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
            json!({"position":"pdfpage(3)","percentage":0.25}),
            Some(&token)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let catalogue = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
        .await
        .1;
    assert_eq!(catalogue[0]["position"], "pdfpage(3)");
    assert_eq!(catalogue[0]["percentage"], 0.25);
    let file = format!("/api/v1/books/{id}/file");
    let response = app
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
    assert_eq!(response.headers()["content-type"], "application/pdf");
    assert_eq!(
        to_bytes(response.into_body(), 1024).await.unwrap(),
        sample_pdf()
    );
    let duplicate = upload(&app, sample_pdf(), Some(&token)).await;
    assert_eq!(duplicate.0, StatusCode::OK);
    assert_eq!(duplicate.1["duplicate"], true);
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/books/{id}/finished"),
            Value::Null,
            Some(&token)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let catalogue = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
        .await
        .1;
    assert_eq!(catalogue[0]["percentage"], 1.0);
    assert_eq!(catalogue[0]["reading_status"], "finished");
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/books/{id}/unfinished"),
            Value::Null,
            Some(&token)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let catalogue = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
        .await
        .1;
    assert_eq!(catalogue[0]["percentage"], 0.0);
    assert_eq!(catalogue[0]["reading_status"], "unfinished");
    assert_eq!(catalogue[0]["position"], "pdfpage(3)");
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/books/{id}/finished"),
            Value::Null,
            Some(&token)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/books/{id}/rereading"),
            Value::Null,
            Some(&token)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let catalogue = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
        .await
        .1;
    assert_eq!(catalogue[0]["reading_status"], "rereading");
    assert_eq!(catalogue[0]["percentage"], 0.0);
    let (status, named) = upload_named(
        &app,
        sample_pdf_without_title(),
        Some(&token),
        Some("path/My Summer Notes.pdf"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{named}");
    let named_id = named["id"].as_i64().unwrap();
    let catalogue = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
        .await
        .1;
    let named_book = catalogue
        .as_array()
        .unwrap()
        .iter()
        .find(|book| book["id"] == named_id)
        .unwrap();
    assert_eq!(named_book["title"], "My Summer Notes");
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/books/{named_id}"),
            Value::Null,
            Some(&token)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/books/{named_id}/file"),
            Value::Null,
            Some(&token)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/books/{named_id}"),
            Value::Null,
            Some(&token)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn cbz_import_page_images_and_location() {
    let temp = tempfile::tempdir().unwrap();
    let app = shelf_server::app(temp.path().join("cbz.db")).unwrap();
    let credentials = json!({"username":"reader","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let (status, book) = upload_named(
        &app,
        sample_cbz(),
        Some(&token),
        Some("Night_City_Issue_1.cbz"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{book}");
    let id = book["id"].as_i64().unwrap();
    let catalogue = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
        .await
        .1;
    assert_eq!(catalogue[0]["format"], "cbz");
    assert_eq!(catalogue[0]["title"], "Night City Issue 1");
    assert_eq!(catalogue[0]["has_cover"], true);
    let pages = call(
        &app,
        "GET",
        &format!("/api/v1/books/{id}/pages"),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(pages.0, StatusCode::OK);
    assert_eq!(pages.1["pageCount"], 2);
    let page0 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/books/{id}/pages/0"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(page0.status(), StatusCode::OK);
    assert_eq!(page0.headers()["content-type"], "image/png");
    assert_eq!(to_bytes(page0.into_body(), 1024).await.unwrap(), tiny_png());
    // Lexicographic order: 001.png before 002.png.
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/books/{id}/location"),
            json!({"position":"cbzpage(2)","percentage":1.0}),
            Some(&token)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let catalogue = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
        .await
        .1;
    assert_eq!(catalogue[0]["position"], "cbzpage(2)");
    assert_eq!(catalogue[0]["percentage"], 1.0);
    assert_eq!(catalogue[0]["reading_status"], "finished");
}

#[tokio::test]
async fn cbr_and_djvu_import_normalize_to_comic_pages() {
    let temp = tempfile::tempdir().unwrap();
    let app = shelf_server::app(temp.path().join("cbr-djvu.db")).unwrap();
    let credentials = json!({"username":"reader","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_owned();

    let (status, book) = upload_named(
        &app,
        sample_cbr(),
        Some(&token),
        Some("Night_City_Issue_2.cbr"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{book}");
    let cbr_id = book["id"].as_i64().unwrap();
    let catalogue = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
        .await
        .1;
    assert_eq!(catalogue[0]["format"], "cbr");
    assert_eq!(catalogue[0]["title"], "Night City Issue 2");
    let pages = call(
        &app,
        "GET",
        &format!("/api/v1/books/{cbr_id}/pages"),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(pages.0, StatusCode::OK, "{}", pages.1);
    assert_eq!(pages.1["pageCount"], 2);
    let file = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/books/{cbr_id}/file"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(file.status(), StatusCode::OK);
    assert_eq!(
        file.headers()["content-type"],
        "application/vnd.comicbook+zip"
    );
    assert!(to_bytes(file.into_body(), 1024 * 1024)
        .await
        .unwrap()
        .starts_with(b"PK"));

    let (status, book) = upload_named(
        &app,
        sample_djvu(),
        Some(&token),
        Some("Scanned_Notes.djvu"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{book}");
    let djvu_id = book["id"].as_i64().unwrap();
    let catalogue = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
        .await
        .1;
    let djvu = catalogue
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == djvu_id)
        .unwrap();
    assert_eq!(djvu["format"], "djvu");
    assert_eq!(djvu["title"], "Scanned Notes");
    let pages = call(
        &app,
        "GET",
        &format!("/api/v1/books/{djvu_id}/pages"),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(pages.0, StatusCode::OK, "{}", pages.1);
    assert_eq!(pages.1["pageCount"], 1);
    let page0 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/books/{djvu_id}/pages/0"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(page0.status(), StatusCode::OK);
    assert_eq!(page0.headers()["content-type"], "image/png");
    assert!(
        to_bytes(page0.into_body(), 1024 * 1024)
            .await
            .unwrap()
            .len()
            > 8
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
    assert_eq!(stats["books"], 1);
    assert_eq!(stats["pages_read"], 163);
    assert_eq!(stats["finished"], 0);
    assert_eq!(stats["reading"], 1);
    assert!((stats["average_progress"].as_f64().unwrap() - 163.0 / 245.0).abs() < 1e-9);
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
async fn authenticated_requests_slide_session_idle_window() {
    use sha2::{Digest, Sha256};
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("slide.db");
    let app = shelf_server::app(path.clone()).unwrap();
    let credentials = json!({"username":"reader","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let hash = format!("{:x}", Sha256::digest(token.as_bytes()));
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute(
        "UPDATE sessions SET expires_at = unixepoch() + 60 WHERE token_hash = ?",
        [&hash],
    )
    .unwrap();
    let before: i64 = db
        .query_row(
            "SELECT expires_at FROM sessions WHERE token_hash = ?",
            [&hash],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
            .await
            .0,
        StatusCode::OK
    );
    let after: i64 = db
        .query_row(
            "SELECT expires_at FROM sessions WHERE token_hash = ?",
            [&hash],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        after > before + 60,
        "idle window should renew far past the near-expiry stamp ({after} vs {before})"
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

#[tokio::test]
async fn bookmarks_annotations_and_reader_prefs_persist() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("reader-data.db");
    let app = shelf_server::app(path.clone()).unwrap();
    let credentials = json!({"username":"reader","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_string();
    let book_id = call(
        &app,
        "POST",
        "/api/v1/books",
        json!({"title":"Notes","author":"Shelf","total_pages":10}),
        Some(&token),
    )
    .await
    .1["id"]
        .as_i64()
        .unwrap();
    let bookmarks = format!("/api/v1/books/{book_id}/bookmarks");
    let annotations = format!("/api/v1/books/{book_id}/annotations");
    let prefs = format!("/api/v1/reader/preferences/{book_id}");

    assert_eq!(
        call(&app, "GET", &bookmarks, Value::Null, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, "GET", &bookmarks, Value::Null, Some(&token))
            .await
            .1,
        json!([])
    );

    let (status, mark) = call(
        &app,
        "POST",
        &bookmarks,
        json!({"cfi":"epubcfi(/6/2!/4)","chapterTitle":"Ch 1","note":"here"}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{mark}");
    assert_eq!(mark["title"], "Ch 1");
    assert_eq!(mark["bookId"], book_id);
    let mark_id = mark["id"].as_i64().unwrap();
    assert_eq!(
        call(
            &app,
            "PATCH",
            &format!("{bookmarks}/{mark_id}"),
            json!({"title":"Chapter one","note":null}),
            Some(&token),
        )
        .await
        .1["title"],
        "Chapter one"
    );

    let (status, note) = call(
        &app,
        "POST",
        &annotations,
        json!({
            "cfi":"epubcfi(/6/4!/4)",
            "text":"highlighted",
            "color":"#22C55E",
            "style":"underline",
            "note":"why",
            "chapterTitle":"Ch 2"
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{note}");
    let annotation_id = note["id"].as_i64().unwrap();
    assert_eq!(note["jumpFileId"], book_id);
    assert_eq!(note["pdf"], Value::Null);
    let (status, pdf_note) = call(
        &app,
        "POST",
        &annotations,
        json!({
            "bookFileId": book_id,
            "text":"pdf highlight",
            "color":"#4ADE80",
            "style":"highlight",
            "note":null,
            "pdf":{
                "page":2,
                "rect":{"x":10.0,"y":20.0,"width":100.0,"height":12.0},
                "rects":[
                    {"x":10.0,"y":20.0,"width":100.0,"height":12.0},
                    {"x":10.0,"y":34.0,"width":40.0,"height":12.0}
                ]
            }
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{pdf_note}");
    assert_eq!(pdf_note["pdf"]["page"], 2);
    assert_eq!(pdf_note["pageno"], 2);
    assert_eq!(pdf_note["pdf"]["rects"].as_array().unwrap().len(), 2);
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("{annotations}?page=1"),
            Value::Null,
            Some(&token),
        )
        .await
        .1["total"],
        2
    );
    let listed = call(
        &app,
        "GET",
        &format!("{annotations}?page=1"),
        Value::Null,
        Some(&token),
    )
    .await
    .1;
    let pdf_listed = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == pdf_note["id"])
        .unwrap();
    assert_eq!(pdf_listed["pdf"]["page"], 2);
    assert_eq!(
        call(
            &app,
            "POST",
            &annotations,
            json!({
                "text":"bad",
                "color":"#4ADE80",
                "style":"highlight",
                "pdf":{"page":-1,"rect":{"x":0,"y":0,"width":1,"height":1},"rects":[{"x":0,"y":0,"width":1,"height":1}]}
            }),
            Some(&token),
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );

    assert_eq!(
        call(&app, "GET", &prefs, Value::Null, Some(&token)).await.1,
        Value::Null
    );
    assert_eq!(
        call(
            &app,
            "PATCH",
            &prefs,
            json!({"theme":{"scheme":"light"},"fontSize":18}),
            Some(&token),
        )
        .await
        .1,
        json!({"theme":{"scheme":"light"},"fontSize":18})
    );
    assert_eq!(
        call(
            &app,
            "PATCH",
            &prefs,
            json!({"theme":{"scheme":"sepia"}}),
            Some(&token),
        )
        .await
        .1,
        json!({"theme":{"scheme":"sepia"},"fontSize":18})
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/reader/defaults",
            json!({"epub":{"fontSize":20}}),
            Some(&token),
        )
        .await
        .1,
        json!({"epub":{"fontSize":20}})
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/reader/defaults/epub",
            Value::Null,
            Some(&token),
        )
        .await
        .1,
        json!({"fontSize":20})
    );

    let restarted = shelf_server::app(path).unwrap();
    let listed = call(&restarted, "GET", &bookmarks, Value::Null, Some(&token))
        .await
        .1;
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["title"], "Chapter one");
    assert_eq!(listed[0]["note"], Value::Null);
    let notes = call(&restarted, "GET", &annotations, Value::Null, Some(&token))
        .await
        .1;
    assert_eq!(notes[0]["text"], "highlighted");
    assert_eq!(
        call(&restarted, "GET", &prefs, Value::Null, Some(&token))
            .await
            .1["fontSize"],
        18
    );
    assert_eq!(
        call(
            &restarted,
            "DELETE",
            &format!("{bookmarks}/{mark_id}"),
            Value::Null,
            Some(&token),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &restarted,
            "DELETE",
            &format!("{annotations}/{annotation_id}"),
            Value::Null,
            Some(&token),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&restarted, "GET", &bookmarks, Value::Null, Some(&token))
            .await
            .1,
        json!([])
    );
    assert_eq!(
        call(
            &restarted,
            "GET",
            &format!("/api/v1/books/{}/bookmarks", book_id + 99),
            Value::Null,
            Some(&token),
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}
