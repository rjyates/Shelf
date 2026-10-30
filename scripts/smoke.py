"""Exercise the public health endpoint of a started container without credentials."""
import json
import time
import urllib.error
import urllib.request

for attempt in range(30):
    try:
        with urllib.request.urlopen("http://127.0.0.1:8080/api/v1/health", timeout=2) as response:
            assert json.load(response)["status"] == "ok"
        break
    except (urllib.error.URLError, TimeoutError, ConnectionError):
        if attempt == 29:
            raise
        time.sleep(1)

with urllib.request.urlopen("http://127.0.0.1:8080/", timeout=2) as response:
    assert "Your reading room" in response.read().decode()

try:
    urllib.request.urlopen("http://127.0.0.1:8080/api/v1/books", timeout=2)
    raise AssertionError("Catalogue must require authentication")
except urllib.error.HTTPError as error:
    assert error.code == 401
