#!/bin/sh
# Bind-mount /data ownership fix, then ghost-story (CTC) + shelf-server in one container.
set -eu

if [ "$(id -u)" -eq 0 ]; then
    data="${SHELF_DATA:-/data}"
    mkdir -p "$data"
    owner=$(stat -c %u "$data")
    group=$(stat -c %g "$data")
    # Docker creates a missing host folder as root, which uid 10001 cannot write.
    if [ "$owner" -eq 0 ]; then
        chown shelf:shelf "$data"
        owner=10001
        group=10001
    fi
    # shortcut: files already inside a root-owned directory stay root-owned, chown -R if a pre-seeded database must be writable
    exec setpriv --reuid="$owner" --regid="$group" --clear-groups --inh-caps=-all -- "$0" "$@"
fi

data="${SHELF_DATA:-/data}"
mkdir -p "$data/files" "$data/ghost-story/models"
# setpriv keeps HOME=/root; ghost-story uses os.homedir() for defaults — must be writable.
export HOME="$data"
mkdir -p "$HOME/.local/share"

: "${GHOST_STORY_API_KEY:=shelf-internal}"
: "${GHOST_STORY_HOST:=127.0.0.1}"
: "${GHOST_STORY_PORT:=8765}"
: "${GHOST_STORY_MODELS_DIR:=$data/ghost-story/models}"
export GHOST_STORY_API_KEY GHOST_STORY_HOST GHOST_STORY_PORT GHOST_STORY_MODELS_DIR
export SHELF_CTC_EMIT_URL="${SHELF_CTC_EMIT_URL:-http://127.0.0.1:${GHOST_STORY_PORT}}"
export SHELF_CTC_EMIT_API_KEY="${SHELF_CTC_EMIT_API_KEY:-$GHOST_STORY_API_KEY}"
# install ctc-model has no --model-dir; point getModelDir() at our cache.
ln -sfn "$(dirname "$GHOST_STORY_MODELS_DIR")" "$HOME/.local/share/ghost-story"

# Emit device: explicit GHOST_STORY_DEVICE wins; else tag default; fall back to cpu if nodes missing.
device="${GHOST_STORY_DEVICE:-}"
if [ -z "$device" ] || [ "$device" = "auto" ]; then
    if [ -r /etc/shelf-gpu-default-device ]; then
        device=$(cat /etc/shelf-gpu-default-device)
    else
        device=cpu
    fi
    if [ "$device" = "cuda" ] && ! ls /dev/nvidia* >/dev/null 2>&1; then
        echo "shelf: no NVIDIA devices; CTC emit falling back to cpu" >&2
        device=cpu
    fi
    if [ "$device" = "webgpu" ] && [ ! -e /dev/dri ]; then
        echo "shelf: no /dev/dri; CTC emit falling back to cpu" >&2
        device=cpu
    fi
fi
export GHOST_STORY_DEVICE="$device"
echo "shelf: ghost-story --device $device"

if [ -z "$(ls -A "$GHOST_STORY_MODELS_DIR" 2>/dev/null || true)" ]; then
    echo "shelf: installing CTC model into $GHOST_STORY_MODELS_DIR"
    ghost-story install ctc-model
    if [ -z "$(ls -A "$GHOST_STORY_MODELS_DIR" 2>/dev/null || true)" ]; then
        echo "shelf: CTC model install left $GHOST_STORY_MODELS_DIR empty" >&2
        exit 1
    fi
fi

# Restart loop: a single OOM/crash must not leave shelf without emissions.
(
    while true; do
        ghost-story serve \
            --host "$GHOST_STORY_HOST" \
            --port "$GHOST_STORY_PORT" \
            --api-key "$GHOST_STORY_API_KEY" \
            --device "$device" \
            --model-dir "$GHOST_STORY_MODELS_DIR" \
            && break
        echo "shelf: ghost-story exited; restarting in 2s" >&2
        sleep 2
    done
) &
gs_pid=$!
cleanup() { kill "$gs_pid" 2>/dev/null || true; }
trap cleanup EXIT INT TERM

for i in $(seq 1 120); do
    if curl -sf -H "Authorization: Bearer ${GHOST_STORY_API_KEY}" \
        "http://${GHOST_STORY_HOST}:${GHOST_STORY_PORT}/v1/capabilities" >/dev/null; then
        break
    fi
    if ! kill -0 "$gs_pid" 2>/dev/null; then
        echo "shelf: ghost-story exited before ready" >&2
        exit 1
    fi
    sleep 1
done

exec /usr/local/bin/shelf-server "$@"
