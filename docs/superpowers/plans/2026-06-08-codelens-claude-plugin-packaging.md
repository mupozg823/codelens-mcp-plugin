# CodeLens Claude Code Plugin Packaging Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Package the existing CodeLens MCP server + skills + agent as an installable Claude Code plugin with an in-repo single-plugin marketplace, gated by a deterministic manifest validator.

**Architecture:** Add `.claude-plugin/plugin.json` (stdio `codelens-mcp` MCP server + reuse top-level `skills/` and `agents/`) and `.claude-plugin/marketplace.json` (Sentry-style, `source: "./"`). A new `scripts/validate-plugin-manifest.py` checks JSON validity, required fields, that bundled paths exist, and marketplace↔plugin name consistency; its tests live in `scripts/test/` (auto-run by the existing "script contract tests" CI step) and its `--check` runs as a new CI step. The binary itself is a documented prerequisite (no Rust changes).

**Tech Stack:** Python 3 (stdlib only: `argparse`, `json`, `pathlib`, `tempfile`, `importlib`), GitHub Actions YAML, JSON manifests, Markdown.

**Spec:** `docs/superpowers/specs/2026-06-08-codelens-claude-plugin-packaging-design.md`
**Branch:** `codelens-sp4-plugin-packaging` (worktree at `.worktrees/sp4-plugin-packaging`, base `main` @ `a0e8a1a`)

---

## File Structure

| Path | Responsibility |
|------|----------------|
| `scripts/validate-plugin-manifest.py` | Pure validator: `collect_manifest_errors(repo_root) -> list[str]` + `--check` CLI. The single piece of real logic. |
| `scripts/test/test-validate-plugin-manifest.py` | Contract tests for the validator (temp-fixture driven). Auto-discovered by CI. |
| `.claude-plugin/plugin.json` | Plugin manifest: stdio MCP server + skills/agents paths. Config (validated, not unit-tested). |
| `.claude-plugin/marketplace.json` | Single-plugin marketplace entry. Config. |
| `.github/workflows/ci.yml` | +1 step invoking the validator `--check`. |
| `README.md` | + "Claude Code Plugin" section (install, prerequisite, failure mode). |

Convention notes (verified in repo):
- Check scripts compute `REPO_ROOT = Path(__file__).resolve().parents[1]`; tests use `parents[2]`.
- Test files expose plain `test_*()` functions + a `main()` runner that prints `PASS`/`FAIL` and `raise SystemExit(main())`. The CI step `for test in scripts/test/test-*.py; do python3 "$test"; done` runs them — **no separate CI step needed for validator tests**.

---

## Task 1: Manifest validator (TDD)

**Files:**
- Create: `scripts/validate-plugin-manifest.py`
- Test: `scripts/test/test-validate-plugin-manifest.py`

- [ ] **Step 1: Write the failing test**

Create `scripts/test/test-validate-plugin-manifest.py`:

