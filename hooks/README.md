# CodeLens plugin hooks

## `hooks.json` — auto-activated on plugin install

> **Warning:** `hooks/hooks.json` is loaded and **activated automatically** when the
> CodeLens plugin is installed. Every hook listed here runs for all users of the
> plugin. Keep it minimal and cheap.

Currently it registers exactly one `PreToolUse` hook on the native `Grep` tool:

| Hook                | Event         | Matcher | Cost                                |
| ------------------- | ------------- | ------- | ----------------------------------- |
| `codelens-first.py` | `PreToolUse`  | `Grep`  | one short `python3` run, `timeout 5`|

### `codelens-first.py` — nudge symbol lookups toward CodeLens

Native `Grep` floods the model context with raw matches (imports, strings,
comments) and re-pays that token cost every turn the result stays in context.
When a Grep pattern is really a **symbol lookup**, a bounded CodeLens call
(`find_symbol` / `find_referencing_symbols`) returns a ranked, deduped answer
for far fewer tokens. This hook detects that case and advises (or, optionally,
enforces) the switch. The hook's own output is bounded — a single short line,
no timestamps or absolute paths — so it never becomes token overhead itself.

**Modes** — set via the `CODELENS_FIRST_MODE` environment variable:

| Mode                 | Behaviour on a symbol-like Grep pattern                                  |
| -------------------- | ----------------------------------------------------------------------- |
| `advisory` (default) | `allow` + `additionalContext` suggesting the CodeLens call. Non-blocking.|
| `strict`             | `deny` with a concrete `find_symbol(...)` replacement call. Blocking.    |
| `off`                | Do nothing.                                                             |

**Gate** — the hook only acts when a **project-local** `.codelens/` directory
exists at or above the session `cwd` (searched up to 5 levels). The global
`~/.codelens` data directory shares the marker's basename but is **not** a
project index, so it is excluded (as is a `.codelens` sitting at a temp root);
otherwise the upward walk would match `~/.codelens` for essentially any project
under the home tree and gate `Grep` across it. Projects that do not use CodeLens
are passed through unconditionally, in every mode (`strict` included).

**What is *not* nudged (always passes):**

- Patterns with regex metacharacters (`.*+?[](){}|` …) — real text search.
- A `path` pointing at a single file — a targeted text audit.
- Patterns shorter than 3 characters after stripping `\b` word anchors.

**Throttle** — in `advisory` mode, at most **2** suggestions are emitted per
session (tracked in `$TMPDIR/codelens-first-<session_id>`), so the advice does
not become nagging. `strict` is not throttled.

**Fail-open** — any malformed stdin or internal error exits 0 with no output.
The hook will never break your `Grep`.

**Disable:**

- `CODELENS_FIRST_MODE=off` — silence the hook for a session.
- Remove the `Grep` entry from `hooks.json` — disable it for the whole plugin.

## Manual-only helper scripts (not registered in `hooks.json`)

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
