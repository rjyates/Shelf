/**
 * :8006 paints a guest damage frame (quality / fast / full, a rect, gray8).
 * Click the panel to type into the guest console. VNC stays under the canvas
 * for that input.
 *
 * shortcut: click-to-type is still the VGA console, drop it when seatd owns the seat.
 */
import RFB from "./core/rfb.js";
import { Panel } from "./panel.js";
import { applyDamage, parseDamage } from "./damage.js";

const screen = document.getElementById("screen");
const canvas = document.getElementById("panel");
const status = document.getElementById("status");
const fullBtn = document.getElementById("full");
const ctx = canvas.getContext("2d", { willReadFrequently: true });

let panel = null;
let image = null;
let busy = false;
let pending = null;
let lastSeq = null;
let shown = null;
let frame = null;
let modeOverride = null;
let rfb = null;

function websockifyUrl() {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  const path = window.location.pathname.replace(/[^/]*$/, "").replace(/\/$/, "");
  return `${protocol}//${window.location.host}${path}/websockify`;
}

function fitStack(width, height) {
  const stage = document.querySelector(".stage");
  if (!stage || !(width > 0) || !(height > 0)) return;
  const nextW = String(width);
  const nextH = String(height);
  if (
    stage.style.getPropertyValue("--panel-w") === nextW &&
    stage.style.getPropertyValue("--panel-h") === nextH
  ) {
    return;
  }
  stage.style.setProperty("--panel-w", nextW);
  stage.style.setProperty("--panel-h", nextH);
  window.dispatchEvent(new Event("resize"));
}

function ensurePanel(width, height) {
  fitStack(width, height);
  if (panel && panel.width === width && panel.height === height) {
    return false;
  }
  panel = new Panel({
    width,
    height,
    dither: true,
    invert: false,
  });
  panel.paint = paint;
  canvas.width = width;
  canvas.height = height;
  image = ctx.createImageData(width, height);
  return true;
}

function paint() {
  if (!panel || !image) return;
  panel.toImageData(image);
  ctx.putImageData(image, 0, 0);
}

async function present(msg, pixels) {
  const fresh = ensurePanel(msg.panel_w, msg.panel_h);
  paint();
  let use = modeOverride ? { ...msg, mode: modeOverride } : { ...msg };
  // The latest message may be a clock-bar rect. The gray buffer is the whole desktop.
  if (fresh && use.mode !== "full") {
    use = { ...use, x: 0, y: 0, w: use.panel_w, h: use.panel_h };
  }
  await applyDamage(panel, use, pixels);
  status.textContent = `${use.mode} · ${use.panel_w}×${use.panel_h} · click panel to type`;
  paint();
  fullBtn.disabled = false;
}

async function pump() {
  if (busy || !pending) return;
  const job = pending;
  pending = null;
  busy = true;
  try {
    await present(job.msg, job.pixels);
  } catch (err) {
    status.textContent = String(err.message || err);
  } finally {
    busy = false;
    if (pending) void pump();
  }
}

function queue(msg, pixels) {
  pending = { msg, pixels };
  void pump();
}

async function poll() {
  try {
    const res = await fetch("/guest/damage.json", { cache: "no-store" });
    if (!res.ok) return;
    const parsed = parseDamage(await res.json());
    if (parsed.seq === lastSeq && frame) return;
    const pixRes = await fetch(`/guest/${parsed.pixels}?seq=${parsed.seq}`, {
      cache: "no-store",
    });
    if (!pixRes.ok) return;
    const buf = new Uint8Array(await pixRes.arrayBuffer());
    const need = parsed.panel_w * parsed.panel_h;
    if (buf.length < need) return;
    frame = buf.subarray(0, need);
    lastSeq = parsed.seq;
    modeOverride = null;
    shown = parsed;
    queue(parsed, frame);
  } catch {
    // No frame yet, or the file was mid-write.
  }
}

fullBtn.addEventListener("click", () => {
  if (!shown || !frame) return;
  modeOverride = "full";
  queue(shown, frame);
});

