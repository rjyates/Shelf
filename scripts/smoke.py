"""Exercise the public health endpoint of a started container without credentials."""
import json
import time
import urllib.error
import urllib.request

# Entrypoint may download the CTC model (~1.2GB) before shelf-server listens.
# Matches deploy/Dockerfile HEALTHCHECK start-period.
for attempt in range(180):
    try:
        with urllib.request.urlopen("http://127.0.0.1:8080/api/v1/health", timeout=2) as response:
            assert json.load(response)["status"] == "ok"
        break
    except (urllib.error.URLError, TimeoutError, OSError):
        if attempt == 179:
            raise
        time.sleep(1)

with urllib.request.urlopen("http://127.0.0.1:8080/", timeout=2) as response:
    assert "Your reading room" in response.read().decode()

try:
    urllib.request.urlopen("http://127.0.0.1:8080/api/v1/books", timeout=2)
    raise AssertionError("Catalogue must require authentication")
except urllib.error.HTTPError as error:
    assert error.code == 401
