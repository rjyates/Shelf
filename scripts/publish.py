"""Publish already-tested artifacts, without rebuilding or writing to main."""
import hashlib
import json
import os
import subprocess
import tempfile
from pathlib import Path
from ci import notes


def run(*args):
    subprocess.run(args, check=True)


def publish(component, plan):
    item = plan["components"][component]
    tag, version = item["tag"], item["version"]
    repo = os.environ["GITHUB_REPOSITORY"]
    # Retrying an already-published exact revision must not create another version.
    existing = subprocess.run(["gh", "release", "view", tag, "--repo", repo], capture_output=True)
    if existing.returncode == 0:
        print(f"{tag} already exists; leaving it unchanged.")
        return
    out = Path("release-out") / component
    out.mkdir(parents=True, exist_ok=True)
    changelog = out / "CHANGELOG.md"
    changelog.write_text(notes(plan, component), encoding="utf-8")
    artifacts = [changelog]
    if component == "server":
        image = f"ghcr.io/{repo.lower()}-server"
        source = f"shelf-tested:{plan['sha']}"
        run("docker", "load", "--input", "artifacts/server-image/shelf-image.tar")
        for alias in (version, f"{version.split('.')[0]}.{version.split('.')[1]}", "latest"):
            run("docker", "tag", source, f"{image}:{alias}")
            run("docker", "push", f"{image}:{alias}")
        compose = out / "compose.yaml"
        compose.write_text(Path("compose.yaml").read_text().replace("ghcr.io/rjyates/shelf-server:latest", f"{image}:{version}"), encoding="utf-8")
        run("docker", "pull", f"{image}:{version}")
        # A public repository does not automatically make its first GHCR package public.
        with tempfile.TemporaryDirectory() as config:
            anonymous = subprocess.run(["docker", "--config", config, "pull", f"{image}:{version}"])
        if anonymous.returncode:
            raise RuntimeError("Make the shelf-server GHCR package public in its package settings, then rerun this workflow. No server release has been published.")
        inspection = subprocess.check_output(["docker", "image", "inspect", f"{image}:{version}", "--format", "{{json .RepoDigests}}"], text=True)
        manifest = out / "image.json"
        manifest.write_text(json.dumps({"image": f"{image}:{version}", "digests": json.loads(inspection), "source": plan["sha"], "platform": "linux/amd64"}, indent=2), encoding="utf-8")
        artifacts.extend([compose, manifest])
    else:
        artifacts.extend(sorted(Path("artifacts").glob("mobile-*/*")))
        if len(artifacts) == 1:
            raise RuntimeError("No mobile artifacts found; refusing to publish.")
    checksums = out / "SHA256SUMS"
    checksums.write_text("".join(f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n" for path in artifacts), encoding="utf-8")
    artifacts.append(checksums)
    run("gh", "release", "create", tag, "--repo", repo, "--target", plan["sha"], "--title", f"Shelf {component} {version}", "--notes-file", str(changelog), "--latest=false", *map(str, artifacts))


if __name__ == "__main__":
    plan = json.loads(Path("release-plan.json").read_text(encoding="utf-8"))
    for component in ("server", "mobile"):
        if plan["selection"][component]:
            publish(component, plan)
