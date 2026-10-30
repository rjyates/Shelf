"""Verify the disposable CI Compose deployment and its named-volume persistence."""
from pathlib import Path
import json
import os
import subprocess
import time
import urllib.error
import urllib.request


def request(path, body=None, token=None, raw=False):
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    if isinstance(body, bytes):
        payload = body
        headers["Content-Type"] = "application/epub+zip"
    else:
        payload = json.dumps(body).encode() if body is not None else None
    value = urllib.request.Request(f"http://127.0.0.1:8080/api/v1/{path}", data=payload, headers=headers)
    with urllib.request.urlopen(value, timeout=5) as response:
        content = response.read()
        return content if raw else json.loads(content) if content else None


def ready():
    for attempt in range(30):
        try:
            assert request("health")["status"] == "ok"
            return
        except (urllib.error.URLError, TimeoutError, ConnectionError):
            if attempt == 29:
                raise
            time.sleep(1)


if __name__ == "__main__":
    if os.environ.get("GITHUB_ACTIONS") != "true":
        raise SystemExit("This test is only for the disposable GitHub Actions Compose project.")
    ready()
    credentials = {"username": "ci-reader", "password": "disposable-ci-test-password"}
    assert request("setup")["setup_required"] is True
    request("setup", credentials)
    token = request("login", credentials)["token"]
    book = request("books", {"title": "CI sample", "author": "Test fixture", "total_pages": 200}, token)
    request(f"books/{book['id']}/progress", {"pages_read": 75}, token)
    epub_bytes = Path("protocol/fixtures/sample.epub").read_bytes()
    epub = request("books/import", epub_bytes, token)
    assert epub["duplicate"] is False
    assert request("books/import", epub_bytes, token)["duplicate"] is True
    request(f"books/{epub['id']}/location", {"position": "epubcfi(/6/2!/4/2)", "percentage": 0.4}, token)
    subprocess.run(["docker", "compose", "-p", "shelf-ci", "up", "-d", "--force-recreate", "--pull", "never"], check=True)
    ready()
    assert request("setup")["setup_required"] is False
    books = request("books", token=token)
    assert len(books) == 2
    assert next(item for item in books if item["id"] == book["id"])["pages_read"] == 75
    imported = next(item for item in books if item["id"] == epub["id"])
    assert imported["percentage"] == 0.4 and imported["total_pages"] is None
    assert request(f"books/{epub['id']}/file", token=token, raw=True) == epub_bytes
    assert request("stats", token=token)["pages_read"] == 75
    print("Compose account, manual progress, EPUB bytes, and reader position survived container recreation.")
