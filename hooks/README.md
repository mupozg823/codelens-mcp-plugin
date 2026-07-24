# CodeLens plugin hooks

## Default install registers **zero** hooks (E6.1)

There is no `hooks/hooks.json` in this repo, and that is intentional. A plugin
`hooks/hooks.json` is loaded and **activated automatically** for every user of
the plugin, so anything listed there is a cost the whole install pays whether or
not it wants the behaviour. The CodeLens plugin therefore ships an empty default
hook surface: installing it adds no `PreToolUse`, `PostToolUse`, or
`SessionStart` hook. `scripts/validate-plugin-manifest.py --check` enforces this
in CI.

Every script in this directory is **opt-in**. Ready-to-merge registration
fragments live in `hooks/optional/`:

| Fragment                                   | Hook                | Event         | Matcher        | Cost                                 |
| ------------------------------------------ | ------------------- | ------------- | -------------- | ------------------------------------ |
| `hooks/optional/codelens-first.hooks.json` | `codelens-first.py` | `PreToolUse`  | `Grep`, `Bash` | one short `python3` run, `timeout 5` |

### Enabling an optional hook

Pick one of the two wirings (never both — see *Double-wiring* below):

1. **Plugin-local** — copy the fragment to `hooks/hooks.json` inside your
   installed plugin directory. `${CLAUDE_PLUGIN_ROOT}` resolves automatically,
   so the file works verbatim:

   ```bash
   cp "$CLAUDE_PLUGIN_ROOT/hooks/optional/codelens-first.hooks.json" \
      "$CLAUDE_PLUGIN_ROOT/hooks/hooks.json"
   ```

   A plugin upgrade overwrites the plugin directory, so re-apply after upgrades.

2. **Settings-level (survives upgrades)** — merge the same `hooks` object into
   your user `~/.claude/settings.json` or a project `.claude/settings.json`, and
   replace `${CLAUDE_PLUGIN_ROOT}` with the absolute path of the installed
   plugin (`${CLAUDE_PLUGIN_ROOT}` is only expanded for plugin-provided hook
   files).

Disable again by deleting the copied `hooks.json` / the merged settings entry,
or set `CODELENS_FIRST_MODE=off` for a single session.

> **Double-wiring:** registering `codelens-first.py` in both places fires the
> hook **twice** per `Grep`/`Bash` call and burns the per-session advisory/deny
> throttle at double speed. Keep exactly one registration.

### `codelens-first.py` — nudge symbol lookups toward CodeLens

Native `Grep` (and shell-invoked `grep`/`rg` via `Bash`) floods the model
context with raw matches (imports, strings, comments) and re-pays that token
cost every turn the result stays in context. When the pattern is really a
**symbol lookup**, a bounded CodeLens call (`find_symbol` /
`find_referencing_symbols`) returns a ranked, deduped answer for far fewer
tokens. This hook detects that case and advises (or, optionally, enforces) the
switch. The hook's own output is bounded — a single short line, no timestamps
or absolute paths — so it never becomes token overhead itself.

For the `Bash` matcher, detection is **pipeline-aware**: the command is split
into pipelines on `;`/`&&`/`||`/newline and only the FIRST command of each
pipeline is classified (`ps aux | grep node` is output filtering and never
fires), tokenized with `shlex`. In `strict` mode a matching segment denies the
*whole* `Bash` call — coarser than the `Grep`-tool path, an accepted tradeoff
given the escape markers and the per-session deny cap below.

**Modes** — set via the `CODELENS_FIRST_MODE` environment variable:

| Mode                 | Behaviour on a symbol-like Grep pattern                                  |
| -------------------- | ----------------------------------------------------------------------- |
| `advisory` (default) | `allow` + `additionalContext` suggesting the CodeLens call. Non-blocking.|
| `strict`             | `deny` **high-confidence** symbols (snake_case/camelCase/`::`) with the concrete CodeLens procedure, ≤10 denies/session (repeats are terse one-liners). Ambiguous lowercase words downgrade to a single advisory. |
| `off`                | Do nothing.                                                             |

**Strict safeguards** — `strict` only denies when *all* of these pass
(fail-open otherwise):

- Daemon health probe against `CODELENS_CARD_URL` (default
  `http://127.0.0.1:7838/.well-known/mcp.json`); down → pass, with a 5-min
  negative cache. `CODELENS_FIRST_ASSUME_ALIVE=1` skips the probe (CI/tests).
