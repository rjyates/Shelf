import os
import subprocess
import textwrap
from pathlib import Path

import check_semantic
import pytest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "check_semantic.py"


@pytest.mark.parametrize(
    "subject",
    [
        "feat: add a viewer",
        "feat(emulator): boot the guest image",
        "fix(shell)!: drop the old profile field",
        "chore(release): 1.0.0 [skip ci]",
    ],
)
def test_conventional_subjects(subject):
    assert check_semantic.is_conventional(subject)


@pytest.mark.parametrize(
    "subject",
    [
        "add a viewer",
        "Feat: add a viewer",
        "feat:",
        "feat: ",
        "feat(emulator):",
    ],
)
def test_rejects_subjects_outside_the_convention(subject):
    assert not check_semantic.is_conventional(subject)


def test_commit_subjects_allow_git_generated_lines():
    assert check_semantic.check_subject("Commit subject", "Merge pull request #9", True) is None
    assert check_semantic.check_subject("Commit subject", "fixup! feat: boot", True) is None
    assert check_semantic.check_subject("Commit subject", "squash! feat: boot", True) is None


def test_pull_request_titles_reject_git_generated_lines():
    assert check_semantic.check_subject("Pull request title", "Merge pull request #9", False)


def test_file_mode_reads_the_subject_and_ignores_comments(tmp_path):
    message = tmp_path / "COMMIT_EDITMSG"
    message.write_text(
        textwrap.dedent(
            """\
            feat(emulator): boot the guest image

            # Please enter the commit message
            """
        ),
        encoding="utf-8",
    )
    assert check_semantic.main(["--file", str(message)]) == 0


def test_file_mode_rejects_a_bad_subject(tmp_path):
    message = tmp_path / "COMMIT_EDITMSG"
    message.write_text("boot the guest\n", encoding="utf-8")
    assert check_semantic.main(["--file", str(message)]) == 1


def test_range_of_a_new_branch_checks_head(tmp_path, monkeypatch):
    subprocess.run(["git", "init"], cwd=tmp_path, check=True, capture_output=True)
    (tmp_path / "README").write_text("x\n", encoding="utf-8")
    subprocess.run(["git", "add", "README"], cwd=tmp_path, check=True)
    subprocess.run(
        [
            "git",
            "-c",
            "user.email=dev@example.com",
            "-c",
            "user.name=Dev",
            "commit",
            "-m",
            "feat: start",
        ],
        cwd=tmp_path,
        check=True,
        capture_output=True,
    )
    monkeypatch.chdir(tmp_path)
    monkeypatch.setenv("COMMIT_RANGE", f"{check_semantic._ZERO}..{'a' * 40}")
    monkeypatch.delenv("PR_TITLE", raising=False)
    assert check_semantic.main([]) == 0


def test_cli_requires_a_source(monkeypatch):
    monkeypatch.delenv("PR_TITLE", raising=False)
    monkeypatch.delenv("COMMIT_RANGE", raising=False)
    assert check_semantic.main([]) == 2


def test_unknown_argument():
    assert check_semantic.main(["--nope"]) == 2


def test_script_entrypoint_matches_the_hook(tmp_path):
    message = tmp_path / "COMMIT_EDITMSG"
    message.write_text("docs: add a pull request template\n", encoding="utf-8")
    result = subprocess.run(
        ["python3", str(SCRIPT), "--file", str(message)],
        check=False,
        capture_output=True,
        text=True,
        env={**os.environ, "PR_TITLE": "this would fail", "COMMIT_RANGE": "ignored"},
    )
    assert result.returncode == 0
