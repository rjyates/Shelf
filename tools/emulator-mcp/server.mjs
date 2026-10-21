#!/usr/bin/env node
/**
 * oseink emulator MCP — see/drive the e-ink panel via the Docker stack.
 * Launch via run.sh so every host (Cursor, Claude Code, VS Code, …) shares one entry.
 *
 * stdout is JSON-RPC. Log only with console.error.
 */
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";
import { MODES } from "../../emulator/viewer/damage.js";
import { captureVga, readSerial } from "./lib/console.mjs";
import {
  composeDown,
  composeLogs,
  composePs,
  composeUp,
  overlayTouch,
  publishDamageInContainer,
  requireContainer,
  viewerReachable,
} from "./lib/docker.mjs";
import { readDamageMetaFromContainer, renderDamagePng } from "./lib/panel.mjs";
import { ROOT, VIEWER_URL, composeProject } from "./lib/paths.mjs";
import {
  guestType,
  viewerFullRefresh,
  viewerScreenshotPng,
} from "./lib/viewer.mjs";

function text(s) {
  return { content: [{ type: "text", text: s }] };
}

function errorText(s) {
  return { content: [{ type: "text", text: s }], isError: true };
}

function imageResult(summary, png) {
  return {
    content: [
      { type: "text", text: summary },
      { type: "image", data: png.toString("base64"), mimeType: "image/png" },
    ],
  };
}

const server = new McpServer({
  name: "oseink-emulator",
  version: "0.4.0",
});

server.tool(
  "panel_screenshot",
  "PRIMARY: screenshot the live e-ink viewer at :8006 (what a human sees). Requires the qemu container. Prefer this over guessing from logs.",
  {},
  async () => {
    try {
      const { png, status, url } = await viewerScreenshotPng();
      return imageResult(`viewer ${url}\nstatus=${status}`, png);
    } catch (err) {
      return errorText(`panel_screenshot failed: ${err.message || err}`);
    }
  },
);

server.tool(
  "damage_render",
  "Render a damage frame from :8006/guest/ through the panel simulator (mode test path). Use damage_publish first if no frame is present.",
  {
    mode: z.enum(["quality", "fast", "full"]).optional(),
    force_full: z.boolean().optional(),
  },
  async ({ mode, force_full }) => {
    try {
      const { png, meta, source } = await renderDamagePng({
        mode,
        forceFull: Boolean(force_full),
      });
      const summary = [
        `source=${source}`,
        `mode=${meta.mode} seq=${meta.seq}`,
        `rect=${meta.x},${meta.y} ${meta.w}x${meta.h}`,
        `knobs=${JSON.stringify(MODES[meta.mode])}`,
      ].join("\n");
      return imageResult(summary, png);
    } catch (err) {
      return errorText(`damage_render failed: ${err.message || err}`);
    }
  },
);

server.tool(
  "panel_status",
  "Container + viewer health, optional live damage.json, mode presets.",
  {},
  async () => {
    try {
      const id = await requireContainer();
      const viewer = await viewerReachable();
      let meta = null;
      let metaError = null;
      if (viewer.ok) {
        try {
          meta = await readDamageMetaFromContainer();
        } catch (err) {
          metaError = String(err.message || err);
        }
      }
      return text(
        JSON.stringify(
          {
            root: ROOT,
            compose_project: composeProject(),
            container_id: id,
            viewer,
            meta,
            meta_error: metaError,
            modes: MODES,
            viewer_url: VIEWER_URL,
          },
          null,
          2,
        ),
      );
    } catch (err) {
      return errorText(`panel_status failed: ${err.message || err}`);
    }
  },
);

server.tool(
  "damage_publish",
  "Write a quality/fast/full test frame inside the container (/shared/run) via docker exec.",
  {
    mode: z.enum(["quality", "fast", "full"]),
    pattern: z.enum(["page", "menu"]).optional(),
  },
  async ({ mode, pattern }) => {
    try {
      const meta = await publishDamageInContainer({
        mode,
        pattern: pattern || (mode === "fast" ? "menu" : "page"),
      });
      return text(`published via docker exec\n${JSON.stringify(meta, null, 2)}`);
    } catch (err) {
      return errorText(`damage_publish failed: ${err.message || err}`);
    }
  },
);

server.tool(
  "display_screenshot",
  "PNG of the live QEMU VGA display (virtio-vga), plus tty0 text decoded from that framebuffer. This is the guest console. The :8006 e-ink page covers it, so panel_screenshot does not show it. Call this whenever you need to see the display.",
  {},
  async () => {
    try {
      const vga = await captureVga();
      const summary = `vga ${vga.width}x${vga.height}\n${vga.text || "(blank)"}`;
      return imageResult(summary, vga.png);
    } catch (err) {
      return errorText(`display_screenshot failed: ${err.message || err}`);
    }
  },
);