```python
#!/usr/bin/env python3
"""Contract tests for scripts/validate-plugin-manifest.py."""
from __future__ import annotations

import importlib.util
import json
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
_SPEC = importlib.util.spec_from_file_location(
    "validate_plugin_manifest", REPO_ROOT / "scripts" / "validate-plugin-manifest.py"
)
_MOD = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(_MOD)
collect_manifest_errors = _MOD.collect_manifest_errors


VALID_PLUGIN = {
    "name": "codelens",
    "version": "1.0.0",
    "description": "d",
    "mcpServers": {"codelens": {"command": "codelens-mcp"}},
    "skills": "./skills/",
    "agents": "./agents/",
}
VALID_MARKET = {
    "name": "codelens",
    "owner": {"name": "x"},
    "plugins": [{"name": "codelens", "source": "./", "description": "d"}],
}


def _write(root: Path, plugin, market) -> None:
    cp = root / ".claude-plugin"
    cp.mkdir(parents=True, exist_ok=True)
    (root / "skills").mkdir(exist_ok=True)
    (root / "skills" / "x").write_text("x", encoding="utf-8")
    (root / "agents").mkdir(exist_ok=True)
    (root / "agents" / "a.md").write_text("a", encoding="utf-8")
    if plugin is not None:
        text = plugin if isinstance(plugin, str) else json.dumps(plugin)
        (cp / "plugin.json").write_text(text, encoding="utf-8")
    if market is not None:
        text = market if isinstance(market, str) else json.dumps(market)
        (cp / "marketplace.json").write_text(text, encoding="utf-8")


def test_valid_manifests_pass() -> None:
    with tempfile.TemporaryDirectory() as d:
        root = Path(d)
        _write(root, VALID_PLUGIN, VALID_MARKET)
        assert collect_manifest_errors(root) == []


def test_missing_plugin_file_reported() -> None:
    with tempfile.TemporaryDirectory() as d:
        root = Path(d)
        _write(root, None, VALID_MARKET)
        errs = collect_manifest_errors(root)
        assert any("plugin.json" in e and "missing" in e for e in errs)


def test_broken_json_reported() -> None:
    with tempfile.TemporaryDirectory() as d:
        root = Path(d)
        _write(root, "{ not json", VALID_MARKET)
        errs = collect_manifest_errors(root)
        assert any("invalid JSON" in e for e in errs)


def test_missing_required_field_reported() -> None:
    with tempfile.TemporaryDirectory() as d:
        root = Path(d)
        p = dict(VALID_PLUGIN)
        del p["mcpServers"]
        _write(root, p, VALID_MARKET)
        errs = collect_manifest_errors(root)
        assert any("mcpServers" in e for e in errs)


def test_bad_mcp_command_reported() -> None:
    with tempfile.TemporaryDirectory() as d:
        root = Path(d)
        p = dict(VALID_PLUGIN)
        p["mcpServers"] = {"codelens": {"command": ""}}
        _write(root, p, VALID_MARKET)
        errs = collect_manifest_errors(root)
        assert any("command" in e for e in errs)


def test_dangling_skills_path_reported() -> None:
    with tempfile.TemporaryDirectory() as d:
        root = Path(d)
        p = dict(VALID_PLUGIN)
        p["skills"] = "./nope/"
        _write(root, p, VALID_MARKET)
        errs = collect_manifest_errors(root)
        assert any("skills" in e and "nope" in e for e in errs)


def test_marketplace_name_mismatch_reported() -> None:
    with tempfile.TemporaryDirectory() as d:
        root = Path(d)
        m = json.loads(json.dumps(VALID_MARKET))
        m["plugins"][0]["name"] = "wrong"
        _write(root, VALID_PLUGIN, m)
        errs = collect_manifest_errors(root)
        assert any("match" in e.lower() for e in errs)


def test_empty_plugins_array_reported() -> None:
    with tempfile.TemporaryDirectory() as d:
        root = Path(d)
        m = json.loads(json.dumps(VALID_MARKET))
        m["plugins"] = []
        _write(root, VALID_PLUGIN, m)
        errs = collect_manifest_errors(root)
        assert any("plugins" in e for e in errs)


def main() -> int:
    failures: list[str] = []
    tests = [
        test_valid_manifests_pass,
        test_missing_plugin_file_reported,
        test_broken_json_reported,
        test_missing_required_field_reported,
        test_bad_mcp_command_reported,
        test_dangling_skills_path_reported,
        test_marketplace_name_mismatch_reported,
        test_empty_plugins_array_reported,
    ]
    for t in tests:
        try:
            t()
            print(f"PASS  {t.__name__}")
        except AssertionError as exc:
            print(f"FAIL  {t.__name__}: {exc}")
            failures.append(t.__name__)
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python3 scripts/test/test-validate-plugin-manifest.py`
Expected: FAIL — `FileNotFoundError` / `spec_from_file_location` cannot load `scripts/validate-plugin-manifest.py` because it does not exist yet (module load raises before any test runs).

- [ ] **Step 3: Write minimal implementation**

Create `scripts/validate-plugin-manifest.py`:

