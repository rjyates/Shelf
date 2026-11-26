use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use tower::ServiceExt;

fn sample_epub_with_media_overlay() -> Vec<u8> {
    use std::io::{Cursor, Write};
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, content) in [
        ("mimetype", "application/epub+zip".as_bytes()),
        ("META-INF/container.xml", br#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><rootfiles><rootfile full-path="package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#),
        ("package.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">shelf-mo</dc:identifier><dc:title>Read aloud</dc:title><dc:creator>Shelf tests</dc:creator><dc:language>en</dc:language></metadata><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml" media-overlay="mo1"/><item id="mo1" href="chapter.smil" media-type="application/smil+xml"/><item id="audio" href="audio.mp3" media-type="audio/mpeg"/><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/></manifest><spine><itemref idref="chapter"/></spine></package>"#),
        ("chapter.xhtml", br#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Chapter</title></head><body><p id="s1">Hello there.</p><p id="s2">Second sentence.</p></body></html>"#),
        ("chapter.smil", br#"<?xml version="1.0"?><smil xmlns="http://www.w3.org/ns/SMIL" version="3.0"><body><par id="p1"><text src="chapter.xhtml#s1"/><audio src="audio.mp3" clipBegin="0s" clipEnd="1.5s"/></par><par id="p2"><text src="chapter.xhtml#s2"/><audio src="audio.mp3" clipBegin="1.5s" clipEnd="3s"/></par></body></smil>"#),
        ("audio.mp3", b"ID3fake-audio"),
        ("nav.xhtml", br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><head><title>Contents</title></head><body><nav epub:type="toc"><ol><li><a href="chapter.xhtml">Chapter</a></li></ol></nav></body></html>"#),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(content).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

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

fn sample_cb7() -> Vec<u8> {
    use std::io::Cursor;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("002.png"), tiny_png()).unwrap();
    std::fs::write(dir.path().join("001.png"), tiny_png()).unwrap();
    sevenz_rust2::compress(dir.path(), Cursor::new(Vec::new()))
        .unwrap()
        .into_inner()
}

fn sample_mp3() -> Vec<u8> {
    let mut bytes = b"ID3".to_vec();
    bytes.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0]);
    bytes.extend_from_slice(&[0xff, 0xfb, 0x90, 0x00]);
    bytes.extend_from_slice(b"shelf-mp3-fixture");
    bytes
}

/// Minimal AAX-shaped buffer: ftyp + checksum region at offset 653 (audible-tools).
fn sample_aax() -> Vec<u8> {
    let mut bytes = vec![0u8; 700];
    bytes[4..8].copy_from_slice(b"ftyp");
    bytes[8..12].copy_from_slice(b"aavd");
    for i in 0..20 {
        bytes[653 + i] = (i as u8).wrapping_add(0x10);
    }
    bytes
}

fn sample_converted_m4b() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&20u32.to_be_bytes());
    bytes.extend_from_slice(b"ftyp");
    bytes.extend_from_slice(b"M4B ");
    bytes.extend_from_slice(b"shelf-aax-converted");
    bytes
}

fn sample_m4b() -> Vec<u8> {
    let mut bytes = vec![0, 0, 0, 24];
    bytes.extend_from_slice(b"ftyp");
    bytes.extend_from_slice(b"M4B ");
    bytes.extend_from_slice(&[0, 0, 0, 0]);
    bytes.extend_from_slice(b"M4B ");
    bytes.extend_from_slice(b"shelf-m4b-fixture!!!!");
    bytes
}

fn sample_flac() -> Vec<u8> {
    let mut bytes = b"fLaC".to_vec();
    bytes.extend_from_slice(b"shelf-flac-fixture");
    bytes
}

fn sample_audio_zip() -> Vec<u8> {
    use std::io::{Cursor, Write};
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, content) in [("02-b.mp3", b"ID3two" as &[u8]), ("01-a.mp3", b"ID3one")] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(content).unwrap();
    }
    zip.finish().unwrap().into_inner()
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
    } else if bytes.starts_with(b"Rar!")
        || bytes.starts_with(b"AT&TFORM")
        || bytes.starts_with(&[0x37, 0x7a, 0xbc, 0xaf, 0x27, 0x1c])
    {
        "application/octet-stream"
    } else if bytes.starts_with(b"PK") && !bytes.windows(19).any(|w| w == b"application/epub+zi") {
        // CBZ (or other ZIP) — EPUB fixtures always embed the mimetype string.
        "application/vnd.comicbook+zip"
    } else {
        "application/epub+zip"
    };
    // Process-wide IMPORTS semaphore (capacity 2) is shared across parallel #[tokio::test]s.
    for attempt in 0..12 {
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
            .oneshot(request.body(Body::from(bytes.clone())).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), 65536).await.unwrap();
        let json = serde_json::from_slice(&body).unwrap_or(Value::Null);
        if status != StatusCode::TOO_MANY_REQUESTS || attempt == 11 {
            return (status, json);
        }
        tokio::time::sleep(std::time::Duration::from_millis(50 * (attempt + 1) as u64)).await;
    }
    unreachable!()
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
async fn tts_chapter_text_from_epub_spine() {
    let temp = tempfile::tempdir().unwrap();
    let app = shelf_server::app(temp.path().join("tts.db")).unwrap();
    let credentials = json!({"username":"reader","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let (status, book) = upload(&app, sample_epub(false), Some(&token)).await;
    assert_eq!(status, StatusCode::CREATED, "{book}");
    let id = book["id"].as_i64().unwrap();
    let (status, text) = call(
        &app,
        "GET",
        &format!("/api/v1/tts/text/{id}/0"),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(text["chapterIndex"], 0);
    let sentences = text["sentences"].as_array().unwrap();
    assert!(sentences.len() >= 2, "{text}");
    assert_eq!(sentences[0]["text"], "A test chapter");
    assert_eq!(sentences[1]["text"], "Original synthetic test text.");
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/tts/text/{id}/9"),
            Value::Null,
            Some(&token),
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn tts_providers_gate_reader_until_enabled() {
    let temp = tempfile::tempdir().unwrap();
    let app = shelf_server::app(temp.path().join("tts-providers.db")).unwrap();
    let credentials = json!({"username":"reader","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_owned();

    let (status, providers) = call(
        &app,
        "GET",
        "/api/v1/tts/providers",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{providers}");
    assert_eq!(providers, json!([]));

    let (status, created) = call(
        &app,
        "POST",
        "/api/v1/tts/admin/providers",
        json!({
            "name": "Kokoro",
            "baseUrl": "http://127.0.0.1:8880/v1",
            "defaultModel": "kokoro",
            "staticVoices": [{"id":"af_heart","name":"Heart","shortName":"af_heart","language":"English","locale":"en-US","gender":"Female"}]
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_i64().unwrap();

    let (status, providers) = call(
        &app,
        "GET",
        "/api/v1/tts/providers",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{providers}");
    assert_eq!(providers.as_array().unwrap().len(), 1);
    assert_eq!(providers[0]["id"], id.to_string());
    assert_eq!(providers[0]["type"], "openai-compatible");

    let (status, voices) = call(&app, "GET", "/api/v1/tts/voices", Value::Null, Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{voices}");
    assert_eq!(voices[0]["id"], "af_heart");
    assert_eq!(voices[0]["providerId"], id.to_string());

    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/tts/admin/providers/{id}"),
            json!({"enabled": false}),
            Some(&token),
        )
        .await
        .0,
        StatusCode::OK
    );
    let (status, providers) = call(
        &app,
        "GET",
        "/api/v1/tts/providers",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{providers}");
    assert_eq!(providers, json!([]));
}

#[tokio::test]
async fn media_overlay_playlist_from_epub_smil() {
    let temp = tempfile::tempdir().unwrap();
    let app = shelf_server::app(temp.path().join("mo.db")).unwrap();
    let credentials = json!({"username":"reader","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let (status, book) = upload(&app, sample_epub_with_media_overlay(), Some(&token)).await;
    assert_eq!(status, StatusCode::CREATED, "{book}");
    let id = book["id"].as_i64().unwrap();
    let (status, playlist) = call(
        &app,
        "GET",
        &format!("/api/v1/epub/{id}/media-overlay?fileId={id}"),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{playlist}");
    assert_eq!(playlist["bookId"], id);
    assert_eq!(playlist["items"].as_array().unwrap().len(), 2);
    assert_eq!(playlist["items"][0]["textFragment"], "s1");
    assert_eq!(playlist["items"][0]["audioHref"], "audio.mp3");
    assert_eq!(playlist["items"][0]["clipBeginSeconds"], 0.0);
    assert_eq!(playlist["items"][0]["clipEndSeconds"], 1.5);
    assert_eq!(playlist["items"][1]["textFragment"], "s2");
    assert_eq!(playlist["sections"].as_array().unwrap().len(), 1);
    assert_eq!(playlist["durationSeconds"], 3.0);
    assert_eq!(playlist["resources"][0]["href"], "audio.mp3");

    let (status, empty) = call(
        &app,
        "GET",
        &format!(
            "/api/v1/epub/{}/media-overlay",
            upload(&app, sample_epub(false), Some(&token)).await.1["id"]
        ),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{empty}");
    assert_eq!(empty["items"], json!([]));
}

#[tokio::test]
async fn epub_package_info_and_file_stream() {
    let temp = tempfile::tempdir().unwrap();
    let app = shelf_server::app(temp.path().join("stream.db")).unwrap();
    let credentials = json!({"username":"reader","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let (status, book) = upload(&app, sample_epub(false), Some(&token)).await;
    assert_eq!(status, StatusCode::CREATED, "{book}");
    let id = book["id"].as_i64().unwrap();

    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/epub/{id}/info"),
            Value::Null,
            None
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );

    let (status, info) = call(
        &app,
        "GET",
        &format!("/api/v1/epub/{id}/info?fileId={id}"),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{info}");
    assert_eq!(info["containerPath"], "package.opf");
    assert_eq!(info["rootPath"], "");
    assert_eq!(info["metadata"]["title"], "A test chapter");
    assert_eq!(info["spine"][0]["href"], "chapter.xhtml");
    assert_eq!(info["manifest"][0]["href"], "chapter.xhtml");
    assert!(info["manifest"][0]["size"].as_u64().unwrap() > 0);
    assert_eq!(info["toc"]["children"][0]["label"], "Chapter");

    let chapter = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/epub/{id}/file/chapter.xhtml"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(chapter.status(), StatusCode::OK);
    assert_eq!(chapter.headers()["content-type"], "application/xhtml+xml");
    let chapter_bytes = to_bytes(chapter.into_body(), 1024).await.unwrap();
    assert!(String::from_utf8_lossy(&chapter_bytes).contains("Original synthetic test text"));

    let escaped = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/epub/{id}/file/../package.opf"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(escaped.status(), StatusCode::BAD_REQUEST);

    let missing = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/epub/{id}/file/missing.xhtml"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    let (status, _) = upload(&app, sample_pdf(), Some(&token)).await;
    assert_eq!(status, StatusCode::CREATED);
    let pdf_id = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
        .await
        .1
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["format"] == "pdf")
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/epub/{pdf_id}/info"),
            Value::Null,
            Some(&token)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
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
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/books/{id}/finished"),
            json!({"finished_at": "2099-01-01"}),
            Some(&token)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST,
        "future finished_at rejected"
    );
    // Prior today finish remains after unfinished; correct via PATCH (append model).
    let finishes = call(
        &app,
        "GET",
        &format!("/api/v1/books/{id}/finishes"),
        Value::Null,
        Some(&token),
    )
    .await
    .1;
    let finish_rows = finishes["finishes"].as_array().expect("finishes");
    assert_eq!(
        finish_rows.len(),
        1,
        "one finish after unfinished keeps history"
    );
    let event_id = finish_rows[0]["id"].as_i64().expect("finish id");
    assert_eq!(
        call(
            &app,
            "PATCH",
            &format!("/api/v1/books/{id}/finishes/{event_id}"),
            json!({"finished_at": "2020-06-15"}),
            Some(&token)
        )
        .await
        .0,
        StatusCode::OK,
        "patch finish day accepted"
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/books/{id}/finished"),
            json!({"finished_at": "2020-06-15"}),
            Some(&token)
        )
        .await
        .0,
        StatusCode::NO_CONTENT,
        "re-mark finished appends while unfinished"
    );
    let finishes = call(
        &app,
        "GET",
        &format!("/api/v1/books/{id}/finishes"),
        Value::Null,
        Some(&token),
    )
    .await
    .1;
    assert_eq!(
        finishes["finishes"].as_array().expect("finishes").len(),
        2,
        "re-mark appends a second finish"
    );
    // Same book, two finishes in 2020 → annual/month still count once.
    let widgets = call(
        &app,
        "GET",
        "/api/v1/insights/widgets",
        Value::Null,
        Some(&token),
    )
    .await
    .1;
    let list = widgets["widgets"].as_array().expect("widgets");
    let goal = list
        .iter()
        .find(|w| w["type"] == "reading-goal")
        .expect("reading-goal");
    assert_eq!(
        goal["data"]["completedBooks"].as_i64().unwrap_or(-1),
        0,
        "2020 finishes must leave current-year goal at 0"
    );
    let challenge = list
        .iter()
        .find(|w| w["type"] == "monthly-challenge")
        .expect("monthly-challenge");
    assert_eq!(
        challenge["data"]["progress"].as_i64().unwrap_or(-1),
        0,
        "2020 finishes must leave current-month challenge at 0"
    );
    let first_id = finishes["finishes"][0]["id"].as_i64().unwrap();
    let second_id = finishes["finishes"][1]["id"].as_i64().unwrap();
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/books/{id}/finishes/{first_id}"),
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
            "DELETE",
            &format!("/api/v1/books/{id}/finishes/{second_id}"),
            Value::Null,
            Some(&token)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    // Last delete clears finished status when no finish events remain.
    let catalogue = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
        .await
        .1;
    assert_eq!(catalogue[0]["reading_status"], "unfinished");
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/books/{id}/finished"),
            json!({"finished_at": "2020-06-15"}),
            Some(&token)
        )
        .await
        .0,
        StatusCode::NO_CONTENT,
        "restore a single backdated finish for later asserts"
    );
    let catalogue = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
        .await
        .1;
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
    let source = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/books/{cbr_id}/source"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(source.status(), StatusCode::OK);
    assert_eq!(
        source.headers()["content-type"],
        "application/vnd.comicbook-rar"
    );
    assert!(to_bytes(source.into_body(), 1024 * 1024)
        .await
        .unwrap()
        .starts_with(b"Rar!"));

    let (status, book) = upload_named(
        &app,
        sample_cb7(),
        Some(&token),
        Some("Night_City_Issue_3.cb7"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{book}");
    let cb7_id = book["id"].as_i64().unwrap();
    let pages = call(
        &app,
        "GET",
        &format!("/api/v1/books/{cb7_id}/pages"),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(pages.0, StatusCode::OK, "{}", pages.1);
    assert_eq!(pages.1["pageCount"], 2);
    let cb7_source = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/books/{cb7_id}/source"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cb7_source.status(), StatusCode::OK);
    assert!(to_bytes(cb7_source.into_body(), 1024 * 1024)
        .await
        .unwrap()
        .starts_with(&[0x37, 0x7a, 0xbc, 0xaf, 0x27, 0x1c]));

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
    let text = call(
        &app,
        "GET",
        &format!("/api/v1/djvu/files/{djvu_id}/pages/0/text"),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(text.0, StatusCode::OK, "{}", text.1);
    assert_eq!(text.1["text"], "");
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
async fn patch_book_updates_title_and_author() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("patch-meta.db");
    let app = shelf_server::app(path).unwrap();
    let credentials = json!({"username":"reader","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, book) = call(
        &app,
        "POST",
        "/api/v1/books",
        json!({"title":"Old Title","author":"Old Author","total_pages":100}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = book["id"].as_i64().unwrap();
    assert_eq!(
        call(
            &app,
            "PATCH",
            &format!("/api/v1/books/{id}"),
            json!({"title":"  New Title  ","author":" New Author "}),
            Some(&token),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let listed = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
        .await
        .1;
    assert_eq!(listed[0]["title"], "New Title");
    assert_eq!(listed[0]["author"], "New Author");
    assert_eq!(
        call(
            &app,
            "PATCH",
            &format!("/api/v1/books/{id}"),
            json!({"title":"","author":"x"}),
            Some(&token),
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &app,
            "PATCH",
            "/api/v1/books/99999",
            json!({"title":"Nope","author":"Ghost"}),
            Some(&token),
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn patch_book_series_fields_round_trip_and_clear() {
    let temp = tempfile::tempdir().unwrap();
    let app = shelf_server::app(temp.path().join("series.db")).unwrap();
    let credentials = json!({"username":"reader","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_string();
    let id = call(
        &app,
        "POST",
        "/api/v1/books",
        json!({"title":"Book","author":"Author","total_pages":10}),
        Some(&token),
    )
    .await
    .1["id"]
        .as_i64()
        .unwrap();
    assert_eq!(
        call(
            &app,
            "PATCH",
            &format!("/api/v1/books/{id}"),
            json!({"title":"Book","author":"Author","series":"  Cycle  ","series_index":2.5}),
            Some(&token),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let listed = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
        .await
        .1;
    assert_eq!(listed[0]["series"], "Cycle");
    assert_eq!(listed[0]["series_index"], 2.5);
    assert_eq!(
        call(
            &app,
            "PATCH",
            &format!("/api/v1/books/{id}"),
            json!({"title":"Book","author":"Author","series":"","series_index":null}),
            Some(&token),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let cleared = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
        .await
        .1;
    assert!(cleared[0]["series"].is_null());
    assert!(cleared[0]["series_index"].is_null());
}

#[tokio::test]
async fn authors_list_counts_books() {
    let temp = tempfile::tempdir().unwrap();
    let app = shelf_server::app(temp.path().join("authors.db")).unwrap();
    let credentials = json!({"username":"reader","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_string();
    for (title, author) in [("One", "Ada"), ("Two", "Ada"), ("Three", "Bea")] {
        call(
            &app,
            "POST",
            "/api/v1/books",
            json!({"title":title,"author":author,"total_pages":5}),
            Some(&token),
        )
        .await;
    }
    let (status, authors) = call(&app, "GET", "/api/v1/authors", Value::Null, Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(authors.as_array().unwrap().len(), 2);
    assert_eq!(authors[0]["name"], "Ada");
    assert_eq!(authors[0]["book_count"], 2);
    assert_eq!(authors[1]["name"], "Bea");
    assert_eq!(authors[1]["book_count"], 1);
}

#[tokio::test]
async fn collections_crud_and_books_filter() {
    let temp = tempfile::tempdir().unwrap();
    let app = shelf_server::app(temp.path().join("collections.db")).unwrap();
    let credentials = json!({"username":"reader","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_string();
    let a = call(
        &app,
        "POST",
        "/api/v1/books",
        json!({"title":"A","author":"X","total_pages":1}),
        Some(&token),
    )
    .await
    .1["id"]
        .as_i64()
        .unwrap();
    let b = call(
        &app,
        "POST",
        "/api/v1/books",
        json!({"title":"B","author":"X","total_pages":1}),
        Some(&token),
    )
    .await
    .1["id"]
        .as_i64()
        .unwrap();
    let (status, created) = call(
        &app,
        "POST",
        "/api/v1/collections",
        json!({"name":"Favourites"}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let cid = created["id"].as_i64().unwrap();
    assert_eq!(
        call(
            &app,
            "PATCH",
            &format!("/api/v1/collections/{cid}"),
            json!({"add_book_ids":[a],"remove_book_ids":[]}),
            Some(&token),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let listed = call(
        &app,
        "GET",
        "/api/v1/collections",
        Value::Null,
        Some(&token),
    )
    .await
    .1;
    assert_eq!(listed[0]["name"], "Favourites");
    assert_eq!(listed[0]["book_count"], 1);
    assert_eq!(listed[0]["book_ids"][0], a);
    let filtered = call(
        &app,
        "GET",
        &format!("/api/v1/books?collection_id={cid}"),
        Value::Null,
        Some(&token),
    )
    .await
    .1;
    assert_eq!(filtered.as_array().unwrap().len(), 1);
    assert_eq!(filtered[0]["id"], a);
    assert_eq!(
        call(
            &app,
            "PATCH",
            &format!("/api/v1/collections/{cid}"),
            json!({"add_book_ids":[b],"remove_book_ids":[a]}),
            Some(&token),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/collections/{cid}"),
            Value::Null,
            Some(&token),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn scopes_crud() {
    let temp = tempfile::tempdir().unwrap();
    let app = shelf_server::app(temp.path().join("scopes.db")).unwrap();
    let credentials = json!({"username":"reader","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, created) = call(
        &app,
        "POST",
        "/api/v1/scopes",
        json!({"name":"Finished","filter":{"status":"finished"}}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let sid = created["id"].as_i64().unwrap();
    let listed = call(&app, "GET", "/api/v1/scopes", Value::Null, Some(&token))
        .await
        .1;
    assert_eq!(listed[0]["name"], "Finished");
    assert_eq!(listed[0]["filter"]["status"], "finished");
    assert_eq!(
        call(
            &app,
            "PATCH",
            &format!("/api/v1/scopes/{sid}"),
            json!({"name":"Done","filter":{"author":"Ada","query":"war"}}),
            Some(&token),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/scopes",
            json!({"name":"Bad","filter":{"unknown":"x"}}),
            Some(&token),
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/scopes/{sid}"),
            Value::Null,
            Some(&token),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn books_pagination_limit_offset() {
    let temp = tempfile::tempdir().unwrap();
    let app = shelf_server::app(temp.path().join("page.db")).unwrap();
    let credentials = json!({"username":"reader","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_string();
    for i in 1..=5 {
        call(
            &app,
            "POST",
            "/api/v1/books",
            json!({"title":format!("T{i}"),"author":"A","total_pages":1}),
            Some(&token),
        )
        .await;
    }
    let page = call(
        &app,
        "GET",
        "/api/v1/books?limit=2&offset=1",
        Value::Null,
        Some(&token),
    )
    .await
    .1;
    assert_eq!(page.as_array().unwrap().len(), 2);
    // ORDER BY id DESC: newest first (5,4,3,2,1) → offset 1 → ids 4,3
    assert_eq!(page[0]["title"], "T4");
    assert_eq!(page[1]["title"], "T3");
}

#[tokio::test]
async fn library_scan_imports_then_duplicates() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("libroot");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("sample.epub"), sample_epub(false)).unwrap();
    // Process-wide env; unique root path avoids colliding with parallel tests.
    std::env::set_var("SHELF_LIBRARY_ROOTS", root.to_str().unwrap());
    let app = shelf_server::app(temp.path().join("scan.db")).unwrap();
    let credentials = json!({"username":"reader","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_string();
    let roots = call(
        &app,
        "GET",
        "/api/v1/library/roots",
        Value::Null,
        Some(&token),
    )
    .await
    .1;
    assert_eq!(roots["roots"][0], root.to_str().unwrap());
    let (status, first) = call(
        &app,
        "POST",
        "/api/v1/library/scan",
        json!({"root": root.to_str().unwrap()}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["scanned"], 1);
    assert_eq!(first["imported"], 1);
    assert_eq!(first["duplicates"], 0);
    let (status, second) = call(
        &app,
        "POST",
        "/api/v1/library/scan",
        json!({}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["imported"], 0);
    assert_eq!(second["duplicates"], 1);
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

#[tokio::test]
async fn mp3_m4b_import_serve_audioms_and_range() {
    let temp = tempfile::tempdir().unwrap();
    let app = shelf_server::app(temp.path().join("audio.db")).unwrap();
    let credentials = json!({"username":"listener","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_owned();

    let (status, book) =
        upload_named(&app, sample_mp3(), Some(&token), Some("Night Watch.mp3")).await;
    assert_eq!(status, StatusCode::CREATED, "{book}");
    let mp3_id = book["id"].as_i64().unwrap();
    let catalogue = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
        .await
        .1;
    assert_eq!(catalogue[0]["format"], "mp3");
    assert_eq!(catalogue[0]["title"], "Night Watch");

    let location = format!("/api/v1/books/{mp3_id}/location");
    assert_eq!(
        call(
            &app,
            "POST",
            &location,
            json!({"position":"audioms(-1)","percentage":0.1}),
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
            json!({"position":"audioms(1500)","percentage":0.25}),
            Some(&token)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let catalogue = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
        .await
        .1;
    assert_eq!(catalogue[0]["position"], "audioms(1500)");
    assert_eq!(catalogue[0]["percentage"], 0.25);

    let file = format!("/api/v1/books/{mp3_id}/file");
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
    assert_eq!(response.headers()["content-type"], "audio/mpeg");
    assert_eq!(response.headers()["accept-ranges"], "bytes");
    assert_eq!(
        to_bytes(response.into_body(), 1024).await.unwrap(),
        sample_mp3()
    );

    let (status, book) =
        upload_named(&app, sample_m4b(), Some(&token), Some("The_Hobbit.m4b")).await;
    assert_eq!(status, StatusCode::CREATED, "{book}");
    let m4b_id = book["id"].as_i64().unwrap();
    let catalogue = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
        .await
        .1;
    let m4b = catalogue
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == m4b_id)
        .unwrap();
    assert_eq!(m4b["format"], "m4b");
    assert_eq!(m4b["title"], "The Hobbit");

    std::fs::create_dir_all(temp.path().join("files")).unwrap();
    let payload: Vec<u8> = (0u8..64).collect();
    let spill_rel = "files/range-fixture-digest.m4b".to_string();
    std::fs::write(temp.path().join(&spill_rel), &payload).unwrap();
    {
        use rusqlite::params;
        let db = rusqlite::Connection::open(temp.path().join("audio.db")).unwrap();
        db.execute(
            "UPDATE epub_files SET content=X'', storage_path=? WHERE book_id=?",
            params![spill_rel, m4b_id],
        )
        .unwrap();
    }
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/books/{m4b_id}/file"))
                .header("authorization", format!("Bearer {token}"))
                .header("range", "bytes=10-19")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.headers()["content-type"], "audio/mp4");
    assert_eq!(response.headers()["content-range"], "bytes 10-19/64");
    assert_eq!(
        to_bytes(response.into_body(), 1024).await.unwrap().as_ref(),
        &payload[10..20]
    );
}

#[tokio::test]
async fn audiobook_manifest_playback_and_zip_tracks() {
    let temp = tempfile::tempdir().unwrap();
    let app = shelf_server::app(temp.path().join("ab.db")).unwrap();
    let credentials = json!({"username":"listener","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_owned();

    let (status, book) = upload_named(&app, sample_flac(), Some(&token), Some("River.flac")).await;
    assert_eq!(status, StatusCode::CREATED, "{book}");
    let flac_id = book["id"].as_i64().unwrap();
    let catalogue = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
        .await
        .1;
    assert_eq!(catalogue[0]["format"], "flac");

    let (status, manifest) = call(
        &app,
        "GET",
        &format!("/api/v1/audiobooks/{flac_id}/manifest"),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{manifest}");
    assert_eq!(manifest["schema"], "bookorbit.audiobook-manifest");
    assert_eq!(manifest["schemaVersion"], 2);
    assert_eq!(manifest["assets"].as_array().unwrap().len(), 1);
    assert_eq!(manifest["assets"][0]["assetId"], "0");
    assert_eq!(manifest["assets"][0]["format"], "flac");
    let revision = manifest["revision"].as_str().unwrap().to_owned();

    let missing = call(
        &app,
        "GET",
        &format!("/api/v1/audiobooks/{flac_id}/playback-state"),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(missing.0, StatusCode::NOT_FOUND);

    let (status, state) = call(
        &app,
        "PUT",
        &format!("/api/v1/audiobooks/{flac_id}/playback-state"),
        json!({
            "assetId": "0",
            "positionMs": 1200,
            "capturedAt": "2026-01-01T00:00:00Z",
            "operationId": "op-1",
            "baseRevision": 0,
            "manifestRevision": revision,
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{state}");
    assert_eq!(state["revision"], 1);
    assert_eq!(state["positionMs"], 1200);

    let (status, asset) = {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/audiobooks/{flac_id}/assets/0/content"))
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let ctype = response.headers()["content-type"]
            .to_str()
            .unwrap()
            .to_owned();
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        (status, (ctype, body))
    };
    assert_eq!(status, StatusCode::OK);
    assert_eq!(asset.0, "audio/flac");
    assert_eq!(asset.1, sample_flac());

    let (status, book) =
        upload_named(&app, sample_audio_zip(), Some(&token), Some("Dune.zip")).await;
    assert_eq!(status, StatusCode::CREATED, "{book}");
    let zip_id = book["id"].as_i64().unwrap();
    let (status, manifest) = call(
        &app,
        "GET",
        &format!("/api/v1/audiobooks/{zip_id}/manifest"),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{manifest}");
    assert_eq!(manifest["assets"].as_array().unwrap().len(), 2);
    assert_eq!(manifest["assets"][0]["assetId"], "0");
    assert_eq!(manifest["chapters"][0]["title"], "01-a");

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/audiobooks/{zip_id}/assets/1/content"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), 1024).await.unwrap(),
        b"ID3two".as_slice()
    );

    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/audiobooks/{flac_id}/playback-state"),
            Value::Null,
            Some(&token),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn aax_import_converts_to_m4b_keeping_source() {
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let tables = temp.path().join("tables");
    std::fs::create_dir_all(&tables).unwrap();
    let m4b_fixture = temp.path().join("out.m4b");
    std::fs::write(&m4b_fixture, sample_converted_m4b()).unwrap();

    let rcrack = bin.join("rcrack");
    std::fs::write(&rcrack, "#!/bin/sh\necho 'result'\necho 'hex:cafed00d'\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&rcrack, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let ffmpeg = bin.join("ffmpeg");
    let fixture_path = m4b_fixture.display().to_string();
    // POSIX-only: CI runners use dash as /bin/sh; bash `${@: -1}` fails there.
    std::fs::write(
        &ffmpeg,
        format!("#!/bin/sh\nout=\nfor arg do out=$arg; done\ncp \"{fixture_path}\" \"$out\"\n"),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&ffmpeg, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    // SAFETY: test-only env for stubbed converter binaries.
    unsafe {
        std::env::set_var("SHELF_AAX_TABLES", &tables);
        std::env::set_var("SHELF_AAX_RCRACK", &rcrack);
        std::env::set_var("SHELF_AAX_FFMPEG", &ffmpeg);
    }

    let app = shelf_server::app(temp.path().join("aax.db")).unwrap();
    let credentials = json!({"username":"aaxuser","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_owned();

    let (status, book) =
        upload_named(&app, sample_aax(), Some(&token), Some("Silent_Planet.aax")).await;
    assert_eq!(status, StatusCode::CREATED, "{book}");
    let id = book["id"].as_i64().unwrap();

    let catalogue = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
        .await
        .1;
    // Stub ffmpeg finishes before the first poll on fast CI; accept aax or m4b.
    assert!(
        catalogue[0]["format"] == "aax" || catalogue[0]["format"] == "m4b",
        "{catalogue}"
    );
    assert_eq!(catalogue[0]["title"], "Silent Planet");
    assert!(
        catalogue[0]["conversion_status"] == "pending"
            || catalogue[0]["conversion_status"] == "running"
            || catalogue[0]["conversion_status"] == "ready"
            || catalogue[0]["conversion_status"] == "failed",
        "{catalogue}"
    );

    let file_while = {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/books/{id}/file"))
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), 8 * 1024).await.unwrap();
        assert!(
            status == StatusCode::CONFLICT || status == StatusCode::OK,
            "{status} {}",
            String::from_utf8_lossy(&body)
        );
        status
    };
    let _ = file_while;

    let mut ready = false;
    for _ in 0..50 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let catalogue = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
            .await
            .1;
        if catalogue[0]["format"] == "m4b" && catalogue[0]["conversion_status"] == "ready" {
            ready = true;
            break;
        }
        if catalogue[0]["conversion_status"] == "failed" {
            panic!("conversion failed: {catalogue}");
        }
    }
    assert!(ready, "AAX did not convert to M4B in time");

    let source = {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/books/{id}/source"))
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        to_bytes(response.into_body(), 8 * 1024).await.unwrap()
    };
    assert_eq!(source.as_ref(), sample_aax().as_slice());

    let playable = {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/books/{id}/file"))
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "audio/mp4");
        to_bytes(response.into_body(), 8 * 1024).await.unwrap()
    };
    assert_eq!(playable.as_ref(), sample_converted_m4b().as_slice());

    let (status, manifest) = call(
        &app,
        "GET",
        &format!("/api/v1/audiobooks/{id}/manifest"),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{manifest}");
    assert_eq!(manifest["assets"][0]["format"], "m4b");
}

#[tokio::test]
async fn combine_epub_audiobook_align_job() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("align.db");
    // Dev/test transcript stand-in (worker looks beside the DB).
    std::fs::write(
        temp.path().join("align-script.txt"),
        "A test chapter Original synthetic test text.",
    )
    .unwrap();
    let app = shelf_server::app(db_path).unwrap();
    let credentials = json!({"username":"aligner","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_owned();

    let (status, epub) = upload_named(
        &app,
        sample_epub(false),
        Some(&token),
        Some("Shared Title.epub"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{epub}");
    let epub_id = epub["id"].as_i64().unwrap();

    let (status, audio) =
        upload_named(&app, sample_mp3(), Some(&token), Some("Shared Title.mp3")).await;
    assert_eq!(status, StatusCode::CREATED, "{audio}");
    let audio_id = audio["id"].as_i64().unwrap();

    let (status, work) = call(
        &app,
        "POST",
        "/api/v1/works",
        json!({"epub_book_id": epub_id, "audio_book_id": audio_id, "start": true}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{work}");
    let work_id = work["id"].as_i64().unwrap();
    assert!(
        work["status"] == "queued" || work["status"] == "processing" || work["status"] == "ready",
        "{work}"
    );

    let mut ready = None;
    for _ in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let (status, w) = call(
            &app,
            "GET",
            &format!("/api/v1/works/{work_id}"),
            Value::Null,
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{w}");
        if w["status"] == "ready" {
            ready = Some(w);
            break;
        }
        if w["status"] == "failed" {
            panic!("align failed: {w}");
        }
    }
    let w = ready.expect("align did not finish in time");
    assert!(w["readaloud_book_id"].as_i64().is_some(), "{w}");
    assert_eq!(w["progress"].as_f64().unwrap_or(0.0), 1.0);

    let catalogue = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
        .await
        .1;
    let readaloud_id = w["readaloud_book_id"].as_i64().unwrap();
    let ra = catalogue
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == readaloud_id)
        .expect("readaloud in catalogue");
    assert_eq!(ra["format"], "epub");
    assert!(
        ra["title"].as_str().unwrap_or("").contains("readaloud"),
        "{ra}"
    );
    // EPUB + audio + readaloud rows exist, but stats count the work as one shelf book.
    let catalogue_n = catalogue.as_array().unwrap().len();
    assert!(
        catalogue_n >= 3,
        "expected epub+audio+readaloud, got {catalogue_n}"
    );
    let stats = call(&app, "GET", "/api/v1/stats", Value::Null, Some(&token))
        .await
        .1;
    assert_eq!(stats["books"], 1, "{stats}");
}

#[tokio::test]
async fn readaloud_finish_mirrors_to_source_epub_card() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("mirror-progress.db");
    let app = shelf_server::app(db_path.clone()).unwrap();
    let credentials = json!({"username":"mirror","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_owned();

    let (status, epub) = upload_named(
        &app,
        sample_epub(false),
        Some(&token),
        Some("Four Lamps.epub"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{epub}");
    let epub_id = epub["id"].as_i64().unwrap();

    let (status, audio) =
        upload_named(&app, sample_mp3(), Some(&token), Some("Four Lamps.mp3")).await;
    assert_eq!(status, StatusCode::CREATED, "{audio}");
    let audio_id = audio["id"].as_i64().unwrap();

    let (status, ra) = upload_named(
        &app,
        sample_epub_with_media_overlay(),
        Some(&token),
        Some("Four Lamps (readaloud).epub"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{ra}");
    let readaloud_id = ra["id"].as_i64().unwrap();

    // Seed a ready work the way Combine would — no whisper needed for this gate.
    {
        let db = rusqlite::Connection::open(&db_path).unwrap();
        db.execute(
            "INSERT INTO book_works(epub_book_id, audio_book_id, readaloud_book_id, status, stage, progress)
             VALUES (?1, ?2, ?3, 'ready', 'SYNC_CHAPTERS', 1)",
            rusqlite::params![epub_id, audio_id, readaloud_id],
        )
        .unwrap();
    }

    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/books/{readaloud_id}/location"),
            json!({"position":"epubcfi(/6/8!/4/2/2[sentence-0]/1:0)","percentage":1.0}),
            Some(&token),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );

    let catalogue = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
        .await
        .1;
    let books = catalogue.as_array().unwrap();
    let epub_row = books.iter().find(|b| b["id"] == epub_id).expect("epub");
    let ra_row = books
        .iter()
        .find(|b| b["id"] == readaloud_id)
        .expect("readaloud");
    assert_eq!(ra_row["percentage"], 1.0, "{ra_row}");
    assert_eq!(ra_row["reading_status"], "finished", "{ra_row}");
    assert_eq!(epub_row["percentage"], 1.0, "{epub_row}");
    assert_eq!(epub_row["reading_status"], "finished", "{epub_row}");
    assert_eq!(epub_row["position"], "epubcfi(/6/8!/4/2/2[sentence-0]/1:0)");
}

#[tokio::test]
async fn combine_with_merge_title_renames_mismatched_editions() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("align-merge.db");
    std::fs::write(
        temp.path().join("align-script.txt"),
        "A test chapter Original synthetic test text.",
    )
    .unwrap();
    let app = shelf_server::app(db_path).unwrap();
    let credentials = json!({"username":"merger","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_owned();

    let (status, epub) = upload_named(
        &app,
        sample_epub(false),
        Some(&token),
        Some("Return of the King EPUB edition.epub"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{epub}");
    let epub_id = epub["id"].as_i64().unwrap();

    let (status, audio) = upload_named(
        &app,
        sample_mp3(),
        Some(&token),
        Some("RotK Audible Book 3.mp3"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{audio}");
    let audio_id = audio["id"].as_i64().unwrap();

    let (status, work) = call(
        &app,
        "POST",
        "/api/v1/works",
        json!({
            "epub_book_id": epub_id,
            "audio_book_id": audio_id,
            "title": "The Return of the King",
            "start": true
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{work}");
    let work_id = work["id"].as_i64().unwrap();

    let catalogue = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
        .await
        .1;
    let books = catalogue.as_array().unwrap();
    let epub_row = books.iter().find(|b| b["id"] == epub_id).unwrap();
    let audio_row = books.iter().find(|b| b["id"] == audio_id).unwrap();
    assert_eq!(epub_row["title"], "The Return of the King");
    assert_eq!(audio_row["title"], "The Return of the King");

    let mut ready = None;
    for _ in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let (status, w) = call(
            &app,
            "GET",
            &format!("/api/v1/works/{work_id}"),
            Value::Null,
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{w}");
        if w["status"] == "ready" {
            ready = Some(w);
            break;
        }
        if w["status"] == "failed" {
            panic!("align failed: {w}");
        }
    }
    let w = ready.expect("align did not finish in time");
    let readaloud_id = w["readaloud_book_id"].as_i64().unwrap();
    let catalogue = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
        .await
        .1;
    let ra = catalogue
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == readaloud_id)
        .expect("readaloud in catalogue");
    assert_eq!(ra["title"], "The Return of the King (readaloud)");
}

#[tokio::test]
async fn metadata_apply_and_fetch_queue() {
    let temp = tempfile::tempdir().unwrap();
    let app = shelf_server::app(temp.path().join("meta.db")).unwrap();
    let credentials = json!({"username":"reader","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_string();
    let id = call(
        &app,
        "POST",
        "/api/v1/books",
        json!({"title":"Dune","author":"Frank Herbert","total_pages":1}),
        Some(&token),
    )
    .await
    .1["id"]
        .as_i64()
        .unwrap();

    let providers = call(
        &app,
        "GET",
        "/api/v1/metadata/providers",
        Value::Null,
        Some(&token),
    )
    .await
    .1;
    assert!(providers
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["key"] == "openLibrary"));

    let (status, _) = call(
        &app,
        "PATCH",
        &format!("/api/v1/books/{id}/metadata"),
        json!({
            "description": "Desert planet.",
            "genres": ["Science Fiction"],
            "isbn13": "9780441172719"
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let meta = call(
        &app,
        "GET",
        &format!("/api/v1/books/{id}/metadata"),
        Value::Null,
        Some(&token),
    )
    .await
    .1;
    assert_eq!(meta["description"], "Desert planet.");
    assert_eq!(meta["genres"][0], "Science Fiction");
    let listed = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
        .await
        .1;
    assert_eq!(listed[0]["genres"], json!(["Science Fiction"]));
    assert!(meta["metadata_score"].as_i64().unwrap() > 0);

    let (status, applied) = call(
        &app,
        "POST",
        &format!("/api/v1/books/{id}/metadata/apply"),
        json!({
            "candidate": {
                "provider": "openLibrary",
                "external_id": "/works/OL123",
                "title": "Dune",
                "subtitle": "Complete",
                "authors": ["Frank Herbert"],
                "description": "From provider",
                "publisher": "Chilton"
            },
            "fields": ["subtitle", "publisher"],
            "respect_locks": true
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(applied["applied"]
        .as_array()
        .unwrap()
        .contains(&json!("subtitle")));

    let meta = call(
        &app,
        "GET",
        &format!("/api/v1/books/{id}/metadata"),
        Value::Null,
        Some(&token),
    )
    .await
    .1;
    assert_eq!(meta["subtitle"], "Complete");
    assert_eq!(meta["publisher"], "Chilton");
    // Locked description should not be overwritten by apply when locked.
    assert_eq!(
        call(
            &app,
            "PATCH",
            &format!("/api/v1/books/{id}/metadata"),
            json!({"locked_fields": ["description"]}),
            Some(&token),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    call(
        &app,
        "POST",
        &format!("/api/v1/books/{id}/metadata/apply"),
        json!({
            "candidate": {
                "provider": "google",
                "external_id": "g1",
                "description": "Should not apply"
            },
            "fields": ["description"],
            "respect_locks": true
        }),
        Some(&token),
    )
    .await;
    let meta = call(
        &app,
        "GET",
        &format!("/api/v1/books/{id}/metadata"),
        Value::Null,
        Some(&token),
    )
    .await
    .1;
    assert_eq!(meta["description"], "Desert planet.");

    let (status, run) = call(
        &app,
        "POST",
        "/api/v1/metadata/fetch/run",
        json!({"ignore_cache": false}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(run["queued"].as_i64().unwrap() >= 0);

    let st = call(
        &app,
        "GET",
        "/api/v1/metadata/fetch/status",
        Value::Null,
        Some(&token),
    )
    .await
    .1;
    assert!(st.get("queued").is_some());
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/metadata/fetch/pause",
            json!({}),
            Some(&token)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn reading_sessions_idempotent_and_daily_rollup() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("sessions.db");
    let app = shelf_server::app(path).unwrap();
    let credentials = json!({"username":"reader","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, book) = upload(&app, sample_epub(false), Some(&token)).await;
    assert_eq!(status, StatusCode::CREATED, "{book}");
    let id = book["id"].as_i64().unwrap();

    let body = json!({
        "sessionId": "11111111-2222-4333-8444-555555555555",
        "startedAt": "2026-03-15T18:00:00Z",
        "endedAt": "2026-03-15T18:20:00Z",
        "durationSeconds": 900,
        "progressDelta": 5.5,
        "endProgress": 42.0,
        "startProgress": 36.5,
        "sessionType": "read",
        "source": "web",
        "timezoneOffsetMinutes": 0
    });
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/books/{id}/sessions"),
            body.clone(),
            Some(&token),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    // Foliate file-id alias + idempotent replay must not double daily counts.
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/books/files/{id}/sessions"),
            body,
            Some(&token),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );

    let sessions = call(
        &app,
        "GET",
        &format!("/api/v1/books/{id}/sessions"),
        Value::Null,
        Some(&token),
    )
    .await
    .1;
    assert!(sessions.is_array(), "{sessions}");
    assert_eq!(sessions.as_array().unwrap().len(), 1);
    assert_eq!(
        sessions[0]["sessionId"],
        "11111111-2222-4333-8444-555555555555"
    );
    assert_eq!(sessions[0]["durationSeconds"], 900);
    assert_eq!(sessions[0]["progressEnd"], 42.0);
    assert_eq!(sessions[0]["source"], "web");

    let daily = call(
        &app,
        "GET",
        "/api/v1/reading-daily",
        Value::Null,
        Some(&token),
    )
    .await
    .1;
    assert!(daily.is_array(), "{daily}");
    let web = daily
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["source"] == "web" && row["day"] == "2026-03-15")
        .expect("daily web row");
    assert_eq!(web["sessionCount"], 1);
    assert_eq!(web["readingSeconds"], 900);
    assert_eq!(web["progressDeltaSum"], 5.5);

    // Under-min sessions are accepted but not stored.
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/books/{id}/sessions"),
            json!({
                "sessionId": "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee",
                "startedAt": "2026-03-15T19:00:00Z",
                "endedAt": "2026-03-15T19:00:05Z",
                "durationSeconds": 5,
                "endProgress": 43.0
            }),
            Some(&token),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let sessions = call(
        &app,
        "GET",
        &format!("/api/v1/books/{id}/sessions"),
        Value::Null,
        Some(&token),
    )
    .await
    .1;
    assert_eq!(sessions.as_array().unwrap().len(), 1);

    let prefs = call(
        &app,
        "GET",
        "/api/v1/reading-prefs",
        Value::Null,
        Some(&token),
    )
    .await
    .1;
    assert_eq!(prefs["timezone"], "UTC");
    let updated = call(
        &app,
        "PUT",
        "/api/v1/reading-prefs",
        json!({"timezone": "America/Chicago", "annualGoalBooks": 24}),
        Some(&token),
    )
    .await
    .1;
    assert_eq!(updated["timezone"], "America/Chicago");
    assert_eq!(updated["annualGoalBooks"], 24);

    let insights = call(
        &app,
        "GET",
        "/api/v1/insights/reading?from=2026-03-01&to=2026-03-31",
        Value::Null,
        Some(&token),
    )
    .await
    .1;
    assert_eq!(insights["from"], "2026-03-01");
    assert_eq!(insights["to"], "2026-03-31");
    let heat = insights["heatmap"].as_array().expect("heatmap");
    let day = heat
        .iter()
        .find(|row| row["day"] == "2026-03-15")
        .expect("heatmap day");
    assert_eq!(day["readingSeconds"], 900);
    assert_eq!(day["sessionCount"], 1);
    let slices = insights["sourceDistribution"]["slices"]
        .as_array()
        .expect("slices");
    assert!(slices
        .iter()
        .any(|s| s["bucket"] == "web" && s["readingSeconds"] == 900));
    let timeline = insights["sessionTimeline"].as_array().expect("timeline");
    assert_eq!(timeline.len(), 1);
    assert_eq!(timeline[0]["durationSeconds"], 900);
    assert!(insights["streak"]["longestStreak"].as_i64().unwrap_or(0) >= 1);
    assert_eq!(
        insights["streak"]["lastSevenDays"]
            .as_array()
            .unwrap()
            .len(),
        7
    );

    let widgets = call(
        &app,
        "GET",
        "/api/v1/insights/widgets",
        Value::Null,
        Some(&token),
    )
    .await
    .1;
    let list = widgets["widgets"].as_array().expect("widgets");
    assert!(list.iter().any(|w| w["type"] == "reading-streak"));
    assert!(list.iter().any(|w| w["type"] == "reading-goal"));
    assert!(list.iter().any(|w| w["type"] == "monthly-challenge"));
    assert!(list.iter().any(|w| w["type"] == "reading-dna"));
    let goal = list.iter().find(|w| w["type"] == "reading-goal").unwrap();
    assert_eq!(goal["data"]["goalBooks"], 24);

    let achievements = call(
        &app,
        "GET",
        "/api/v1/insights/achievements",
        Value::Null,
        Some(&token),
    )
    .await
    .1;
    assert_eq!(achievements["totalAvailable"].as_i64().unwrap(), 10);
    assert!(achievements["totalEarned"].as_i64().unwrap() >= 1);
    let cats = achievements["categories"].as_array().expect("categories");
    assert!(cats.iter().any(|c| c["key"] == "reading"));
    let reading = cats.iter().find(|c| c["key"] == "reading").unwrap();
    let first = reading["achievements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["key"] == "first_session")
        .unwrap();
    assert_eq!(first["earned"], true);

    let library = call(
        &app,
        "GET",
        "/api/v1/insights/library",
        Value::Null,
        Some(&token),
    )
    .await
    .1;
    assert!(library["formatDistribution"]["total"].as_i64().unwrap() >= 1);
    let formats = library["formatDistribution"]["slices"].as_array().unwrap();
    assert!(formats
        .iter()
        .any(|s| s["count"].as_i64().unwrap_or(0) >= 1));
    assert!(library["metadataCompleteness"]["total"].as_i64().unwrap() >= 1);
}

#[tokio::test]
async fn fold_editions_merge_hides_secondary_and_keeps_filename() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("fold-merge.db");
    let app = shelf_server::app(db_path).unwrap();
    let credentials = json!({"username":"folder","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_owned();

    let (status, first) = upload_named(
        &app,
        sample_epub(false),
        Some(&token),
        Some("primary-edition.epub"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    let primary_id = first["id"].as_i64().unwrap();

    // Second EPUB must differ by digest from the first fixture.
    let (status, second) = upload_named(
        &app,
        sample_epub_with_media_overlay(),
        Some(&token),
        Some("duplicate-copy.epub"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{second}");
    let secondary_id = second["id"].as_i64().unwrap();

    let catalogue = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
        .await
        .1;
    let books = catalogue.as_array().unwrap();
    let primary = books.iter().find(|b| b["id"] == primary_id).unwrap();
    assert_eq!(primary["original_filename"], "primary-edition.epub");

    let (status, merged) = call(
        &app,
        "POST",
        "/api/v1/books/merge",
        json!({
            "primary_id": primary_id,
            "secondary_id": secondary_id,
            "title": "One Shelf Card"
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{merged}");

    let catalogue = call(&app, "GET", "/api/v1/books", Value::Null, Some(&token))
        .await
        .1;
    let books = catalogue.as_array().unwrap();
    let primary = books.iter().find(|b| b["id"] == primary_id).unwrap();
    let secondary = books.iter().find(|b| b["id"] == secondary_id).unwrap();
    assert_eq!(primary["title"], "One Shelf Card");
    assert_eq!(secondary["title"], "One Shelf Card");
    assert_eq!(secondary["merged_into"], primary_id);
    assert!(primary.get("merged_into").is_none() || primary["merged_into"].is_null());

    let stats = call(&app, "GET", "/api/v1/stats", Value::Null, Some(&token))
        .await
        .1;
    assert_eq!(stats["books"], 1, "{stats}");
}

#[tokio::test]
async fn koreader_page_stats_clusters_into_sessions() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("koreader.db");
    let app = shelf_server::app(path).unwrap();
    let credentials = json!({"username":"koreader","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, book) = upload(&app, sample_epub(false), Some(&token)).await;
    assert_eq!(status, StatusCode::CREATED, "{book}");
    let id = book["id"].as_i64().unwrap();

    // Two clusters: contiguous pages, then a >1800s gap.
    let body = json!({
        "deviceId": "koreader-device-1",
        "deviceName": "Kindle",
        "events": [
            {"page": 10, "startTime": 1_700_000_000, "durationSeconds": 60, "totalPages": 200},
            {"page": 11, "startTime": 1_700_000_060, "durationSeconds": 60, "totalPages": 200},
            {"page": 20, "startTime": 1_700_002_000, "durationSeconds": 120, "totalPages": 200}
        ],
        "timezoneOffsetMinutes": 0
    });
    let (status, result) = call(
        &app,
        "POST",
        &format!("/api/v1/books/{id}/koreader/page-stats"),
        body.clone(),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["imported"], 2, "{result}");
    assert_eq!(result["source"], "koreader");
    assert_eq!(result["sessionIds"].as_array().unwrap().len(), 2);

    // Idempotent replay.
    let (status, replay) = call(
        &app,
        "POST",
        &format!("/api/v1/books/{id}/koreader/page-stats"),
        body,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["imported"], 0, "{replay}");
    assert_eq!(replay["skipped"], 2, "{replay}");

    let sessions = call(
        &app,
        "GET",
        &format!("/api/v1/books/{id}/sessions"),
        Value::Null,
        Some(&token),
    )
    .await
    .1;
    assert_eq!(sessions.as_array().unwrap().len(), 2, "{sessions}");
    assert_eq!(sessions[0]["source"], "koreader");
    assert!(sessions[0]["sessionId"]
        .as_str()
        .unwrap()
        .starts_with("kor:"));

    let daily = call(
        &app,
        "GET",
        "/api/v1/reading-daily",
        Value::Null,
        Some(&token),
    )
    .await
    .1;
    let kor = daily
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["source"] == "koreader")
        .expect("koreader daily row");
    assert_eq!(kor["sessionCount"], 2);
    assert_eq!(kor["readingSeconds"], 240);
}

#[tokio::test]
async fn kobo_statistics_delta_creates_session() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("kobo.db");
    let app = shelf_server::app(path).unwrap();
    let credentials = json!({"username":"kobo","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, book) = upload(&app, sample_epub(false), Some(&token)).await;
    assert_eq!(status, StatusCode::CREATED, "{book}");
    let id = book["id"].as_i64().unwrap();

    let body = json!({
        "deviceId": "kobo-device-1",
        "deviceName": "Clara 2E",
        "spentReadingMinutes": 15,
        "lastModified": "2026-03-15T18:30:00Z",
        "endProgress": 25.0,
        "timezoneOffsetMinutes": 0
    });
    let (status, result) = call(
        &app,
        "POST",
        &format!("/api/v1/books/{id}/kobo/statistics"),
        body.clone(),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["imported"], 1, "{result}");
    assert_eq!(result["source"], "kobo");
    assert_eq!(result["durationSeconds"], 900);
    assert!(result["sessionIds"][0]
        .as_str()
        .unwrap()
        .starts_with("kst:"));

    // Same counter → noop.
    let (status, noop) = call(
        &app,
        "POST",
        &format!("/api/v1/books/{id}/kobo/statistics"),
        body,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{noop}");
    assert_eq!(noop["imported"], 0, "{noop}");
    assert_eq!(noop["skipped"], 1, "{noop}");

    // Counter advance → another session.
    let (status, second) = call(
        &app,
        "POST",
        &format!("/api/v1/books/{id}/kobo/statistics"),
        json!({
            "deviceId": "kobo-device-1",
            "spentReadingMinutes": 20,
            "lastModified": "2026-03-15T19:00:00Z",
            "endProgress": 30.0
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["imported"], 1, "{second}");
    assert_eq!(second["durationSeconds"], 300);

    let sessions = call(
        &app,
        "GET",
        &format!("/api/v1/books/{id}/sessions"),
        Value::Null,
        Some(&token),
    )
    .await
    .1;
    assert_eq!(sessions.as_array().unwrap().len(), 2, "{sessions}");
    assert_eq!(sessions[0]["source"], "kobo");
}

#[tokio::test]
async fn kindle_delivery_settings_and_stub_send() {
    std::env::set_var("SHELF_KINDLE_SMTP_STUB", "1");
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("kindle.db");
    let app = shelf_server::app(path).unwrap();
    let credentials = json!({"username":"kindle","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, empty) = call(
        &app,
        "GET",
        "/api/v1/kindle-delivery",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{empty}");
    assert_eq!(empty["configured"], false, "{empty}");
    assert_eq!(empty["hasPassword"], false, "{empty}");

    let (status, saved) = call(
        &app,
        "PUT",
        "/api/v1/kindle-delivery",
        json!({
            "recipientEmail": "me@kindle.com",
            "smtpHost": "smtp.example.test",
            "smtpPort": 587,
            "smtpUsername": "sender@example.test",
            "smtpPassword": "smtp-secret",
            "fromAddress": "sender@example.test",
            "fromName": "Shelf",
            "startTls": true,
            "ssl": false
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["configured"], true, "{saved}");
    assert_eq!(saved["hasPassword"], true, "{saved}");
    assert_eq!(saved["recipientEmail"], "me@kindle.com");
    assert!(saved.get("smtpPassword").is_none(), "{saved}");

    let (status, book) = upload(&app, sample_epub(false), Some(&token)).await;
    assert_eq!(status, StatusCode::CREATED, "{book}");
    let id = book["id"].as_i64().unwrap();

    let (status, sent) = call(
        &app,
        "POST",
        &format!("/api/v1/books/{id}/send-to-kindle"),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    assert_eq!(sent["sent"], true, "{sent}");
    assert_eq!(sent["subject"], "convert", "{sent}");
    assert_eq!(sent["format"], "epub");
    assert_eq!(sent["to"], "me@kindle.com");

    let (status, pdf) = upload_named(&app, sample_pdf(), Some(&token), Some("fixture.pdf")).await;
    assert_eq!(status, StatusCode::CREATED, "{pdf}");
    let pdf_id = pdf["id"].as_i64().unwrap();
    let (status, pdf_sent) = call(
        &app,
        "POST",
        &format!("/api/v1/books/{pdf_id}/send-to-kindle"),
        json!({}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{pdf_sent}");
    assert_eq!(pdf_sent["format"], "pdf", "{pdf_sent}");

    let (status, audio) = upload(&app, sample_mp3(), Some(&token)).await;
    assert_eq!(status, StatusCode::CREATED, "{audio}");
    let audio_id = audio["id"].as_i64().unwrap();
    let (status, reject) = call(
        &app,
        "POST",
        &format!("/api/v1/books/{audio_id}/send-to-kindle"),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{reject}");
}

#[tokio::test]
async fn opds_basic_auth_catalog_and_download() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("opds.db");
    let app = shelf_server::app(path).unwrap();
    let credentials = json!({"username":"opds-owner","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, empty) = call(&app, "GET", "/api/v1/opds-users", Value::Null, Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{empty}");
    assert_eq!(empty["users"].as_array().unwrap().len(), 0, "{empty}");

    let (status, created) = call(
        &app,
        "POST",
        "/api/v1/opds-users",
        json!({"username":"eink","password":"eink-secret-long"}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let opds_id = created["id"].as_i64().unwrap();
    assert_eq!(created["username"], "eink");

    let (status, listed) = call(&app, "GET", "/api/v1/opds-users", Value::Null, Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(listed["users"].as_array().unwrap().len(), 1, "{listed}");

    // Session Bearer must not unlock OPDS feeds.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/opds")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // base64("eink:eink-secret-long")
    let basic = "ZWluazplaW5rLXNlY3JldC1sb25n";
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/opds")
                .header("authorization", format!("Basic {basic}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let ctype = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(ctype.contains("opds-catalog"), "{ctype}");
    let body = String::from_utf8(
        to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(body.contains("All Books"), "{body}");
    assert!(body.contains("/api/v1/opds/catalog"), "{body}");

    let (status, book) = upload(&app, sample_epub(false), Some(&token)).await;
    assert_eq!(status, StatusCode::CREATED, "{book}");
    let book_id = book["id"].as_i64().unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/opds/catalog")
                .header("authorization", format!("Basic {basic}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let catalog = String::from_utf8(
        to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(
        catalog.contains(&format!("/api/v1/opds/{book_id}/file")),
        "{catalog}"
    );
    assert!(
        catalog.contains("http://opds-spec.org/acquisition"),
        "{catalog}"
    );

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/opds/{book_id}/file"))
                .header("authorization", format!("Basic {basic}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let file_ctype = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(file_ctype, "application/epub+zip");
    let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    assert!(!bytes.is_empty());

    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/api/v1/opds-users/{opds_id}"),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/opds")
                .header("authorization", format!("Basic {basic}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

async fn hardcover_settings_validate_and_stub_sync() {
    std::env::set_var("SHELF_HARDCOVER_STUB", "1");
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("hardcover.db");
    let app = shelf_server::app(path).unwrap();
    let credentials = json!({"username":"hc","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, empty) = call(&app, "GET", "/api/v1/hardcover", Value::Null, Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{empty}");
    assert_eq!(empty["tokenConfigured"], false, "{empty}");
    assert_eq!(empty["effectiveEnabled"], false, "{empty}");

    let (status, saved) = call(
        &app,
        "PUT",
        "/api/v1/hardcover",
        json!({
            "apiToken": "hc-test-token",
            "enabled": true,
            "bookSyncMode": "all_eligible",
            "privacySettingId": 3
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["tokenConfigured"], true, "{saved}");
    assert_eq!(saved["effectiveEnabled"], true, "{saved}");
    assert!(saved.get("apiToken").is_none(), "{saved}");

    let (status, valid) = call(
        &app,
        "POST",
        "/api/v1/hardcover/validate",
        json!({}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{valid}");
    assert_eq!(valid["valid"], true, "{valid}");
    assert_eq!(valid["hardcoverUsername"], "stub-user", "{valid}");

    let (status, book) = upload(&app, sample_epub(false), Some(&token)).await;
    assert_eq!(status, StatusCode::CREATED, "{book}");
    let id = book["id"].as_i64().unwrap();
    call(
        &app,
        "POST",
        &format!("/api/v1/books/{id}/finished"),
        json!({"finished_at": "2024-06-12"}),
        Some(&token),
    )
    .await;

    let (status, synced) = call(
        &app,
        "POST",
        "/api/v1/hardcover/sync",
        json!({}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{synced}");
    assert_eq!(synced["status"], "completed", "{synced}");
    assert!(synced["syncedBooks"].as_i64().unwrap() >= 1, "{synced}");

    let (status, one) = call(
        &app,
        "POST",
        &format!("/api/v1/books/{id}/hardcover/sync"),
        json!({}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{one}");
    assert_eq!(one["result"], "synced", "{one}");

    let (status, _) = call(
        &app,
        "DELETE",
        "/api/v1/hardcover",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, after) = call(&app, "GET", "/api/v1/hardcover", Value::Null, Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{after}");
    assert_eq!(after["tokenConfigured"], false, "{after}");
}

#[tokio::test]

async fn readwise_settings_validate_and_stub_sync() {
    std::env::set_var("SHELF_READWISE_STUB", "1");
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("readwise.db");
    let app = shelf_server::app(path).unwrap();
    let credentials = json!({"username":"rw","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, empty) = call(&app, "GET", "/api/v1/readwise", Value::Null, Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{empty}");
    assert_eq!(empty["tokenConfigured"], false, "{empty}");

    let (status, saved) = call(
        &app,
        "PUT",
        "/api/v1/readwise",
        json!({ "apiToken": "rw-test-token", "enabled": true }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["effectiveEnabled"], true, "{saved}");
    assert!(saved.get("apiToken").is_none(), "{saved}");

    let (status, valid) = call(
        &app,
        "POST",
        "/api/v1/readwise/validate",
        json!({}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{valid}");
    assert_eq!(valid["valid"], true, "{valid}");

    let (status, book) = upload(&app, sample_epub(false), Some(&token)).await;
    assert_eq!(status, StatusCode::CREATED, "{book}");
    let id = book["id"].as_i64().unwrap();
    let (status, ann) = call(
        &app,
        "POST",
        &format!("/api/v1/books/{id}/annotations"),
        json!({
            "cfi": "epubcfi(/6/2!/4)",
            "text": "A stub highlight for Readwise",
            "note": "margin",
            "chapterTitle": "Ch 1"
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{ann}");

    let (status, synced) = call(
        &app,
        "POST",
        "/api/v1/readwise/sync",
        json!({}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{synced}");
    assert_eq!(synced["syncedHighlights"], 1, "{synced}");
    assert!(
        synced["lastSyncedAnnotationId"].as_i64().unwrap() > 0,
        "{synced}"
    );

    let (status, again) = call(
        &app,
        "POST",
        "/api/v1/readwise/sync",
        json!({}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["syncedHighlights"], 0, "{again}");

    let (status, _) = call(
        &app,
        "DELETE",
        "/api/v1/readwise",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]

async fn storygraph_settings_validate_and_stub_sync() {
    std::env::set_var("SHELF_STORYGRAPH_STUB", "1");
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("storygraph.db");
    let app = shelf_server::app(path).unwrap();
    let credentials = json!({"username":"sg","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, empty) = call(&app, "GET", "/api/v1/storygraph", Value::Null, Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{empty}");
    assert_eq!(empty["cookiesConfigured"], false, "{empty}");

    let (status, saved) = call(
        &app,
        "PUT",
        "/api/v1/storygraph",
        json!({
            "sessionCookie": "sg-session",
            "rememberToken": "sg-remember",
            "enabled": true,
            "bookSyncMode": "all_eligible"
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["effectiveEnabled"], true, "{saved}");
    assert!(saved.get("sessionCookie").is_none(), "{saved}");

    let (status, valid) = call(
        &app,
        "POST",
        "/api/v1/storygraph/validate",
        json!({}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{valid}");
    assert_eq!(valid["valid"], true, "{valid}");

    let (status, book) = upload(&app, sample_epub(false), Some(&token)).await;
    assert_eq!(status, StatusCode::CREATED, "{book}");
    let id = book["id"].as_i64().unwrap();
    call(
        &app,
        "POST",
        &format!("/api/v1/books/{id}/finished"),
        json!({"finished_at": "2024-06-12"}),
        Some(&token),
    )
    .await;

    let (status, synced) = call(
        &app,
        "POST",
        "/api/v1/storygraph/sync",
        json!({}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{synced}");
    assert!(synced["syncedBooks"].as_i64().unwrap() >= 1, "{synced}");

    let (status, one) = call(
        &app,
        "POST",
        &format!("/api/v1/books/{id}/storygraph/sync"),
        json!({}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{one}");
    assert_eq!(one["result"], "synced", "{one}");

    let (status, _) = call(
        &app,
        "DELETE",
        "/api/v1/storygraph",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]

async fn koreader_hash_link_annotation_and_progress() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("koreader-sync.db");
    let app = shelf_server::app(path).unwrap();
    let credentials = json!({"username":"kor","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, saved) = call(
        &app,
        "PUT",
        "/api/v1/koreader",
        json!({
            "username": "kor-plugin",
            "password": "plugin-secret-ok",
            "syncEnabled": true,
            "hideAudioOnlyBooks": true
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["effectiveEnabled"], true, "{saved}");
    assert!(saved.get("password").is_none(), "{saved}");

    let (status, book) = upload(&app, sample_epub(false), Some(&token)).await;
    assert_eq!(status, StatusCode::CREATED, "{book}");
    let id = book["id"].as_i64().unwrap();
    let hash = "0123456789abcdef0123456789abcdef";

    let (status, unmatched) = call(
        &app,
        "POST",
        "/api/v1/koreader/plugin/match-check",
        json!({
            "deviceId": "dev-1",
            "deviceModel": "test",
            "pluginVersion": "0.1",
            "hashes": [hash],
            "books": [{"hash": hash, "title": "Unmatched Title", "authors": "A"}]
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{unmatched}");
    assert_eq!(unmatched["unmatchedCount"], 1, "{unmatched}");

    let (status, linked) = call(
        &app,
        "POST",
        &format!("/api/v1/koreader/unmatched/{hash}/link"),
        json!({"bookId": id}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{linked}");
    assert_eq!(linked["bookId"], id, "{linked}");

    let (status, matched) = call(
        &app,
        "POST",
        "/api/v1/koreader/plugin/match-check",
        json!({
            "deviceId": "dev-1",
            "hashes": [hash]
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{matched}");
    assert_eq!(matched["matchedCount"], 1, "{matched}");

    let (status, uploaded) = call(
        &app,
        "POST",
        "/api/v1/koreader/plugin/annotations",
        json!({
            "deviceId": "dev-1",
            "books": [{
                "hash": hash,
                "annotations": [{
                    "datetime": "2024-06-12 10:00:00",
                    "drawer": "lighten",
                    "text": "A highlight",
                    "note": "from device",
                    "chapter": "Ch 1",
                    "posFormat": "xpointer",
                    "pos0": "/body/DocFragment[1]/text()[2].12"
                }]
            }]
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{uploaded}");
    assert_eq!(uploaded["upserted"], 1, "{uploaded}");

    let (status, anns) = call(
        &app,
        "GET",
        &format!("/api/v1/books/{id}/annotations"),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{anns}");
    assert!(
        anns.as_array()
            .unwrap()
            .iter()
            .any(|a| a["text"] == "A highlight"),
        "{anns}"
    );

    let (status, prog) = call(
        &app,
        "POST",
        "/api/v1/koreader/plugin/progress",
        json!({
            "deviceId": "dev-1",
            "items": [{"hash": hash, "percentage": 0.42}]
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{prog}");
    assert_eq!(prog["updated"], 1, "{prog}");

    let (status, catalog) = call(
        &app,
        "GET",
        "/api/v1/koreader/plugin/catalog",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{catalog}");
    assert!(catalog["total"].as_i64().unwrap() >= 1, "{catalog}");

    let (status, _) = call(
        &app,
        "DELETE",
        "/api/v1/koreader",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]

async fn kobo_device_library_sync_and_progress() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("kobo-sync.db");
    let app = shelf_server::app(path).unwrap();
    let credentials = json!({"username":"kobo","password":"test-password-long-enough"});
    call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    let token = call(&app, "POST", "/api/v1/login", credentials, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, device) = call(
        &app,
        "POST",
        "/api/v1/kobo/devices",
        json!({"name": "Test Clara"}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{device}");
    let device_token = device["token"].as_str().unwrap().to_string();

    let (status, book) = upload(&app, sample_epub(false), Some(&token)).await;
    assert_eq!(status, StatusCode::CREATED, "{book}");
    let id = book["id"].as_i64().unwrap();
    let entitlement = format!("00000000-0000-4000-8000-{id:012x}");

    let (status, init) = call(
        &app,
        "GET",
        &format!("/api/v1/kobo/{device_token}/v1/initialization"),
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{init}");
    assert!(
        init["Resources"]["library_sync"]
            .as_str()
            .unwrap()
            .contains(&device_token),
        "{init}"
    );

    let (status, sync) = call(
        &app,
        "GET",
        &format!("/api/v1/kobo/{device_token}/v1/library/sync"),
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{sync}");
    let arr = sync.as_array().expect("sync array");
    assert!(!arr.is_empty(), "{sync}");
    assert!(
        arr.iter()
            .any(|e| e["NewEntitlement"]["BookEntitlement"]["Id"] == entitlement),
        "{sync}"
    );

    let (status, state) = call(
        &app,
        "PUT",
        &format!("/api/v1/kobo/{device_token}/v1/library/{entitlement}/state"),
        json!({
            "ReadingStates": [{
                "CurrentBookmark": { "ProgressPercent": 55.0 },
                "StatusInfo": { "Status": "Reading" },
                "Statistics": { "SpentReadingMinutes": 12 }
            }]
        }),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{state}");

    let (status, got) = call(
        &app,
        "GET",
        &format!("/api/v1/kobo/{device_token}/v1/library/{entitlement}/state"),
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{got}");
    assert_eq!(got.as_array().unwrap().len(), 1, "{got}");

    let (status, dl) = call(
        &app,
        "GET",
        &format!("/api/v1/kobo/{device_token}/v1/library/{entitlement}/download"),
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{dl}");
}

#[tokio::test]

