#!/usr/bin/env bash
set -euo pipefail

# Redeploy the DEV CodeLens daemon only — never the shared consumption daemon.
#
# Root-cause fix for "CodeLens keeps going unavailable in other projects":
# the machine has ONE consumption daemon on :7838 that every project
# (via the global ~/.claude.json codelens entry) shares. Rebuilding it while
# dogfooding this repo dropped every other project's CodeLens session. This
# wrapper targets a SEPARATE dev daemon (label dev.codelens.mcp-dev-mutation,
# port 7736, binary codelens-mcp-http-dev) so dev rebuilds are isolated —
# blast radius shrinks from "all projects" to "this repo only".
#
# codelens-mcp-plugin/.mcp.json points at :7736, so this repo's own Claude
# sessions dogfood the working-tree build; all other projects stay on the
# untouched :7838 consumption daemon.
#
# First-time setup (creates the launchd plists + builds the dev binary):
#   bash scripts/install-http-daemons-launchd.sh . \
#     --label-prefix dev.codelens.mcp-dev \
#     --bin-path "$PWD/.codelens/bin/codelens-mcp-http-dev" \
#     --mutation-port 7736 \
#     --semantic --run-at-load --load
#
# After that, iterate with this script (rebuild + resign + restart + probe).
# All extra args pass through to redeploy-daemons.sh (e.g. --skip-mutation).

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

exec bash "${REPO_ROOT}/scripts/redeploy-daemons.sh" \
  --label-prefix dev.codelens.mcp-dev \
  --mutation-port 7736 \
  --target "${REPO_ROOT}/.codelens/bin/codelens-mcp-http-dev" \
  --build --probe "$@"