// Reapply the current frame as another refresh class.
window.addEventListener("keydown", (event) => {
  if (event.target !== document.body && event.target !== document.documentElement) {
    return;
  }
  if (event.key === "1") modeOverride = "quality";
  else if (event.key === "2") modeOverride = "fast";
  else if (event.key === "3") modeOverride = "full";
  else return;
  if (shown && frame) queue(shown, frame);
});

status.textContent = "Waiting for a frame.";
fullBtn.disabled = true;
setInterval(() => void poll(), 300);
void poll();

rfb = new RFB(screen, websockifyUrl());
rfb.scaleViewport = true;
rfb.focusOnClick = true;

rfb.addEventListener("disconnect", (event) => {
  if (!event.detail.clean && !shown) {
    status.textContent = "Disconnected. Refresh when the guest is up.";
  }
});

rfb.addEventListener("securityfailure", () => {
  status.textContent = "VNC security failure.";
});

function note(text) {
  const el = document.getElementById("profile-note");
  if (el) el.textContent = text;
}

function markProfile(current, choices) {
  const colorOn = current.color === "kaleido" || current.color === "full";
  for (const btn of document.querySelectorAll("[data-color]")) {
    const on = btn.dataset.color === "color" ? colorOn : !colorOn;
    btn.setAttribute("aria-pressed", on ? "true" : "false");
  }
  const flags = new Set(choices.input[current.input] || []);
  for (const btn of document.querySelectorAll("[data-input]")) {
    btn.setAttribute("aria-pressed", flags.has(btn.dataset.input) ? "true" : "false");
  }
  const width = document.querySelector("#panel-w");
  const height = document.querySelector("#panel-h");
  if (width && Number.isInteger(current.width)) width.value = String(current.width);
  if (height && Number.isInteger(current.height)) height.value = String(current.height);
  fitStack(current.width, current.height);
}

function panelSize() {
  const width = Number(document.querySelector("#panel-w").value);
  const height = Number(document.querySelector("#panel-h").value);
  if (!Number.isInteger(width) || !Number.isInteger(height)) return null;
  if (width < 1 || height < 1 || width > 4096 || height > 4096) return null;
  return { width, height };
}

function selection() {
  const colorBtn = document.querySelector('[data-color][aria-pressed="true"]');
  const input = [];
  for (const btn of document.querySelectorAll('[data-input][aria-pressed="true"]')) {
    input.push(btn.dataset.input);
  }
  const size = panelSize();
  if (!size) return null;
  return { color: colorBtn ? colorBtn.dataset.color : "", input, ...size };
}

async function initProfile() {
  let choices;
  let current;
  try {
    choices = await (await fetch("/profile-choices.json")).json();
    current = await (await fetch("/profile")).json();
    markProfile(current, choices);
  } catch {
    return;
  }

  let saving = false;
  async function save() {
    if (saving) return;
    const body = selection();
    if (!body) {
      note("Width and height are whole numbers from 1 to 4096.");
      return;
    }
    fitStack(body.width, body.height);
    saving = true;
    try {
      const res = await fetch("/profile", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      if (!res.ok) throw new Error("Could not save the profile.");
      current = await (await fetch("/profile")).json();
      markProfile(current, choices);
      note("Saved the profile.");
    } catch (err) {
      note(err.message || "Could not save the profile.");
    } finally {
      saving = false;
    }
  }

  for (const btn of document.querySelectorAll("[data-color]")) {
    btn.addEventListener("click", () => {
      for (const other of document.querySelectorAll("[data-color]")) {
        other.setAttribute("aria-pressed", other === btn ? "true" : "false");
      }
      void save();
    });
  }

  for (const btn of document.querySelectorAll("[data-input]")) {
    btn.addEventListener("click", () => {
      const turningOff = btn.getAttribute("aria-pressed") === "true";
      const pressed = document.querySelectorAll('[data-input][aria-pressed="true"]').length;
      if (turningOff && pressed === 1) {
        note("Pick at least one.");
        return;
      }
      btn.setAttribute("aria-pressed", turningOff ? "false" : "true");
      void save();
    });
  }

  for (const id of ["panel-w", "panel-h"]) {
    document.getElementById(id).addEventListener("change", () => void save());
  }
}

void initProfile();
