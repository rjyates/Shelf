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


def released_platforms(manifest):
    platforms = []
    for item in manifest["manifests"]:
        platform = item.get("platform") or {}
        arch = platform.get("architecture")
        if arch in (None, "unknown"):
            continue
        platforms.append({"os": platform["os"], "architecture": arch, "digest": item["digest"]})
    found = {f"{item['os']}/{item['architecture']}" for item in platforms}
    if found != {"linux/amd64", "linux/arm64"}:
        raise RuntimeError(f"Server image must include linux/amd64 and linux/arm64, found {', '.join(sorted(found)) or 'none'}")
    return platforms


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
        saved = {"amd64": "artifacts/server-image/shelf-image.tar", "arm64": "artifacts/server-image-arm64/shelf-image.tar"}
        missing = [path for path in saved.values() if not Path(path).is_file()]
        if missing:
            raise RuntimeError("Both tested amd64 and arm64 server images are required for a server release.")
        refs = []
        for arch, tar in saved.items():
            ref = f"{image}:{version}-{arch}"
            # Each saved image reuses the shelf-tested tag, so retag it before loading the next one.
            run("docker", "load", "--input", tar)
            run("docker", "tag", source, ref)
            run("docker", "push", ref)
            refs.append(ref)
        for alias in (version, f"{version.split('.')[0]}.{version.split('.')[1]}", "latest"):
            run("docker", "buildx", "imagetools", "create", "-t", f"{image}:{alias}", *refs)
        compose = out / "compose.yaml"
        compose.write_text(Path("compose.yaml").read_text().replace("ghcr.io/rjyates/shelf-server:latest", f"{image}:{version}"), encoding="utf-8")
        run("docker", "pull", f"{image}:{version}")
        # A public repository does not automatically make its first GHCR package public.
        with tempfile.TemporaryDirectory() as config:
            anonymous = subprocess.run(["docker", "--config", config, "pull", f"{image}:{version}"])
        if anonymous.returncode:
            raise RuntimeError("Make the shelf-server GHCR package public in its package settings, then rerun this workflow. No server release has been published.")
        inspected = subprocess.check_output(["docker", "buildx", "imagetools", "inspect", "--raw", f"{image}:{version}"], text=True)
        platforms = released_platforms(json.loads(inspected))
        manifest = out / "image.json"
        manifest.write_text(json.dumps({"image": f"{image}:{version}", "digests": [f"{image}@{item['digest']}" for item in platforms], "source": plan["sha"], "platforms": [f"{item['os']}/{item['architecture']}" for item in platforms]}, indent=2), encoding="utf-8")
        artifacts.extend([compose, manifest])
    else:
        mobile = [Path("artifacts/mobile-android/app-release.apk"), Path("artifacts/mobile-ios/shelf-ios-unsigned.ipa")]
        if not all(path.is_file() for path in mobile):
            raise RuntimeError("Both Android and iPhone artifacts are required for a shared mobile release.")
        artifacts.extend(mobile)
    checksums = out / "SHA256SUMS"
    checksums.write_text("".join(f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n" for path in artifacts), encoding="utf-8")
    artifacts.append(checksums)
    run("gh", "release", "create", tag, "--repo", repo, "--target", plan["sha"], "--title", f"Shelf {component} {version}", "--notes-file", str(changelog), "--latest=false", *map(str, artifacts))


if __name__ == "__main__":
    plan = json.loads(Path("release-plan.json").read_text(encoding="utf-8"))
    for component in ("server", "mobile"):
        if plan["selection"][component]:
            publish(component, plan)
