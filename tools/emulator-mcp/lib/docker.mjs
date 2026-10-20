/**
 * All emulator access goes through docker compose + the container's :8006.
 * No host-side reads of distro/overlay for live state.
 */
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { EMULATOR_DIR, VIEWER_URL, composeProject } from "./paths.mjs";

const execFileAsync = promisify(execFile);

export async function dockerCompose(args, opts = {}) {
  const project = composeProject();
  try {
    const { stdout, stderr } = await execFileAsync(
      "docker",
      ["compose", "-p", project, ...args],
      {
        cwd: EMULATOR_DIR,
        maxBuffer: opts.maxBuffer || 4 * 1024 * 1024,
        ...opts,
      },
    );
    return { ok: true, project, stdout: stdout || "", stderr: stderr || "" };
  } catch (err) {
    return {
      ok: false,
      project,
      stdout: String(err.stdout || ""),
      stderr: String(err.stderr || err.message || err),
    };
  }
}

/** Running qemu service id, or null. */
export async function containerId() {
  const r = await dockerCompose(["ps", "-q", "qemu"]);
  if (!r.ok) return null;
  const id = r.stdout.trim().split("\n").filter(Boolean)[0];
  return id || null;
}

export async function requireContainer() {
  const id = await containerId();
  if (!id) {
    throw new Error(
      `emulator container not running (docker compose -p ${composeProject()} up from emulator/).`,
    );
  }
  return id;
}

export async function viewerReachable(timeoutMs = 2500) {
  const ctrl = new AbortController();
  const t = setTimeout(() => ctrl.abort(), timeoutMs);
  try {
    const res = await fetch(VIEWER_URL, { signal: ctrl.signal });
    const body = await res.text();
    return {
      ok: res.ok,
      status: res.status,
      title: /<title>([^<]*)<\/title>/i.exec(body)?.[1] || null,
      url: VIEWER_URL,
    };
  } catch (err) {
    return {
      ok: false,
      status: 0,
      error: String(err.message || err),
      url: VIEWER_URL,
    };
  } finally {
    clearTimeout(t);
  }
}

/** GET a path served by the container viewer (e.g. guest/damage.json). */
export async function fetchFromViewer(relPath, { as = "text" } = {}) {
  await requireContainer();
  const viewer = await viewerReachable();
  if (!viewer.ok) {
    throw new Error(`viewer not reachable at ${VIEWER_URL}: ${viewer.error || viewer.status}`);
  }
  const base = VIEWER_URL.replace(/\/?$/, "/");
  const url = new URL(relPath.replace(/^\//, ""), base).href;
  const res = await fetch(url, { cache: "no-store" });
  if (!res.ok) {
    throw new Error(`GET ${url} → HTTP ${res.status}`);
  }
  if (as === "json") return res.json();
  if (as === "bytes") return Buffer.from(await res.arrayBuffer());
  return res.text();
}

/**
 * Write damage files inside the container at /shared/run (the 9p share).
 * That is what the guest and the novnc guest/ mount both see.
 */
export async function publishDamageInContainer({
  mode,
  pattern = "page",
  x = 0,
  y = 0,
  w,
  h,
  seq,
}) {
  if (!["quality", "fast", "full"].includes(mode)) {
    throw new Error(`unknown mode ${mode}`);
  }
  if (!["page", "menu"].includes(pattern)) {
    throw new Error(`unknown pattern ${pattern}`);
  }
  await requireContainer();

  const panel_w = 400;
  const panel_h = 300;
  const bw = w ?? (mode === "fast" ? Math.floor(panel_w / 3) : panel_w);
  const bh = h ?? panel_h;
  const nextSeq = seq ?? Date.now() % 1_000_000;
  const meta = {
    v: 1,
    mode,
    x,
    y,
    w: bw,
    h: bh,
    panel_w,
    panel_h,
    seq: nextSeq,
    format: "gray8",
    pixels: "frame.gray",
  };
  const json = JSON.stringify(meta);

  // Copy demo frame + write meta atomically inside the container share.
  const script = [
    "set -e",
    "mkdir -p /shared/run",
    `cp /shared/share/demo/${pattern}.gray /shared/run/frame.gray.tmp`,
    "mv /shared/run/frame.gray.tmp /shared/run/frame.gray",
    `printf '%s\\n' '${json.replace(/'/g, `'\\''`)}' > /shared/run/damage.json.tmp`,
    "mv /shared/run/damage.json.tmp /shared/run/damage.json",
  ].join("\n");

  const r = await dockerCompose(["exec", "-T", "qemu", "sh", "-c", script]);
  if (!r.ok) {
    throw new Error(`docker exec publish failed: ${r.stderr || r.stdout}`);
  }
  return meta;
}

export async function composeLogs(tail = 80) {
  await requireContainer();
  const r = await dockerCompose(["logs", "--no-color", "--tail", String(tail)]);
  if (!r.ok) return { ok: false, project: r.project, text: r.stderr || r.stdout };
  return { ok: true, project: r.project, text: r.stdout + r.stderr };
}

export async function composePs() {
  const r = await dockerCompose(["ps", "--format", "json"]);
  if (!r.ok) return { ok: false, project: r.project, text: r.stderr || r.stdout };
  return {
    ok: true,
    project: r.project,
    text: (r.stdout.trim() || "(no containers)"),
  };
}

export async function composeUp({ build = true } = {}) {
  const args = build
    ? ["up", "-d", "--build", "--remove-orphans"]
    : ["up", "-d", "--remove-orphans"];
  const r = await dockerCompose(args, { maxBuffer: 16 * 1024 * 1024 });
  if (!r.ok) {
    throw new Error(`compose up failed: ${r.stderr || r.stdout}`);
  }
  return { project: r.project, text: (r.stdout + r.stderr).trim() };
}

export async function composeDown({ volumes = false } = {}) {
  const args = ["down", "--remove-orphans"];
  if (volumes) args.push("-v");
  const r = await dockerCompose(args, { maxBuffer: 8 * 1024 * 1024 });
  if (!r.ok) {
    throw new Error(`compose down failed: ${r.stderr || r.stdout}`);
  }
  return { project: r.project, text: (r.stdout + r.stderr).trim() };
}

/**
 * Touch overlay paths inside the container so oseink-devd reloads the shell.
 * Paths are under /shared (host distro/overlay).
 */
export async function overlayTouch(targets = ["bin/oseink-shell"]) {
  await requireContainer();
  const list = (Array.isArray(targets) ? targets : [targets]).map((t) =>
    t.replace(/^\//, "").replace(/^shared\//, "").replace(/^oseink\//, ""),
  );
  if (!list.length) throw new Error("overlay_touch needs at least one path");
  const touches = list.map((t) => `touch "/shared/${t}"`).join(" && ");
  const r = await dockerCompose([
    "exec",
    "-T",
    "qemu",
    "sh",
    "-c",
    `set -e; ${touches}`,
  ]);
  if (!r.ok) {
    throw new Error(`overlay_touch failed: ${r.stderr || r.stdout}`);
  }
  return { touched: list.map((t) => `/shared/${t}`) };
}
