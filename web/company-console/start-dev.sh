#!/bin/sh
# Wrapper for `next dev` that ensures /usr/local/bin is in PATH.
# Required for the Claude preview tool, which runs with a minimal environment
# where `node` is not discoverable. Turbopack spawns pooled worker processes
# by looking up "node" in PATH — without this wrapper those spawns fail with
# "No such file or directory".
export PATH=/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin
exec /usr/local/bin/node \
  /Users/cno/hyper-stigmergic-morphogenesisII/web/company-console/node_modules/next/dist/bin/next \
  dev \
  /Users/cno/hyper-stigmergic-morphogenesisII/web/company-console \
  -p 3050