- Not in a worktree cwd (`/worktrees/` — post-edit index staleness).
- No escape marker in the command: `# [cl-text]` (plain text audit) or
  `# [cl-fallback]` (CodeLens failed / returned nothing) always pass.
- No text-audit flags (`-i` / `-F` / `-v`).
- Targets are not docs/logs/artifacts (`.md`/`.json`/`logs/`/`dist/` …) and not
  absolute paths outside the project root.

**Gate** — the hook only acts when a **project-local** `.codelens/` directory
exists at or above the session `cwd` (searched up to 5 levels). The global
`~/.codelens` data directory shares the marker's basename but is **not** a
project index, so it is excluded (as is a `.codelens` sitting at a temp root);
otherwise the upward walk would match `~/.codelens` for essentially any project
under the home tree and gate `Grep` across it. Projects that do not use CodeLens
are passed through unconditionally, in every mode (`strict` included).

**What is *not* nudged (always passes):**

- Patterns with regex metacharacters (`.*+?[](){}|` …) — real text search.
- Multi-alternative patterns (`a\|b\|c`) — an OR-of-terms audit, not a lookup.
- A `path` (or the sole positional path in a `Bash` grep/rg segment) pointing
  at a single file — a targeted text audit.
- Patterns shorter than 3 characters after stripping `\b` word anchors.
- Any `Bash` command whose grep/rg segment doesn't parse cleanly (fail-open).

**Throttle** — in `advisory` mode, at most **2** suggestions are emitted per
session (tracked in `$TMPDIR/codelens-first-<session_id>`), so the advice does
not become nagging. In `strict` mode: **≤10 denies**/session
(`$TMPDIR/codelens-first-deny-<session_id>`; deny #2+ uses a terse one-line
reason to keep injected-token cost flat — raised from 3 on 2026-07-12 after
14-day metrics showed the old cap was the gate's main leak: 143 `deny_capped`
passes vs 46 denies) and **≤1 advisory** for ambiguous-symbol downgrades.

**Metrics** — when `~/.claude/metrics/` already exists (the hook never creates
it), every decision appends a JSONL record to `codelens-first.jsonl` there
(`{"s": <session-prefix>, "d": deny|advise|pass, "why": ..., "sym": ...}`), so
redirect→conversion rates and realized token savings stay measurable offline.

**Fail-open** — any malformed stdin or internal error exits 0 with no output.
The hook will never break your `Grep`.

**Disable:**

- `CODELENS_FIRST_MODE=off` — silence the hook for a session.
- Remove the registration you added in *Enabling an optional hook* — the hook is
  not registered by default, so an untouched install is already silent.

## Manual-only helper scripts (no registration fragment)

`hooks/codelens-session-probe.sh` is a host-side `SessionStart` hook (register
it in your own `settings.json`, not here — matcher `startup|clear|compact`
recommended): it injects one bounded line (≤350 bytes) telling the model
whether the CodeLens daemon is alive and whether the project is auto-bound via
an `.mcp.json` `x-codelens-project` header. Verb-routing detail is delegated to
the host's always-on rules, not repeated here. It stays **silent** for projects
that don't use CodeLens (no `.codelens/` index and no header) — zero token cost
outside CodeLens projects — and also for `source: "resume"` events, which
continue an existing context that already carries the original injection.
Documented exception: a session started directly in `$HOME` matches the global
`~/.codelens` data directory and does fire (unlike `codelens-first.py`, which
excludes it as a project-index marker), because home sessions are harness work
that does use CodeLens. The home-session message instructs binding to the
*target repo* being queried, never to `$HOME` itself — a
`prepare_harness_session(project=$HOME)` call attempts to index the whole home
tree and times out (measured 2026-07-12).

`hooks/post-edit-diagnostics.sh` and `hooks/clang-linker.sh` are **opt-in
examples** with no fragment in `hooks/optional/`. If they were auto-activated,
`post-edit-diagnostics.sh` would spawn a CodeLens diagnostics pass on every
`Edit`, charging every plugin user that cost on each edit.

Both scripts also predate the current stdin-JSON hook contract — they read the
edited file path from a positional argument (`$1` / `$TOOL_INPUT_FILE_PATH`),
**not** from `PreToolUse`/`PostToolUse` stdin JSON. To wire either one into
`settings.json` you must first adapt it to parse the stdin JSON contract (e.g.
`jq -r '.tool_input.file_path'`). Register them per-project in your own
`settings.json`, never here.
