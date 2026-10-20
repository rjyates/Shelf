/**
 * :8006 is the e-ink panel attached to the guest. Pixels come from the guest
 * framebuffer over VNC; the Panel applies quantize / delay / ghost / flash.
 * Click the panel to type into the guest (RFB under the canvas).
 *
 * shortcut: VNC full-frame ingest until the shell paints only via damage
 * (issue 7 modes still live in damage.js; MCP can publish test frames).
 */
import RFB from "./core/rfb.js";
import { Panel } from "./panel.js";
import { MODES, configurePanel } from "./damage.js";

const screen = document.getElementById("screen");
const canvas = document.getElementById("panel");
const status = document.getElementById("status");
const fullBtn = document.getElementById("full");
const ctx = canvas.getContext("2d", { willReadFrequently: true });

/** Default refresh class for console updates (page-turn / GC16-like). */
let mode = "quality";
let panel = null;
let image = null;
let lastRgba = null;
let busy = false;
let pending = false;
let updates = 0;
let rfb = null;

const FULL_EVERY = 24;

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function websockifyUrl() {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  const path = window.location.pathname.replace(/[^/]*$/, "").replace(/\/$/, "");
  return `${protocol}//${window.location.host}${path}/websockify`;
}

function ensurePanel(width, height) {
  if (panel && panel.width === width && panel.height === height) {
    return;
  }
  panel = new Panel({
    width,
    height,
    grayLevels: MODES.quality.grayLevels,
    ghost: MODES.quality.ghost,
    delayMs: MODES.quality.delayMs,
    dither: true,
    // VGA console is light-on-dark; e-ink ink is dark-on-paper.
    invert: true,
  });
  configurePanel(panel, mode);
  canvas.width = width;
  canvas.height = height;
  image = ctx.createImageData(width, height);
  lastRgba = null;
  updates = 0;
}

function paint() {
  if (!panel || !image) return;
  panel.toImageData(image);
  ctx.putImageData(image, 0, 0);
}

async function showFlash() {
  if (!panel) return;
  panel.ink.fill(0);
  paint();
  await sleep(90);
  panel.ink.fill(1);
  paint();
  await sleep(90);
}

async function ingest(forceFull = false) {
  if (!rfb) return;
  if (busy) {
    pending = true;
    return;
  }
  busy = true;
  try {
    const frame = rfb.getImageData();
    ensurePanel(frame.width, frame.height);
    lastRgba = frame.data;
    updates += 1;

    configurePanel(panel, forceFull ? "full" : mode);
    const wantFull = forceFull || mode === "full" || updates % FULL_EVERY === 0;

    if (wantFull) {
      await sleep(panel.delayMs);
      await showFlash();
      panel.applyFull(lastRgba, { flash: false });
      if (mode === "full") mode = "quality";
    } else {
      await panel.partial(lastRgba);
    }
    status.textContent = `${forceFull || wantFull ? "full" : mode} · click panel to type`;
    paint();
  } catch (err) {
    status.textContent = String(err.message || err);
  } finally {
    busy = false;
    if (pending) {
      pending = false;
      void ingest();
    }
  }
}

function hookFlip() {
  const display = rfb._display;
  const flip = display.flip.bind(display);
  display.flip = function hookedFlip() {
    flip();
    void ingest();
  };
}

fullBtn.addEventListener("click", () => {
  void ingest(true);
});

// Mode shortcuts while developing e-ink refresh classes.
window.addEventListener("keydown", (event) => {
  if (event.target !== document.body && event.target !== document.documentElement) {
    return;
  }
  if (event.key === "1") mode = "quality";
  else if (event.key === "2") mode = "fast";
  else if (event.key === "3") mode = "full";
  else return;
  status.textContent = `mode ${mode}`;
  void ingest(mode === "full");
});

status.textContent = "Connecting to the guest…";
fullBtn.disabled = true;

rfb = new RFB(screen, websockifyUrl());
rfb.scaleViewport = true;
rfb.focusOnClick = true;

rfb.addEventListener("connect", () => {
  status.textContent = "Guest live · e-ink panel";
  fullBtn.disabled = false;
  hookFlip();
  void ingest(true);
});

rfb.addEventListener("disconnect", (event) => {
  fullBtn.disabled = true;
  if (!event.detail.clean) {
    status.textContent = "Disconnected. Refresh when the guest is up.";
  }
});

rfb.addEventListener("securityfailure", () => {
  status.textContent = "VNC security failure.";
});
