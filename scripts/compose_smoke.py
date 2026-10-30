"""Verify the disposable CI Compose deployment and its named-volume persistence."""
import json
import os
import subprocess
import time
import urllib.error
import urllib.request


def request(path, body=None, token=None):
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    payload = json.dumps(body).encode() if body is not None else None
    value = urllib.request.Request(f"http://127.0.0.1:8080/api/v1/{path}", data=payload, headers=headers)
    with urllib.request.urlopen(value, timeout=5) as response:
        content = response.read()
        return json.loads(content) if content else None


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
    subprocess.run(["docker", "compose", "-p", "shelf-ci", "up", "-d", "--force-recreate", "--pull", "never"], check=True)
    ready()
    assert request("setup")["setup_required"] is False
    books = request("books", token=token)
    assert len(books) == 1 and books[0]["pages_read"] == 75
    assert request("stats", token=token)["pages_read"] == 75
    print("Compose setup, account, session, and progress survived container recreation.")
