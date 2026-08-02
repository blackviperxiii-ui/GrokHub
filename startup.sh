#!/bin/sh
set -eu
cd /workspace
# Healthy already?
if curl -sf -o /dev/null --max-time 2 http://127.0.0.1:8080/; then
  # Prefer real content; if only broken, continue
  exit 0
fi
export PORT=8080 NITRO_PORT=8080 HOST=0.0.0.0 NITRO_HOST=0.0.0.0
if [ -f .output/server/index.mjs ]; then
  nohup node .output/server/index.mjs >>/tmp/app-startup.log 2>&1 &
else
  nohup npm run dev >>/tmp/app-startup.log 2>&1 &
fi
exit 0
