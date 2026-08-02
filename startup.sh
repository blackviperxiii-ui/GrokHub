#!/bin/sh
set -eu
cd /workspace
if curl -sf -o /dev/null --max-time 2 http://127.0.0.1:8080/; then
  exit 0
fi
export PORT=8080 NITRO_PORT=8080 HOST=0.0.0.0 NITRO_HOST=0.0.0.0
# Prefer Vite dev when node_modules present for live HMR; fall back to built server
if [ -d node_modules ] && [ -f package.json ]; then
  nohup npm run dev >>/tmp/app-startup.log 2>&1 &
elif [ -f .output/server/index.mjs ]; then
  nohup node .output/server/index.mjs >>/tmp/app-startup.log 2>&1 &
else
  nohup npm run dev >>/tmp/app-startup.log 2>&1 &
fi
exit 0