```python
#!/usr/bin/env python3
"""Validate the .claude-plugin manifests (plugin.json + marketplace.json).

Deterministic structure gate, mirroring scripts/surface-manifest.py --check.
Verifies JSON validity, required fields, that bundled skills/agents directories
exist and are non-empty, and that the marketplace entry is consistent with
plugin.json. The codelens-mcp binary is an out-of-band prerequisite and is NOT
checked here.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]

PLUGIN_MANIFEST = ".claude-plugin/plugin.json"
MARKETPLACE_MANIFEST = ".claude-plugin/marketplace.json"
REQUIRED_PLUGIN_FIELDS = ("name", "version", "description", "mcpServers")
REQUIRED_MARKET_FIELDS = ("name", "owner", "plugins")


def _load_json(path: Path, label: str):
    if not path.exists():
        return None, f"{label}: missing"
    try:
        return json.loads(path.read_text(encoding="utf-8")), None
    except json.JSONDecodeError as exc:
        return None, f"{label}: invalid JSON ({exc})"


def collect_manifest_errors(repo_root: Path) -> list[str]:
    errors: list[str] = []

    plugin, err = _load_json(repo_root / PLUGIN_MANIFEST, PLUGIN_MANIFEST)
    if err:
        errors.append(err)
    if isinstance(plugin, dict):
        for field in REQUIRED_PLUGIN_FIELDS:
            if field not in plugin:
                errors.append(f"{PLUGIN_MANIFEST}: missing required field '{field}'")

        servers = plugin.get("mcpServers")
        if servers is not None and not isinstance(servers, dict):
            errors.append(f"{PLUGIN_MANIFEST}: mcpServers must be an object")
        elif isinstance(servers, dict):
            entry = servers.get("codelens")
            if not isinstance(entry, dict) or not isinstance(entry.get("command"), str) or not entry.get("command"):
                errors.append(
                    f"{PLUGIN_MANIFEST}: mcpServers.codelens.command must be a non-empty string"
                )

        for key in ("skills", "agents"):
            rel = plugin.get(key)
            if rel is None:
                continue
            directory = (repo_root / rel).resolve()
            if not directory.is_dir() or not any(directory.iterdir()):
                errors.append(
                    f"{PLUGIN_MANIFEST}: {key} path '{rel}' is not a non-empty directory"
                )

    market, err = _load_json(repo_root / MARKETPLACE_MANIFEST, MARKETPLACE_MANIFEST)
    if err:
        errors.append(err)
    if isinstance(market, dict):
        for field in REQUIRED_MARKET_FIELDS:
            if field not in market:
                errors.append(f"{MARKETPLACE_MANIFEST}: missing required field '{field}'")

        plugins = market.get("plugins")
        if not isinstance(plugins, list) or not plugins:
            errors.append(f"{MARKETPLACE_MANIFEST}: 'plugins' must be a non-empty array")
        else:
            plugin_name = plugin.get("name") if isinstance(plugin, dict) else None
            for i, item in enumerate(plugins):
                if not isinstance(item, dict):
                    errors.append(f"{MARKETPLACE_MANIFEST}: plugins[{i}] must be an object")
                    continue
                if not item.get("source"):
                    errors.append(f"{MARKETPLACE_MANIFEST}: plugins[{i}].source missing")
                if not item.get("name"):
                    errors.append(f"{MARKETPLACE_MANIFEST}: plugins[{i}].name missing")
                elif (
                    item.get("source") == "./"
                    and plugin_name is not None
                    and item["name"] != plugin_name
                ):
                    errors.append(
                        f"{MARKETPLACE_MANIFEST}: plugins[{i}].name '{item['name']}' "
                        f"does not match plugin.json name '{plugin_name}'"
                    )

    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help="validate manifests; exit non-zero on any error (CI gate)",
    )
    parser.parse_args()

    errors = collect_manifest_errors(REPO_ROOT)
    if errors:
        print("Plugin manifest validation FAILED:")
        for entry in errors:
            print(f"  - {entry}")
        return 1
    print("Plugin manifest validation OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
```

- [ ] **Step 4: Run test to verify it passes**

Run: `python3 scripts/test/test-validate-plugin-manifest.py`
Expected: 8 lines of `PASS  ...`, exit code 0.

- [ ] **Step 5: Commit**

```bash
git add scripts/validate-plugin-manifest.py scripts/test/test-validate-plugin-manifest.py
git commit -m "feat(sp4): add .claude-plugin manifest validator (TDD)"
```

---

## Task 2: Plugin + marketplace manifests

**Files:**
- Create: `.claude-plugin/plugin.json`
- Create: `.claude-plugin/marketplace.json`

> Manifests are configuration (TDD exception). The deterministic check is running the Task 1 validator against the real files.

- [ ] **Step 1: Create `.claude-plugin/plugin.json`**

