"""Verify the disposable CI Compose deployment and its named-volume persistence."""
from pathlib import Path
import json
import os
import shutil
import subprocess
import tempfile
import time
import urllib.error
import urllib.request

IMAGE = "ghcr.io/rjyates/shelf-server:latest"


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


def host_directory_mount():
    """A root-owned folder mounted on /data must accept a new database."""
    parent = tempfile.mkdtemp(prefix="shelf-bind-")
    name = "shelf-bind-ci"
    try:
        subprocess.run(
            ["docker", "run", "--rm", "--entrypoint", "mkdir", "-v", f"{parent}:/out", IMAGE, "-p", "/out/data"],
            check=True,
        )
        data = str(Path(parent) / "data")
        subprocess.run(["docker", "rm", "-f", name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        subprocess.run(
            ["docker", "run", "-d", "--name", name, "-p", "127.0.0.1:18080:8080", "-v", f"{data}:/data", IMAGE],
            check=True,
        )
        for attempt in range(30):
            try:
                with urllib.request.urlopen("http://127.0.0.1:18080/api/v1/health", timeout=2) as response:
                    assert json.load(response)["status"] == "ok"
                break
            except (urllib.error.URLError, TimeoutError, ConnectionError, AssertionError):
                if attempt == 29:
                    logs = subprocess.run(["docker", "logs", name], capture_output=True, text=True)
                    raise AssertionError(f"bind-mounted /data did not become healthy\n{logs.stdout}\n{logs.stderr}")
                time.sleep(1)
        assert os.stat(Path(data) / "shelf.db").st_uid == 10001
        status = subprocess.check_output(["docker", "exec", name, "cat", "/proc/1/status"], text=True)
        uid = next(line.split()[1] for line in status.splitlines() if line.startswith("Uid:"))
        assert uid == "10001"
        print("Root-owned host directory mount created /data/shelf.db as uid 10001.")
    finally:
        subprocess.run(["docker", "rm", "-f", name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        subprocess.run(
            ["docker", "run", "--rm", "--entrypoint", "chown", "-v", f"{parent}:/data", IMAGE, "-R", f"{os.getuid()}:{os.getgid()}", "/data"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        shutil.rmtree(parent, ignore_errors=True)


if __name__ == "__main__":
    if os.environ.get("GITHUB_ACTIONS") != "true":
        raise SystemExit("This test is only for the disposable GitHub Actions Compose project.")
    host_directory_mount()
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
