#!/bin/sh
# A host bind mount replaces the /data ownership baked into the image.
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
    exec setpriv --reuid="$owner" --regid="$group" --clear-groups --inh-caps=-all -- /usr/local/bin/shelf-server "$@"
fi
exec /usr/local/bin/shelf-server "$@"