```json
{
  "name": "codelens",
  "displayName": "CodeLens",
  "description": "Compressed code-intelligence MCP for planner/reviewer/refactor harnesses — AST + call-graph + hybrid retrieval, mutation-gated refactoring.",
  "version": "1.0.0",
  "author": { "name": "mupozg823" },
  "homepage": "https://github.com/mupozg823/codelens-mcp-plugin",
  "repository": "https://github.com/mupozg823/codelens-mcp-plugin",
  "license": "Apache-2.0",
  "keywords": ["code-intelligence", "tree-sitter", "mcp", "ast", "refactoring"],
  "mcpServers": { "codelens": { "command": "codelens-mcp" } },
  "skills": "./skills/",
  "agents": "./agents/"
}
```

- [ ] **Step 2: Create `.claude-plugin/marketplace.json`**

```json
{
  "name": "codelens",
  "owner": { "name": "mupozg823" },
  "plugins": [
    {
      "name": "codelens",
      "source": "./",
      "description": "CodeLens MCP + analyze/review/onboard skills + read-only explorer agent."
    }
  ]
}
```

- [ ] **Step 3: Validate the real manifests**

Run: `python3 scripts/validate-plugin-manifest.py --check`
Expected: `Plugin manifest validation OK`, exit code 0.

- [ ] **Step 4: Re-run validator tests (regression guard)**

Run: `python3 scripts/test/test-validate-plugin-manifest.py`
Expected: all `PASS`, exit 0 (tests use temp fixtures and must be unaffected by the new real files).

- [ ] **Step 5: Commit**

```bash
git add .claude-plugin/plugin.json .claude-plugin/marketplace.json
git commit -m "feat(sp4): add CodeLens Claude Code plugin + marketplace manifests"
```

---

## Task 3: Wire the validator into CI

**Files:**
- Modify: `.github/workflows/ci.yml` (insert immediately after the `surface manifest drift check` step, currently ending at line 56)

- [ ] **Step 1: Add the CI step**

Insert this block right after the existing `surface manifest drift check` step (after its `run:` line) and before the `bench invocation lint` step:

```yaml
      - name: plugin manifest validation
        run: python3 scripts/validate-plugin-manifest.py --check
```

Resulting order:
```yaml
      - name: surface manifest drift check
        run: python3 scripts/surface-manifest.py --check

      - name: plugin manifest validation
        run: python3 scripts/validate-plugin-manifest.py --check

      - name: bench invocation lint
```

