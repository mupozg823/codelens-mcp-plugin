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
## Drift signals (why the daemon asks to be restarted)

`prepare_harness_session` / `get_capabilities` can attach a `restart_recommended`
warning. Two **independent** signals feed it (`crates/codelens-mcp/src/build_info.rs`):

| `reason_code` | Trigger | Meaning |
| ------------- | ------- | ------- |
| `stale_daemon_binary` | on-disk executable mtime is newer than the daemon's start time | the daemon outlived its own binary — a rebuild landed but the process was never restarted |
| `head_git_sha_mismatch` | daemon's compile-time `BUILD_GIT_SHA` differs from the project's HEAD | a commit merged after the binary was built is silently absent |

Precedence: `stale_daemon_binary` wins the `reason_code` slot when both fire, so
existing consumers keep their semantics.

Three refinements keep this signal from crying wolf:

- **Common-prefix SHA comparison** (issue #221). `git rev-parse --short` widens
  its output on prefix collisions, so the two sides can describe the same commit
  at different widths. SHAs match when one is a prefix of the other, subject to a
  4-character minimum.
- **`"unknown"` sentinel** — emitted by `build.rs` for non-git builds — is treated
  as *no signal*, not as a mismatch.
- **Binary-relevant classification.** Commits that touch only hooks, docs, scripts,
  or `.github/` produce a byte-identical rebuild; warning on them prompts a restart
  that drops live MCP sessions for no gain. The daemon runs
  `git diff --name-only <binary_sha>..<HEAD> -- crates Cargo.toml Cargo.lock` — the
  same path set `scripts/daemon-stale-check.sh` uses for its "binary-equivalent"
  verdict — and suppresses `head_git_sha_mismatch` when that diff is empty.
  **Fail-open:** if git is unavailable or either SHA is unreachable the
  classification is unknown and the warning is kept. The mtime signal is never
  suppressed by this path.

The HEAD comparison only runs when the daemon executable and the project resolve
to the **same git root** (`should_compare_project_head`). Because the repo-local
daemons run from `.codelens/bin/` inside this repo, the comparison is active here;
a daemon installed outside the project tree simply reports no HEAD signal. Set
`CODELENS_HEAD_GIT_SHA_OVERRIDE` to force the comparison on.

## launchd exit 78: the spawn-failure wedge

Symptom — every service is dead at once and refuses to come back:

```
launchctl print "gui/$(id -u)/dev.codelens.mcp-readonly" | grep -E 'state|last exit'
# state = not running
# last exit code = 78
```

`launchctl kickstart -k` reports success and schedules a spawn, but no process
ever appears (`pgrep -fl codelens-mcp` stays empty) while running the same binary
by hand works fine. Once launchd has wedged this way, kickstart cannot clear it —
only a full re-registration does:

```bash
for label in dev.codelens.mcp-readonly dev.codelens.mcp-mutation \
             dev.codelens.mcp-dev-readonly dev.codelens.mcp-dev-mutation; do
  launchctl bootout "gui/$(id -u)/$label" 2>/dev/null
  launchctl bootstrap "gui/$(id -u)" "$HOME/Library/LaunchAgents/$label.plist"
done
sleep 4 && pgrep -fl codelens-mcp
```

Note that `com.apple.provenance` reappears on the binary after modern macOS
touches it; its presence alone is **not** the blocker here. Reach for the
xattr/codesign path only when the err log actually names
`OS_REASON_CODESIGNING` (see above) — otherwise re-registration is the cure.
