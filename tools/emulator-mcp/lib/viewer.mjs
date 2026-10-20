/**
 * Capture and drive the live :8006 e-ink viewer through the Docker-served page.
 */
import { chromium } from "playwright";
import { requireContainer, viewerReachable } from "./docker.mjs";
import { VIEWER_URL } from "./paths.mjs";

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function withPage(fn, { waitMs = 1500 } = {}) {
  await requireContainer();
  const health = await viewerReachable();
  if (!health.ok) {
    throw new Error(`viewer not reachable at ${VIEWER_URL}: ${health.error || health.status}`);
  }

  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage({ viewport: { width: 960, height: 800 } });
    await page.goto(VIEWER_URL, { waitUntil: "domcontentloaded", timeout: 30000 });
    // Let RFB connect and the panel paint at least one frame.
    await sleep(waitMs);
    return await fn(page);
  } finally {
    await browser.close();
  }
}

/** PNG of what a human sees at :8006 (bezel + e-ink panel). */
export async function viewerScreenshotPng() {
  return withPage(async (page) => {
    const status = (await page.locator("#status").textContent().catch(() => "")) || "";
    const png = await page.locator(".bezel").screenshot({ type: "png" });
    return { png: Buffer.from(png), status: status.trim(), url: VIEWER_URL };
  });
}

/** Click the panel (focuses RFB) and type into the guest. */
export async function guestType(text, { submit = false } = {}) {
  if (typeof text !== "string" || !text.length) {
    throw new Error("guest_type requires non-empty text");
  }
  return withPage(async (page) => {
    await page.locator("#screen").click({ timeout: 10000 });
    await page.keyboard.type(text, { delay: 20 });
    if (submit) await page.keyboard.press("Enter");
    await sleep(400);
    const png = await page.locator(".bezel").screenshot({ type: "png" });
    return { png: Buffer.from(png), typed: text, submit: Boolean(submit) };
  }, { waitMs: 2000 });
}

/** Click Full refresh on the viewer. */
export async function viewerFullRefresh() {
  return withPage(async (page) => {
    await page.locator("#full").click({ timeout: 10000 });
    await sleep(800);
    const status = (await page.locator("#status").textContent().catch(() => "")) || "";
    const png = await page.locator(".bezel").screenshot({ type: "png" });
    return { png: Buffer.from(png), status: status.trim() };
  });
}
