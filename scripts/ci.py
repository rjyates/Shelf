"""Component selection, Conventional Commit checks, and independent release plans."""
import argparse
import json
import os
import re
import subprocess
from pathlib import Path

CONVENTIONAL = re.compile(r"^(feat|fix|perf|build|ci|docs|style|refactor|test|chore|revert)(\([A-Za-z0-9._/-]+\))?(!)?: \S.*$")
COMPONENTS = ("server", "mobile")


def git(*args):
    return subprocess.check_output(["git", *args], text=True, encoding="utf-8").strip()


def affected(paths):
    result = {"server": False, "mobile": False, "android": False, "ios": False}
    for path in paths:
        if path == ".flutter-version":
            result.update(mobile=True, android=True, ios=True)
            continue
        if path in {"tests/web.test.mjs", "tests/reader.test.mjs", "scripts/smoke.py", "scripts/compose_smoke.py"}:
            result["server"] = True
            continue
        if path.startswith(("server/", "web/", "deploy/")) or path in {"compose.yaml", ".dockerignore", "rust-toolchain.toml", "LICENSE"}:
            result["server"] = True
        if path.startswith("mobile/"):
            if path in {"mobile/README.md", "mobile/.gitignore"}:
                continue
            result["mobile"] = True
            result["android"] = result["ios"] = True
        if path.startswith(("protocol/", "scripts/", "tests/", ".github/workflows/")):
            result = dict.fromkeys(result, True)
    return result


def changed(base, head, merge_base=False):
    if not base or set(base) == {"0"}:
        return [path for path in git("ls-tree", "-r", "-z", "--name-only", head).split("\0") if path]
    # A rename affects both the source and destination components.
    return [path for path in git("diff", "--no-renames", "-z", "--name-only", "--diff-filter=ACDMRT", f"{base}{'...' if merge_base else '..'}{head}").split("\0") if path]


def last_tag(component, head):
    tags = git("tag", "--merged", head, "--list", f"{component}-v*").splitlines()
    valid = [(tuple(map(int, match.groups())), tag) for tag in tags
             if (match := re.fullmatch(rf"{component}-v(\d+)\.(\d+)\.(\d+)", tag))]
    return max(valid)[1] if valid else None


def severity(message):
    if re.match(r"^[a-z]+(?:\([^\n]+\))?!:", message) or re.search(r"^BREAKING[ -]CHANGE:", message, re.M):
        return 2
    return 1 if message.startswith("feat:") or message.startswith("feat(") else 0


def bump(previous, messages):
    if previous is None:
        return "0.1.0"
    major, minor, patch = map(int, previous.split("-v")[1].split("."))
    impact = max(map(severity, messages), default=0)
    if impact == 2:
        major, minor, patch = major + 1, 0, 0
    elif impact == 1:
        minor, patch = minor + 1, 0
    else:
        patch += 1
    return f"{major}.{minor}.{patch}"


def commits(component, base, head):
    revision = f"{base}..{head}" if base else head
    result = []
    for sha in git("rev-list", "--first-parent", "--reverse", revision).splitlines():
        parents = git("rev-list", "--parents", "-n", "1", sha).split()
        paths = changed(parents[1] if len(parents) > 1 else None, sha)
        if affected(paths)[component]:
            result.append({"sha": sha, "message": git("show", "-s", "--format=%B", sha)})
    return result


def check_semantics(base, head, title=None):
    errors = []
    impact = 0
    if title is not None and not CONVENTIONAL.fullmatch(title):
        errors.append("PR title must use type(scope): description.")
    revision = f"{base}..{head}" if base and set(base) != {"0"} else head
    for sha in git("rev-list", revision).splitlines():
        body = git("show", "-s", "--format=%B", sha)
        subject = body.splitlines()[0] if body else ""
        impact = max(impact, severity(body))
        if not CONVENTIONAL.fullmatch(subject):
            errors.append(f"Commit {sha[:8]} must use type(scope): description.")
    if title is not None and severity(title) < impact:
        errors.append("PR title must preserve the highest feature or breaking-change impact of its commits; use feat for features and ! for breaking changes.")
    if errors:
        raise ValueError("\n".join(errors))


def make_plan(base, head, pull_request=False):
    selection = affected(changed(base, head, pull_request))
    plan = {"sha": git("rev-parse", head), "selection": selection, "components": {}}
    for component in COMPONENTS:
        tag = last_tag(component, head)
        entries = commits(component, tag, head)
        # Main includes any component work not yet released, including a failed prior run.
        if not pull_request:
            selection[component] = bool(entries)
            pending = affected(changed(tag, head)) if entries else dict.fromkeys(selection, False)
            if component == "mobile":
                selection["android"] = pending["android"]
                selection["ios"] = pending["ios"]
        version = bump(tag, [entry["message"] for entry in entries])
        plan["components"][component] = {"version": version, "tag": f"{component}-v{version}", "previous": tag, "commits": entries}
    return plan


def notes(plan, component):
    item = plan["components"][component]
    lines = [f"# Shelf {component} {item['version']}", "", f"Source revision: `{plan['sha']}`", "", "## Changes", ""]
    for entry in item["commits"]:
        lines.append(f"- {entry['message'].splitlines()[0]} ({entry['sha'][:8]})")
    if component == "mobile":
        lines.extend(["", "Android and iPhone artifacts share this mobile version. Android uses development signing. The iOS archive is unsigned and cannot be installed directly or uploaded to the App Store."])
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=["plan", "semantic", "notes"])
    parser.add_argument("--base", default=os.environ.get("BASE_SHA", ""))
    parser.add_argument("--head", default=os.environ.get("HEAD_SHA", "HEAD"))
    parser.add_argument("--pr", action="store_true")
    parser.add_argument("--component", choices=COMPONENTS)
    args = parser.parse_args()
    if args.command == "semantic":
        check_semantics(args.base, args.head, os.environ.get("PR_TITLE"))
    elif args.command == "plan":
        plan = make_plan(args.base, args.head, args.pr)
        Path("release-plan.json").write_text(json.dumps(plan, indent=2), encoding="utf-8")
        if output := os.environ.get("GITHUB_OUTPUT"):
            flutter = Path(".flutter-version").read_text(encoding="utf-8").strip()
            if not re.fullmatch(r"\d+\.\d+\.\d+", flutter):
                raise ValueError(".flutter-version must contain an exact Flutter version")
            with open(output, "a", encoding="utf-8") as handle:
                handle.write(f"flutter_version={flutter}\n")
                for key, value in plan["selection"].items():
                    handle.write(f"{key}={str(value).lower()}\n")
                for component, item in plan["components"].items():
                    handle.write(f"{component}_version={item['version']}\n")
        print(json.dumps(plan["selection"]))
    else:
        if not args.component:
            parser.error("notes requires --component")
        print(notes(json.loads(Path("release-plan.json").read_text(encoding="utf-8")), args.component))


if __name__ == "__main__":
    main()
