#!/usr/bin/env python3
"""Reject commit subjects and pull request titles that are not Conventional Commits."""

import os
import re
import subprocess
import sys

TYPES = (
    "build",
    "chore",
    "ci",
    "docs",
    "feat",
    "fix",
    "perf",
    "refactor",
    "revert",
    "style",
    "test",
)
_CONVENTIONAL = re.compile(
    rf"^({'|'.join(TYPES)})(\([A-Za-z0-9._/-]+\))?!?: [^\s].*$"
)
_ZERO = "0000000000000000000000000000000000000000"
_REDO = """
Rejected. Redo this as a semantic commit:

    type(scope): description

Types: feat, fix, docs, style, refactor, perf, test, build, ci, chore, revert
Scope is optional. Example: feat(emulator): boot the guest image

Do not bypass this with --no-verify.
"""


def is_conventional(subject):
    return _CONVENTIONAL.match(subject) is not None


def is_git_generated(subject):
    return subject.startswith(("Merge ", "fixup! ", "squash! "))


def subject_from_file(path):
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            return line
    return ""


def subjects_in_range(commit_range):
    start = commit_range.split("..", 1)[0]
    if start == _ZERO:
        command = ["git", "log", "-1", "--format=%s", "HEAD"]
    else:
        command = ["git", "log", "--format=%s", commit_range]
    output = subprocess.check_output(command, text=True)
    return [line for line in output.splitlines() if line]


def _note(message):
    print(message, file=sys.stderr)
    if os.environ.get("GITHUB_ACTIONS"):
        print(f"::error::{message}", file=sys.stderr)


def check_subject(kind, subject, allow_git):
    if allow_git and is_git_generated(subject):
        return None
    if is_conventional(subject):
        return None
    message = f"{kind} rejected: {subject}"
    _note(message)
    return message


def main(argv=None):
    args = list(sys.argv[1:] if argv is None else argv)
    path = None
    while args:
        arg = args.pop(0)
        if arg == "--file":
            if not args:
                print("Missing path after --file", file=sys.stderr)
                return 2
            path = args.pop(0)
        else:
            print(f"Unknown argument: {arg}", file=sys.stderr)
            return 2

    errors = []
    if path is not None:
        errors.extend(
            filter(None, [check_subject("Commit subject", subject_from_file(path), True)])
        )
    elif "PR_TITLE" not in os.environ and not os.environ.get("COMMIT_RANGE"):
        print("Pass --file, or set PR_TITLE, or set COMMIT_RANGE.", file=sys.stderr)
        return 2
    else:
        if "PR_TITLE" in os.environ:
            errors.extend(
                filter(
                    None,
                    [check_subject("Pull request title", os.environ["PR_TITLE"], False)],
                )
            )
        if os.environ.get("COMMIT_RANGE"):
            for subject in subjects_in_range(os.environ["COMMIT_RANGE"]):
                errors.extend(
                    filter(None, [check_subject("Commit subject", subject, True)])
                )

    if errors:
        print(_REDO, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