server.tool(
  "console_read",
  "Text of the guest consoles. vga is tty0 (the framebuffer console). ttyS0 is the serial port. Default returns both. Use display_screenshot when you need the pixels.",
  {
    which: z
      .enum(["all", "vga", "ttyS0"])
      .optional()
      .describe("Which console. Default all."),
    tail: z
      .number()
      .int()
      .min(10)
      .max(500)
      .optional()
      .describe("Serial log lines to keep. Default 200."),
  },
  async ({ which, tail }) => {
    try {
      const want = which || "all";
      const parts = [];
      if (want === "all" || want === "vga") {
        const vga = await captureVga();
        parts.push(`tty0 ${vga.width}x${vga.height}\n${vga.text || "(blank)"}`);
      }
      if (want === "all" || want === "ttyS0") {
        const serial = await readSerial(tail ?? 200);
        parts.push(`ttyS0\n${serial || "(empty)"}`);
      }
      return text(parts.join("\n---\n"));
    } catch (err) {
      return errorText(`console_read failed: ${err.message || err}`);
    }
  },
);

server.tool(
  "guest_type",
  "Click the panel and type into the desktop seat. Returns the e-ink screenshot. Those keys do not go to the VGA console while Weston holds the seat.",
  {
    text: z.string().min(1).describe("Keys to type into the desktop seat"),
    submit: z.boolean().optional().describe("Press Enter after typing"),
  },
  async ({ text: typed, submit }) => {
    try {
      const { png, typed: t, submit: s } = await guestType(typed, { submit });
      return imageResult(`typed=${JSON.stringify(t)} submit=${s}`, png);
    } catch (err) {
      return errorText(`guest_type failed: ${err.message || err}`);
    }
  },
);

server.tool(
  "viewer_full_refresh",
  "Click Full refresh on the :8006 viewer and return a screenshot.",
  {},
  async () => {
    try {
      const { png, status } = await viewerFullRefresh();
      return imageResult(`full refresh\nstatus=${status}`, png);
    } catch (err) {
      return errorText(`viewer_full_refresh failed: ${err.message || err}`);
    }
  },
);

server.tool(
  "overlay_touch",
  "Touch files under /shared in the container so oseink-devd reloads the shell (default: bin/oseink-shell).",
  {
    paths: z
      .array(z.string())
      .optional()
      .describe("Paths relative to /shared, e.g. [\"bin/oseink-shell\", \"share/banner\"]"),
  },
  async ({ paths }) => {
    try {
      const result = await overlayTouch(paths || ["bin/oseink-shell"]);
      return text(JSON.stringify(result, null, 2));
    } catch (err) {
      return errorText(`overlay_touch failed: ${err.message || err}`);
    }
  },
);

server.tool(
  "emulator_up",
  "docker compose up -d [--build] for the emulator project.",
  {
    build: z.boolean().optional().describe("Pass --build (default true)"),
  },
  async ({ build }) => {
    try {
      const result = await composeUp({ build: build !== false });
      return text(`up project=${result.project}\n${result.text || "(ok)"}`);
    } catch (err) {
      return errorText(`emulator_up failed: ${err.message || err}`);
    }
  },
);

server.tool(
  "emulator_down",
  "docker compose down for the emulator project.",
  {
    volumes: z.boolean().optional().describe("Also remove volumes (-v)"),
  },
  async ({ volumes }) => {
    try {
      const result = await composeDown({ volumes: Boolean(volumes) });
      return text(`down project=${result.project}\n${result.text || "(ok)"}`);
    } catch (err) {
      return errorText(`emulator_down failed: ${err.message || err}`);
    }
  },
);

server.tool(
  "emulator_logs",
  "Tail docker compose logs for the emulator.",
  {
    tail: z.number().int().min(10).max(500).optional(),
  },
  async ({ tail }) => {
    try {
      const result = await composeLogs(tail ?? 80);
      if (!result.ok) return errorText(result.text);
      return text(result.text || "(empty logs)");
    } catch (err) {
      return errorText(`emulator_logs failed: ${err.message || err}`);
    }
  },
);

server.tool(
  "emulator_ps",
  "docker compose ps for the emulator.",
  {},
  async () => {
    const result = await composePs();
    if (!result.ok) return errorText(result.text);
    return text(result.text);
  },
);

async function main() {
  const transport = new StdioServerTransport();
  await server.connect(transport);
  console.error("oseink-emulator MCP listening on stdio (docker-backed)");
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