(No CI step is needed for the validator's tests — the existing `script contract tests` step runs every `scripts/test/test-*.py`, which now includes `test-validate-plugin-manifest.py`.)

- [ ] **Step 2: Verify the validator step command locally**

Run: `python3 scripts/validate-plugin-manifest.py --check`
Expected: `Plugin manifest validation OK`, exit 0.

- [ ] **Step 3: Verify the contract-test glob picks up the new test**

Run:
```bash
for test in scripts/test/test-*.py; do echo "== $test"; python3 "$test"; done
```
Expected: every file prints `PASS` lines; `test-validate-plugin-manifest.py` appears in the list; overall exit 0.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci(sp4): gate plugin manifests with validate-plugin-manifest --check"
```

---

## Task 4: README "Claude Code Plugin" section

**Files:**
- Modify: `README.md` (insert a new `## Claude Code Plugin` section immediately after the `### Install Channel Matrix` table, which ends around line 96, and before the next top-level `## ` heading)

- [ ] **Step 1: Insert the section**

Add this Markdown:

```markdown
## Claude Code Plugin

CodeLens ships as a Claude Code plugin that wires the MCP server plus the
CodeLens-specific skills and read-only explorer agent in one install.

**Prerequisite — install the binary first.** The plugin connects to a
`codelens-mcp` binary on your `PATH`; the plugin system does not build it.
The recommended path is the installer or a GitHub Release tarball, which
bundle the semantic model so `semantic_search` and hybrid retrieval work
out of the box:

```bash
curl -fsSL https://raw.githubusercontent.com/mupozg823/codelens-mcp-plugin/main/install.sh | bash
```

A leaner `cargo install codelens-mcp` also works but provides BM25 + AST +
call-graph only (no semantic model; `semantic_search` is gracefully absent
until you add `--features semantic` and a model directory — see the
[Install Channel Matrix](#install-channel-matrix)).

**Install the plugin:**

```text
/plugin marketplace add mupozg823/codelens-mcp-plugin
/plugin install codelens@codelens
```

**What you get:** the `mcp__codelens__*` tools, the `codelens-analyze`,
`codelens-review`, and `codelens-onboard` skills, and the read-only
`codelens-explorer` agent.

**If the tools don't appear** after install, the binary isn't on your
`PATH`. Verify the install with:

```bash
codelens-mcp doctor
```

**Optional — post-edit diagnostics.** The repo ships
`hooks/post-edit-diagnostics.sh`, which runs CodeLens diagnostics on each
edited file. It is **not** auto-installed by the plugin. To enable it, add a
`PostToolUse` hook for the `Edit` matcher pointing at that script in your
Claude Code settings.
```

- [ ] **Step 2: Verify Markdown placement and links**

Run:
```bash
grep -n "## Claude Code Plugin" README.md
grep -n "plugin marketplace add" README.md
```
Expected: the new section appears once, after the Install Channel Matrix; the `/plugin marketplace add` line is present.

- [ ] **Step 3: Re-run the docs contract test (no removed-alias leakage)**

Run: `python3 scripts/test/test-current-docs-tool-surface.py`
Expected: all `PASS` (the new section introduces no removed workflow aliases).

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "docs(sp4): document Claude Code plugin install + prerequisite"
```

---

## Task 5: Final verification gate

**Files:** none (verification only)

- [ ] **Step 1: Deterministic gate — validator + all script contract tests**

Run:
```bash
python3 scripts/validate-plugin-manifest.py --check
for test in scripts/test/test-*.py; do python3 "$test"; done
```
Expected: validator prints `Plugin manifest validation OK`; every contract test prints `PASS`; combined exit 0.

- [ ] **Step 2: Confirm pre-existing drift gates are unaffected**

Run:
```bash
python3 scripts/regen-tool-defs.py --check
python3 scripts/surface-manifest.py --check
```
Expected: both report no drift, exit 0 (no Rust source or tool definitions were touched).

- [ ] **Step 3: Sanity — no Rust source changed**

Run: `git diff --name-only main...HEAD`
Expected: only `.claude-plugin/*`, `scripts/validate-plugin-manifest.py`, `scripts/test/test-validate-plugin-manifest.py`, `.github/workflows/ci.yml`, `README.md`, and the `docs/superpowers/{specs,plans}/*` files. No `crates/**` paths.

- [ ] **Step 4: Manual install probe (user-run, external dependency)**

In a Claude Code session (fresh — plugin/MCP registration is session-scoped):
```text
/plugin marketplace add /Users/bagjaeseog/codelens-mcp-plugin/.worktrees/sp4-plugin-packaging
/plugin install codelens@codelens
```
Then confirm `mcp__codelens__*` tools, the three skills, and the `codelens-explorer` agent are exposed. This step requires a human-driven session restart and is flagged as a manual checkpoint, not an automated gate.

---

## Self-Review

**Spec coverage:**
- D1 stdio connection → Task 2 `mcpServers.codelens.command`; validated in Task 1 (`test_bad_mcp_command_reported`). ✓
- D2 bundle skills+agent, no hooks → Task 2 `skills`/`agents` keys, no `hooks` key; README documents post-edit opt-in (Task 4). ✓
- D3 install.sh/Release prerequisite, semantic out-of-box → Task 4 README prerequisite. ✓
- D4 in-repo marketplace → Task 2 `marketplace.json`; install flow in Task 4. ✓
- D5 manifest location `.claude-plugin/{plugin,marketplace}.json` → Task 2. ✓
- A1 plugin version `1.0.0` → Task 2 plugin.json. ✓
- A2 CI gate → Task 3. ✓
- A3 failure mode documented (`doctor`) → Task 4. ✓
- Validator (validate-plugin-manifest.py) → Task 1. ✓

**Placeholder scan:** No TBD/TODO; every code/step shows full content. ✓

**Type consistency:** `collect_manifest_errors(repo_root) -> list[str]` is defined in Task 1 Step 3 and imported by the Task 1 Step 1 test under the same name; `--check` flag consistent across Tasks 1/3/5; manifest field names (`mcpServers`, `skills`, `agents`, `plugins`, `source`, `name`) consistent across validator, manifests, and tests. ✓
