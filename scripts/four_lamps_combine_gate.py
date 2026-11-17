"""PR gate: full CTC Combine of samples/align-smoke/Four_Lamps → readaloud-verify.

Runs against a live single-service Compose stack (ghost-story in the Shelf image).
Asserts packaging and SMIL quality so Combine regressions fail the PR.
"""
from __future__ import annotations

import json
import os
import re
import statistics
import sys
import time
import urllib.error
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EPUB = ROOT / "samples" / "align-smoke" / "Four_Lamps.epub"
MP3 = ROOT / "samples" / "align-smoke" / "Four_Lamps.mp3"
BASE = os.environ.get("SHELF_GATE_BASE", "http://127.0.0.1:8080/api/v1")
# CTC emit on ~6 min TTS can take a while on CPU; CI job timeout is the hard cap.
POLL_SECS = int(os.environ.get("SHELF_GATE_POLL_SECS", "3600"))
POLL_EVERY = float(os.environ.get("SHELF_GATE_POLL_EVERY", "5"))


def request(path: str, body=None, token=None, filename=None, timeout=60):
    headers = {}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    data = None
    if isinstance(body, (bytes, bytearray)):
        data = body
        headers["Content-Type"] = "application/octet-stream"
        if filename:
            headers["X-Shelf-Filename"] = filename
    elif body is not None:
        data = json.dumps(body).encode()
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(f"{BASE}/{path}", data=data, headers=headers)
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        raw = resp.read()
        return resp.status, (json.loads(raw) if raw else None)


def wait_health(deadline: float) -> None:
    while time.time() < deadline:
        try:
            status, health = request("health", timeout=5)
            if status == 200 and health and health.get("status") == "ok":
                return
        except (urllib.error.URLError, TimeoutError, ConnectionError):
            pass
        time.sleep(2)
    raise SystemExit("shelf health never became ok")


def smil_clip_durations(smil_text: str) -> list[float]:
    """Clip lengths from SMIL clock values (matches Rust parse_smil_clip_durations)."""
    durs = []
    for b, e in re.findall(
        r'clipBegin="([0-9.]+)s"\s+clipEnd="([0-9.]+)s"', smil_text
    ):
        durs.append(float(e) - float(b))
    return durs


def smil_timings_collapsed(durs: list[float], max_under_50ms_frac: float = 0.25) -> bool:
    """RotK-class failure: median near-zero / most clips under 50ms."""
    if not durs:
        return True
    bad = sum(1 for d in durs if d < 0.05)
    return bad / len(durs) > max_under_50ms_frac


def count_sentence_spans(xhtml: str) -> int:
    """Foliate `sentence-N` or ST `{chapterId}-s{N}` sentence span ids."""
    foliate = len(re.findall(r'id="sentence-\d+"', xhtml))
    st = len(re.findall(r'id="[^"\s]+-s\d+"', xhtml))
    return foliate + st


def verify_readaloud(epub_bytes: bytes) -> None:
    with zipfile.ZipFile(__import__("io").BytesIO(epub_bytes)) as z:
        names = z.namelist()
        smil = [n for n in names if n.endswith(".smil")]
        audio = [
            n
            for n in names
            if n.lower().endswith((".mp3", ".m4a", ".m4b", ".ogg", ".opus"))
        ]
        if not smil:
            raise AssertionError("readaloud has no SMIL")
        if not audio:
            raise AssertionError("readaloud has no narration audio")
        opf_name = next(n for n in names if n.endswith(".opf"))
        opf = z.read(opf_name).decode("utf-8", "replace")
        items = re.findall(r"<item\b(?!ref)([^>]*)/?>", opf, re.I)
        mo_item = sum(1 for a in items if "media-overlay=" in a)
        if mo_item == 0:
            raise AssertionError("no manifest media-overlay — Foliate will miss MO")
        if 'property="media:active-class"' not in opf:
            raise AssertionError("OPF missing media:active-class")
        if 'property="media:duration"' not in opf:
            raise AssertionError("OPF missing media:duration")

        durs = []
        prose_wrapped = 0
        xhtml = [n for n in names if n.endswith((".xhtml", ".html", ".htm"))]
        for n in xhtml:
            body = z.read(n).decode("utf-8", "replace")
            prose_wrapped += count_sentence_spans(body)
        for n in smil:
            durs.extend(smil_clip_durations(z.read(n).decode("utf-8", "replace")))
        if not durs:
            raise AssertionError("no SMIL clips")
        bad = sum(1 for d in durs if d < 0.05)
        median = statistics.median(durs)
        print(
            f"smil_files={len(smil)} clips={len(durs)} median={median:.3f}s "
            f"under_50ms={bad}/{len(durs)} sentence_spans={prose_wrapped}"
        )
        if smil_timings_collapsed(durs):
            raise AssertionError(
                f"timings collapsed: {bad}/{len(durs)} clips under 50ms"
            )
        if prose_wrapped < 10:
            raise AssertionError(
                f"expected multi-chapter sentence spans, got {prose_wrapped}"
            )
        if len(durs) < 40:
            raise AssertionError(f"expected ~86 Four Lamps clips, got {len(durs)}")


