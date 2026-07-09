# HTTP Daemon Operations

Reference for the repo-local CodeLens launchd daemons — deploy, redeploy, and
codesigning recovery. Extracted from `CLAUDE.md` to keep the per-turn context lean.

## Dev daemon vs consumption daemon (blast-radius isolation)

The machine runs a **single** consumption daemon pair on `:7839`/`:7838`. Every
project attaches to it via the global `~/.claude.json` `codelens` entry, so
rebuilding it while dogfooding **this** repo drops *every other project's*
CodeLens session (the recurring "CodeLens is genuinely unavailable → grep
fallback" symptom). To break that coupling this repo uses a **dedicated dev
daemon**:

| Role | Label | Ports | Binary | Who uses it |
| ---- | ----- | ----- | ------ | ----------- |
| Consumption | `dev.codelens.mcp` | 7839 / 7838 | `codelens-mcp-http` | global config → all other projects — **never rebuilt during dev** |
| Dev | `dev.codelens.mcp-dev` | 7739 / 7736 | `codelens-mcp-http-dev` | this repo's `.mcp.json` — **rebuild freely** |

`codelens-mcp-plugin/.mcp.json` points at `:7739`, so dogfooding the
working-tree build never touches the shared consumption daemon. Iterate with:

```bash
bash scripts/redeploy-dev-daemon.sh          # rebuild + resign + restart :7739/:7736 only
```

First-time setup (creates the dev plists) is documented at the top of
`scripts/redeploy-dev-daemon.sh`. Only redeploy the **consumption** daemon
(`redeploy-daemons.sh`) on a deliberate release, not during iteration.

## HTTP Daemon Operations (this repo)

Two repo-local launchd agents share the on-disk index and use advisory `register_agent_work` / `claim_files` for mutation collisions:

- `dev.codelens.mcp-readonly` → `:7839`, profile `reviewer-graph`, mode `read-only` — for planner/reviewer (Claude) sessions
- `dev.codelens.mcp-mutation` → `:7838`, profile `refactor-full`, mode `mutation-enabled` — for builder (Codex) sessions

The **consumption** clients (global `~/.claude.json`, `~/.codex/config.toml`) attach by URL to `:7839` by default; this repo's `.mcp.json` uses the dev daemon on `:7739` (see above). Restart cycle (preferred path):

```bash
bash scripts/redeploy-daemons.sh --probe          # quick: cp + xattr/codesign + kickstart + LISTEN + tools/list
bash scripts/redeploy-daemons.sh --build --probe  # also runs cargo build --release --features http,semantic
bash scripts/daemon-stale-check.sh                # read-only: compare daemon binary git sha to source HEAD (exit 1 if stale)
```

What the script does: `cp target/release/codelens-mcp → .codelens/bin/codelens-mcp-http`, `xattr -dr com.apple.provenance ${target}` (otherwise macOS gatekeeper SIGKILLs the daemon with `OS_REASON_CODESIGNING`), `codesign --force --sign -` (ad-hoc resign so launchd accepts the new mach-o), `launchctl bootout/bootstrap` plus `kickstart -k gui/$UID/dev.codelens.mcp-{readonly,mutation}` to refresh launchd's cached code requirement, wait for LISTEN on 7838/7839, and (with `--probe`) issue `tools/list` against both.

Manual fallback (if the script is unavailable):

```bash
cp -f target/release/codelens-mcp .codelens/bin/codelens-mcp-http
xattr -dr com.apple.provenance .codelens/bin/codelens-mcp-http
codesign --force --sign - .codelens/bin/codelens-mcp-http
launchctl kickstart -k "gui/$(id -u)/dev.codelens.mcp-readonly"
launchctl kickstart -k "gui/$(id -u)/dev.codelens.mcp-mutation"
sleep 4 && pgrep -fl codelens-mcp
```

If `pgrep` shows nothing after restart, the binary is missing `--features http` (see the Feature Flag Matrix in `../../CLAUDE.md`) — check `.codelens/reports/launchd/dev.codelens.mcp-readonly.err.log`. If the err log shows `last exit reason = OS_REASON_CODESIGNING`, the xattr/codesign step was skipped.
