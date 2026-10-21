/**
 * QEMU VGA framebuffer and serial console.
 * The :8006 page paints the e-ink damage frame on top of VNC, so the VGA
 * console is not in panel_screenshot. screendump reads virtio-vga directly.
 * ttyS0 is QEMU -serial mon:stdio, which lands in the container log.
 * shortcut: qemux status lines in that log are dropped by prefix; switch to a
 * dedicated serial file if a guest line starts getting dropped.
 */
import { execFile } from "node:child_process";
import { readFileSync } from "node:fs";
import { promisify } from "node:util";
import { composeLogs, requireContainer } from "./docker.mjs";
import { rgbToPng } from "./png.mjs";
import { EMULATOR_DIR, composeProject } from "./paths.mjs";

const execFileAsync = promisify(execFile);
const FONT = readFileSync(new URL("./font8x16.bin", import.meta.url));

const GLYPH = new Map();
for (let i = 32; i < 127; i++) {
  const key = FONT.subarray(i * 16, i * 16 + 16).toString("latin1");
  if (!GLYPH.has(key)) GLYPH.set(key, String.fromCharCode(i));
}

const MONITOR_PY = `
import socket, sys
path = sys.argv[1]
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.settimeout(15)
s.connect("/dev/shm/monitor.sock")

def until_prompt():
    buf = b""
    while b"(qemu)" not in buf:
        chunk = s.recv(65536)
        if not chunk:
            raise SystemExit("QEMU monitor closed")
        buf += chunk
    return buf

until_prompt()
s.sendall(b"screendump " + path.encode() + b"\\n")
reply = until_prompt()
low = reply.lower()
if b"could not" in low or b"screendump:" in low:
    raise SystemExit(reply.decode("utf-8", "replace"))
`;

function cellKey(rgb, width, x0, y0) {
  const bytes = Buffer.alloc(16);
  for (let row = 0; row < 16; row++) {
    let bits = 0;
    const y = y0 + row;
    for (let col = 0; col < 8; col++) {
      const o = (y * width + x0 + col) * 3;
      if (rgb[o] | rgb[o + 1] | rgb[o + 2]) bits |= 0x80 >> col;
    }
    bytes[row] = bits;
  }
  return bytes.toString("latin1");
}

/** Visible fbcon text. Blank cells and the underline cursor become spaces. */
export function decodeFbcon(rgb, width, height) {
  const cols = Math.floor(width / 8);
  const rows = Math.floor(height / 16);
  const lines = [];
  const blank = "\0".repeat(16);
  const cursorTop = "\0".repeat(14);
  for (let cy = 0; cy < rows; cy++) {
    let line = "";
    for (let cx = 0; cx < cols; cx++) {
      const key = cellKey(rgb, width, cx * 8, cy * 16);
      if (key === blank) {
        line += " ";
        continue;
      }
      const ch = GLYPH.get(key);
      if (ch) {
        line += ch;
        continue;
      }
      line += key.slice(0, 14) === cursorTop ? " " : "?";
    }
    lines.push(line.replace(/\s+$/, ""));
  }
  return lines.join("\n").replace(/\n+$/, "");
}

export function parsePpm(buf) {
  if (buf[0] !== 0x50 || buf[1] !== 0x36 || buf[2] !== 0x0a) {
    throw new Error("screendump was not a P6 PPM");
  }
  let i = 3;
  const line = () => {
    const n = buf.indexOf(0x0a, i);
    if (n < 0) throw new Error("truncated PPM");
    const s = buf.toString("ascii", i, n);
    i = n + 1;
    return s;
  };
  const dim = line();
  const max = line();
  const [width, height] = dim.trim().split(/\s+/).map(Number);
  if (max.trim() !== "255" || !width || !height) {
    throw new Error(`bad PPM header ${dim} ${max}`);
  }
  const need = width * height * 3;
  const rgb = buf.subarray(i, i + need);
  if (rgb.length !== need) throw new Error(`PPM short ${rgb.length} != ${need}`);
  return { width, height, rgb };
}

/** Drop the qemux wrapper lines that share stdout with ttyS0. */
export function stripSerialLog(raw) {
  const ansi = /\x1b(?:\[[0-9;?]*[ -/]*[@-~]|.)/g;
  const kept = [];
  for (const line of String(raw).split(/\r?\n/)) {
    const body = line.replace(/^\S+\s+\|\s?/, "").replace(ansi, "").replace(/\r/g, "");
    const trimmed = body.trim();
    if (!trimmed) continue;
    if (trimmed.startsWith("❯")) continue;
    if (trimmed.startsWith("RTNETLINK ")) continue;
    kept.push(trimmed);
  }
  return kept.join("\n");
}

async function composeExec(args, { encoding = "utf8", maxBuffer = 1024 * 1024 } = {}) {
  try {
    const { stdout, stderr } = await execFileAsync(
      "docker",
      ["compose", "-p", composeProject(), "exec", "-T", "qemu", ...args],
      { cwd: EMULATOR_DIR, encoding, maxBuffer },
    );
    return { stdout, stderr: stderr || "" };
  } catch (err) {
    const detail = String(err.stderr || err.stdout || err.message || err).trim();
    throw new Error(detail);
  }
}

/** Live virtio-vga framebuffer as PNG plus decoded tty0 text. */
export async function captureVga() {
  await requireContainer();
  const ppmPath = `/tmp/oseink-vga-${process.pid}-${Date.now()}.ppm`;
  try {
    await composeExec(["python3", "-c", MONITOR_PY, ppmPath]);
    const ppm = await composeExec(["cat", ppmPath], {
      encoding: "buffer",
      maxBuffer: 32 * 1024 * 1024,
    });
    const { width, height, rgb } = parsePpm(ppm.stdout);
    return {
      width,
      height,
      png: rgbToPng(width, height, rgb),
      text: decodeFbcon(rgb, width, height),
    };
  } finally {
    await composeExec(["rm", "-f", ppmPath]).catch(() => {});
  }
}

export async function readSerial(tail = 200) {
  const result = await composeLogs(tail);
  if (!result.ok) throw new Error(result.text || "serial log failed");
  return stripSerialLog(result.text);
}
