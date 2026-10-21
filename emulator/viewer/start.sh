#!/usr/bin/env bash
# Sourced by qemux before nginx copies default.conf. return, do not exit.
set -Eeuo pipefail

# The picker writes /shared/run/profile.json. The location has to be in
# default.conf before server.sh copies it into the live site.
nohup python3 -u /usr/share/novnc/profile-writer.py >/var/log/oseink-profile.log 2>&1 &

if ! grep -q 'profile-location.conf' /etc/nginx/default.conf; then
  sed -i '$i\    include /usr/share/novnc/profile-location.conf;' /etc/nginx/default.conf
fi

return 0
