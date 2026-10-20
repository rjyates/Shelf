#!/bin/sh
# Launch entry for every MCP host (Cursor, Claude Code, VS Code, etc.).
# Resolves its own directory so configs do not need a pre-known absolute path.
set -e
cd "$(dirname "$0")"
if [ ! -d node_modules/@modelcontextprotocol ]; then
	npm install >&2
fi
if [ ! -d "$HOME/.cache/ms-playwright" ] && [ ! -d "${PLAYWRIGHT_BROWSERS_PATH:-}" ]; then
	# Best-effort; viewer_screenshot needs Chromium once.
	npx playwright install chromium >&2 || true
fi
exec node server.mjs