def _self_check() -> None:
    """Unit twin for SMIL histogram (no live Combine)."""
    healthy = [0.04] + [2.5 + (i % 5) * 0.1 for i in range(1, 90)]
    assert not smil_timings_collapsed(healthy), "healthy Four Lamps-shaped must pass"
    assert statistics.median(healthy) > 1.0
    collapsed = [0.001] * 99 + [1.0]
    assert smil_timings_collapsed(collapsed), "near-all <50ms must fail"
    durs = smil_clip_durations(
        '<audio clipBegin="1.000s" clipEnd="3.500s"/>'
        '<audio clipBegin="3.500s" clipEnd="3.520s"/>'
    )
    assert len(durs) == 2 and abs(durs[0] - 2.5) < 1e-9
    assert count_sentence_spans('id="sentence-0"') == 1
    assert count_sentence_spans('id="ch1-s0" id="ch1-s1"') == 2


def main() -> None:
    if not EPUB.is_file() or not MP3.is_file():
        raise SystemExit(f"missing fixtures: {EPUB} / {MP3}")

    deadline = time.time() + POLL_SECS
    wait_health(min(deadline, time.time() + 120))

    creds = {
        "username": "four-lamps-gate",
        "password": "ci-four-lamps-gate-secret",
    }
    # Fresh Compose project: setup may still be required.
    try:
        _, setup = request("setup")
        if setup and setup.get("setup_required"):
            request("setup", creds)
    except urllib.error.HTTPError as err:
        if err.code not in (400, 409):
            raise

    try:
        _, login = request("login", creds)
    except urllib.error.HTTPError:
        # Project may already have another user; require a dedicated gate env.
        raise SystemExit("login failed — gate expects a fresh DB or matching user")
    if not isinstance(login, dict) or "token" not in login:
        raise SystemExit(f"login response missing token: {login!r}")
    token = login["token"]

    _, epub = request(
        "books/import", EPUB.read_bytes(), token=token, filename="Four_Lamps.epub"
    )
    _, audio = request(
        "books/import", MP3.read_bytes(), token=token, filename="Four_Lamps.mp3"
    )
    if not isinstance(epub, dict) or not isinstance(audio, dict):
        raise SystemExit(f"import failed: epub={epub!r} audio={audio!r}")
    epub_id = epub["id"]
    audio_id = audio["id"]
    print(f"imported epub={epub_id} audio={audio_id}")

    _, work = request(
        "works",
        {"epub_book_id": epub_id, "audio_book_id": audio_id, "start": True},
        token=token,
    )
    if not isinstance(work, dict) or "id" not in work:
        raise SystemExit(f"create work failed: {work!r}")
    work_id = work["id"]
    print(f"work={work_id} status={work.get('status')}")

    ready = None
    while time.time() < deadline:
        _, w = request(f"works/{work_id}", token=token)
        if not isinstance(w, dict):
            raise SystemExit(f"work poll returned non-object: {w!r}")
        status = w.get("status")
        stage = w.get("stage")
        progress = w.get("progress")
        print(f"  status={status} stage={stage} progress={progress}")
        if status == "ready":
            ready = w
            break
        if status == "failed":
            raise SystemExit(f"Combine failed: {w}")
        time.sleep(POLL_EVERY)
    if ready is None:
        raise SystemExit(f"Combine did not finish within {POLL_SECS}s")

    readaloud_id = ready.get("readaloud_book_id")
    if not readaloud_id:
        raise SystemExit(f"ready work missing readaloud: {ready}")

    # Download via authenticated file endpoint.
    req = urllib.request.Request(
        f"{BASE}/books/{readaloud_id}/file",
        headers={"Authorization": f"Bearer {token}"},
    )
    with urllib.request.urlopen(req, timeout=120) as resp:
        ra_bytes = resp.read()
    print(f"readaloud_id={readaloud_id} bytes={len(ra_bytes)}")
    verify_readaloud(ra_bytes)
    print("Four Lamps CTC Combine gate passed.")


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--self-check":
        _self_check()
        print("four_lamps_combine_gate self-check ok")
    else:
        main()
