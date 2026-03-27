# CodeLens Architecture Roadmap

Updated: 2026-03-27

## Goal

CodeLens should become a Serena-compatible MCP backend with two operating modes:

1. JetBrains-native mode
   Uses IntelliJ PSI, inspections, refactoring, open editor state, and IDE execution features.
2. Workspace mode
   Runs without JetBrains so it can be used from VS Code, Cursor, Antigravity, Codex, Claude Code, and other MCP-capable clients.

The public tool contract should stay stable across both modes. When JetBrains is available, CodeLens should provide a higher-precision backend instead of a different product.

## Hard Truths

- JetBrains-level precision is only possible when a JetBrains backend is active.
- A standalone VS Code extension cannot reproduce IntelliJ PSI behavior by itself.
- "Use anywhere" is realistic.
- "Same interface everywhere, best backend when available" is realistic.
- "Same performance everywhere" is not realistic.

## Product Positioning

CodeLens should not try to replace Serena, Junie, or JetBrains AI Assistant as end-user agents.

CodeLens should provide:

- Serena-compatible symbolic tools
- JetBrains-native IDE intelligence when available
- a backend abstraction that allows non-JetBrains clients to keep using the same tool names

This makes CodeLens the infrastructure layer that agents can attach to.

## Target Architecture

### 1. Core Layer

Create a transport-agnostic core that owns:

- tool names
- request and response schemas
- memory and onboarding semantics
- project activation semantics
- capability negotiation
- backend selection

The core must not depend on IntelliJ classes.

Suggested package target:

```text
src/main/kotlin/com/codelens/core/
  api/
  contract/
  activation/
  memories/
  backend/
  transport/
```

### 2. Backend Layer

Backends implement the same symbolic and file operations.

#### JetBrains backend

Responsibilities:

- PSI symbol extraction
- reference search
- type hierarchy
- refactoring-backed symbol rename
- IDE run configurations
- inspections and editor state

Current code that belongs here:

- `com.codelens.services.*`
- `com.codelens.util.PsiUtils`
- `com.codelens.plugin.*`

Suggested package target:

```text
src/main/kotlin/com/codelens/backend/jetbrains/
  symbols/
  references/
  files/
  inspections/
  editor/
  run/
```

#### Workspace backend

Responsibilities:

- standalone project activation
- file reads and writes
- ripgrep-based file and pattern search
- symbolic operations via language servers, tree-sitter, or backend-specific indexers

This backend is required for:

- VS Code
- Cursor
- Antigravity
- Codex CLI
- Claude Code
- headless CI use

Suggested package target:

```text
src/main/kotlin/com/codelens/backend/workspace/
  files/
  search/
  symbols/
  activation/
```

The first version can be degraded and should prioritize compatibility over perfect precision.

### 3. Transport Layer

Transport must be thin and backend-agnostic.

Supported transports:

- JetBrains MCP extension point
- stdio MCP server
- HTTP/SSE compat server

Current code that belongs here:

- `com.codelens.tools.*`
- `com.codelens.tools.adapters.*`
- `com.codelens.serena.SerenaCompatServer`
- `jetbrains_sse_bridge.py`

Suggested package target:

```text
src/main/kotlin/com/codelens/transport/
  mcp/
  serena/
  sse/
```

## Public Tool Strategy

The public interface should be stable even when the backend changes.

### Tier A: must-match Serena

These tools are the compatibility baseline:

- `activate_project`
- `check_onboarding_performed`
- `initial_instructions`
- `list_memories`
- `read_memory`
- `write_memory`
- `find_symbol`
- `find_referencing_symbols`
- `get_symbols_overview`
- `search_for_pattern`
- `replace_symbol_body`
- `insert_after_symbol`
- `insert_before_symbol`
- `rename_symbol`

### Tier B: JetBrains-enhanced Serena compatibility

These should exist as either aliases or capability-specific tools:

- `jet_brains_find_symbol`
- `jet_brains_find_referencing_symbols`
- `jet_brains_get_symbols_overview`
- `jet_brains_type_hierarchy`

If Serena clients expect these names, CodeLens should expose them directly instead of forcing client-side remapping.

### Tier C: CodeLens-native tools

These are worth keeping as value-add tools:

- `get_open_files`
- `get_file_problems`
- `get_run_configurations`
- `execute_run_configuration`
- `open_file_in_editor`
- `get_project_modules`
- `get_project_dependencies`
- `get_repositories`

These should be advertised through capabilities instead of polluting the minimal Serena profile when the client asks for a lean context.

