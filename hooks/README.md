# CodeLens plugin hooks

## `hooks.json` — auto-activated on plugin install

> **Warning:** `hooks/hooks.json` is loaded and **activated automatically** when the
> CodeLens plugin is installed. Every hook listed here runs for all users of the
> plugin. Keep it minimal and cheap.
>
> **Double-wiring:** if you registered `codelens-first.py` directly in your user
> `settings.json` (the pre-plugin setup), remove that entry when installing the
> plugin — otherwise the hook fires **twice** per `Grep`/`Bash` call and burns
> the per-session advisory/deny throttle at double speed.

Currently it registers one `PreToolUse` hook on two matchers: the native
`Grep` tool, and `Bash` (for shell-invoked `grep`/`rg`):

| Hook                | Event         | Matcher      | Cost                                |
| ------------------- | ------------- | ------------ | ----------------------------------- |
| `codelens-first.py` | `PreToolUse`  | `Grep`, `Bash` | one short `python3` run, `timeout 5`|

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
| `strict`             | `deny` **high-confidence** symbols (snake_case/camelCase/`::`) with the concrete CodeLens procedure, ≤3 denies/session (repeats are terse one-liners). Ambiguous lowercase words downgrade to a single advisory. |
| `off`                | Do nothing.                                                             |

**Strict safeguards** — `strict` only denies when *all* of these pass
(fail-open otherwise):

- Daemon health probe against `CODELENS_CARD_URL` (default
  `http://127.0.0.1:7839/.well-known/mcp.json`); down → pass, with a 5-min
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
not become nagging. In `strict` mode: **≤3 denies**/session
(`$TMPDIR/codelens-first-deny-<session_id>`; deny #2+ uses a terse one-line
reason to keep injected-token cost flat) and **≤1 advisory** for
ambiguous-symbol downgrades.

**Metrics** — when `~/.claude/metrics/` already exists (the hook never creates
it), every decision appends a JSONL record to `codelens-first.jsonl` there
(`{"s": <session-prefix>, "d": deny|advise|pass, "why": ..., "sym": ...}`), so
redirect→conversion rates and realized token savings stay measurable offline.

**Fail-open** — any malformed stdin or internal error exits 0 with no output.
The hook will never break your `Grep`.

**Disable:**

- `CODELENS_FIRST_MODE=off` — silence the hook for a session.
- Remove the `Grep` and/or `Bash` entry from `hooks.json` — disable it for the whole plugin.

## Manual-only helper scripts (not registered in `hooks.json`)

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
that does use CodeLens.

`hooks/post-edit-diagnostics.sh` and `hooks/clang-linker.sh` are **opt-in
examples**, deliberately left out of `hooks.json`. If they were auto-activated,
`post-edit-diagnostics.sh` would spawn a CodeLens diagnostics pass on every
`Edit`, charging every plugin user that cost on each edit.

Both scripts also predate the current stdin-JSON hook contract — they read the
edited file path from a positional argument (`$1` / `$TOOL_INPUT_FILE_PATH`),
**not** from `PreToolUse`/`PostToolUse` stdin JSON. To wire either one into
`settings.json` you must first adapt it to parse the stdin JSON contract (e.g.
`jq -r '.tool_input.file_path'`). Register them per-project in your own
`settings.json`, never here.