## Current Repo Audit

### What already fits the long-term vision

- Tool count is already broad and useful.
- Serena-compatible memory and onboarding flow exists.
- JetBrains MCP tool registration exists.
- A Serena-compatible HTTP layer exists.
- JetBrains PSI-backed services are already separated from tool wrappers.

Relevant files:

- [`ToolRegistry.kt`](/Users/bagjaeseog/codelens-mcp-plugin/src/main/kotlin/com/codelens/tools/ToolRegistry.kt)
- [`SerenaCompatServer.kt`](/Users/bagjaeseog/codelens-mcp-plugin/src/main/kotlin/com/codelens/serena/SerenaCompatServer.kt)
- [`plugin.xml`](/Users/bagjaeseog/codelens-mcp-plugin/src/main/resources/META-INF/plugin.xml)

### What is still tightly coupled

- Tools call JetBrains-backed implementations directly.
- There is no backend interface that could swap JetBrains for workspace mode.
- Serena compatibility is partial and mostly name-level today.
- The compatibility HTTP server duplicates file/search logic that should live in a shared backend.

## Gaps to Close

### Serena parity gaps

- Parse and honor `.serena/project.yml`
- Parse and honor `~/.serena/serena_config.yml`
- Support backend selection semantics such as `language_backend: JetBrains`
- Add missing `jet_brains_*` tool aliases if Serena clients rely on them
- Align onboarding and dashboard semantics where practical

### JetBrains parity gaps

- PSI-backed search should use indexes more aggressively
- editor and inspection data should expose richer detail
- language coverage should move beyond Java and Kotlin
- structural search and replace should be added later

### Product gaps

- no standalone MCP server without JetBrains
- no VS Code extension or thin client packaging
- no capability negotiation yet
- no formal contract tests shared across backends

## Recommended Execution Order

### Phase 1: Serena backend compatibility

Goal:

- Make CodeLens a drop-in JetBrains backend for Serena-oriented workflows.

Work:

- add `jet_brains_*` aliases
- read and validate `.serena/project.yml`
- read and validate `~/.serena/serena_config.yml`
- define a backend capability model
- add contract tests for the Serena-compatible tools

This phase should happen before any VS Code client work.

### Phase 2: Extract a backend interface

Goal:

- Stop binding tools directly to IntelliJ services.

Work:

- introduce `CodeLensBackend`
- split transport from backend
- move JetBrains file/search/symbol logic behind the interface
- route `ToolRegistry` through the backend contract instead of concrete services

Proposed interface groups:

- `ProjectBackend`
- `MemoryBackend`
- `SymbolBackend`
- `ReferenceBackend`
- `FileBackend`
- `ExecutionBackend`

### Phase 3: Build workspace mode

Goal:

- Run CodeLens without JetBrains.

Work:

- standalone stdio MCP server
- standalone project activation
- file and pattern tools using local filesystem plus `rg`
- minimal symbolic support using LSP or tree-sitter
- degrade unsupported operations cleanly instead of failing ambiguously

This is the phase that unlocks VS Code, Cursor, Antigravity, and terminal-first use.

### Phase 4: Thin clients

Goal:

- Package the same backend for common hosts.

Work:

- VS Code extension that launches or connects to CodeLens
- MCP configs and launcher scripts for Cursor and Antigravity
- optional standalone desktop dashboard later

The thin clients should stay small. The backend is the product.

### Phase 5: Performance and quality

Goal:

- approach JetBrains and Serena usability parity

Work:

- index-backed symbol search
- caching and invalidation
- multi-module awareness
- richer diagnostics
- contract tests across backends
- end-to-end smoke tests against external MCP clients

## Immediate Next Implementation Targets

These are the highest-signal next steps for this repo:

1. Add a backend abstraction without changing the public tool names.
2. Add Serena JetBrains backend compatibility for `.serena/project.yml` and `language_backend: JetBrains`.
3. Add `jet_brains_*` aliases that forward to the current JetBrains-backed symbol services.
4. Move duplicated file and search logic out of `SerenaCompatServer` into shared backend services.
5. Document capability differences between JetBrains mode and workspace mode in the README.

## Definition of Success

CodeLens is on the right path when all of the following are true:

- Serena-oriented clients can use CodeLens with minimal prompt changes.
- JetBrains mode is clearly the high-precision backend.
- Workspace mode can run without JetBrains and still provide useful MCP tools.
- Tool names and schemas stay stable across backends.
- Thin clients remain thin and avoid embedding core logic.
