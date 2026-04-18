# Migrate From CodeLens To Symbiote

Status: pre-cutover migration contract for the future v2.0 rename  
Last updated: 2026-04-18  
Related: [ADR-0007](adr/ADR-0007-symbiote-rebrand.md), [Platform setup](platform-setup.md), [Host-adaptive harness](host-adaptive-harness.md)

As of 2026-04-18, the public primary install name is still `codelens-mcp`.
This document exists so the future rename to `symbiote-mcp` is explicit,
host-by-host, and reversible instead of being reconstructed from chat
history or memory.

## What Changes

At the public v2.0 cutover, these names are expected to change together:

| Surface | Current | Target |
| --- | --- | --- |
| Binary | `codelens-mcp` | `symbiote-mcp` |
| Recommended MCP server id | `codelens` | `symbiote` |
| Resource URI prefix | `codelens://` | `symbiote://` |
| Environment prefix | `CODELENS_*` | `SYMBIOTE_*` |
| Runtime state dir | `.codelens/` | `.symbiote/` |
| Repository name | `codelens-mcp-plugin` | `symbiote-mcp` |
| Workspace crate names | `codelens-*` | `symbiote-*` |

## What Already Works Before Cutover

These compatibility paths already exist in the current repository:

- `symbiote://...` resolves as an alias of `codelens://...`.
- `SYMBIOTE_*` environment variables are accepted alongside `CODELENS_*`.
- `codelens-mcp attach <host>` and `codelens-mcp detach <host>` already emit the host-specific routing/config contract that the future rename will preserve.

That means migration can be done in two phases:

1. Start consuming `symbiote://` and `SYMBIOTE_*` now if you want lower cutover risk.
2. Switch the binary name and host server ids when the public rename actually ships.

## Cutover Checklist

Use this sequence when the public rename becomes available.

1. Upgrade the binary from `codelens-mcp` to `symbiote-mcp`.
2. Replace host config entries from `codelens` to `symbiote`.
3. Swap executable references from `codelens-mcp` to `symbiote-mcp`.
4. Swap `codelens://` resource references to `symbiote://`.
5. Swap `CODELENS_*` environment variables to `SYMBIOTE_*`.
6. If you persist runtime state, rename `.codelens/` to `.symbiote/` only after confirming the new binary understands the migrated layout.
7. Keep the old names only for the published compatibility window; do not leave both names in permanent docs or infra once the migration is complete.

## Host-By-Host Diffs

These diffs show the intended steady-state after the rename.
If you want a zero-risk rollout during the compatibility window, you may keep
the old server id key and only change the command first.

### Claude Code

Global or per-project MCP config:

```diff
 {
   "mcpServers": {
-    "codelens": {
+    "symbiote": {
       "type": "http",
-      "url": "http://127.0.0.1:7837/mcp"
+      "url": "http://127.0.0.1:7837/mcp"
     }
   }
 }
```

If you use stdio instead of HTTP:

```diff
 {
   "mcpServers": {
-    "codelens": {
+    "symbiote": {
       "type": "stdio",
-      "command": "codelens-mcp",
+      "command": "symbiote-mcp",
       "args": ["."]
     }
   }
 }
```

Project instructions:

```diff
-# CodeLens Routing
+# Symbiote Routing
```

### Codex

Codex config:

```diff
-[mcp_servers.codelens]
+[mcp_servers.symbiote]
 url = "http://127.0.0.1:7837/mcp"
```

No URL change is required if you keep the same daemon.
The meaningful change is the binary/server naming around it:

```diff
-codelens-mcp attach codex
+symbiote-mcp attach codex
```

Repo policy:

```diff
-# CodeLens Routing
+# Symbiote Routing
```

### Cursor

Project or global MCP config:

```diff
 {
   "mcpServers": {
-    "codelens": {
+    "symbiote": {
       "type": "http",
       "url": "http://127.0.0.1:7837/mcp"
     }
   }
 }
```

Rule file names can stay stable if you prefer, but the recommended rename is:

```diff
-.cursor/rules/codelens-routing.mdc
+.cursor/rules/symbiote-routing.mdc
```

Rule heading:

```diff
-description: Route CodeLens usage by task risk and phase
+description: Route Symbiote usage by task risk and phase
```

### Cline

Project MCP config:

```diff
 {
-  "codelens": {
+  "symbiote": {
     "type": "http",
     "url": "http://127.0.0.1:7837/mcp"
   }
 }
```

Rules:

```diff
-# CodeLens Routing
+# Symbiote Routing
```

### Windsurf

Global MCP config:

```diff
 {
   "mcpServers": {
-    "codelens": {
+    "symbiote": {
       "type": "http",
       "url": "http://127.0.0.1:7837/mcp"
     }
   }
 }
```

If you stay on stdio:

```diff
 {
-  "codelens": {
+  "symbiote": {
-    "command": "codelens-mcp",
+    "command": "symbiote-mcp",
     "args": [".", "--profile", "builder-minimal"],
     "transport": "stdio"
   }
 }
```

### VS Code

`.vscode/mcp.json`:

```diff
 {
   "servers": {
-    "codelens": {
+    "symbiote": {
       "type": "http",
       "url": "http://127.0.0.1:7837/mcp"
     }
   }
 }
```

### JetBrains

Server display name:

```diff
-Name: codelens
+Name: symbiote
 URL: http://127.0.0.1:7837/mcp
 Transport: HTTP
```

### CI / Scripts

Binary calls:

```diff
-codelens-mcp . --cmd get_capabilities --args '{}'
+symbiote-mcp . --cmd get_capabilities --args '{}'
```

HTTP daemon startup:

```diff
-codelens-mcp /path/to/project --transport http --profile reviewer-graph --daemon-mode read-only --port 7837
+symbiote-mcp /path/to/project --transport http --profile reviewer-graph --daemon-mode read-only --port 7837
```

Environment variables:

```diff
-CODELENS_LOG=info
-CODELENS_PROFILE=builder-minimal
-CODELENS_MODEL_DIR=/models
+SYMBIOTE_LOG=info
+SYMBIOTE_PROFILE=builder-minimal
+SYMBIOTE_MODEL_DIR=/models
```

Resource URIs in tests, prompts, dashboards, or scripts:

```diff
-codelens://surface/manifest
-codelens://harness/spec
-codelens://host-adapters/codex
+symbiote://surface/manifest
+symbiote://harness/spec
+symbiote://host-adapters/codex
```

### Docker / Containers

Entrypoint rename:

```diff
-ENTRYPOINT ["codelens-mcp", "/workspace", "--transport", "http", "--port", "7837"]
+ENTRYPOINT ["symbiote-mcp", "/workspace", "--transport", "http", "--port", "7837"]
```

Image names should be updated together with the repository/tag cutover, not
before it.

## Recommended Rollout Strategy

For teams with several hosts or repos, the low-risk order is:

1. Switch docs and automation from `codelens://` to `symbiote://`.
2. Switch env vars from `CODELENS_*` to `SYMBIOTE_*`.
3. Upgrade one canary host to the renamed binary and config key.
4. Upgrade the rest of the interactive hosts.
5. Upgrade CI and shared daemon launch scripts.
6. Rename persisted runtime directories only after the new binary has been validated.

## Rollback

If the public rename lands and you need to roll back during the compatibility window:

1. Revert the host MCP server id from `symbiote` to `codelens`.
2. Revert the executable name from `symbiote-mcp` to `codelens-mcp`.
3. Revert `symbiote://` references back to `codelens://` only if the consuming tool cannot follow the compatibility alias.
4. Revert `SYMBIOTE_*` to `CODELENS_*` only if the target environment has not yet picked up the alias-supporting runtime.

## Non-Goals

This migration guide does not assume:

- a new daemon port
- a new transport shape
- different harness modes
- different profile semantics
- different audit or preflight contracts

The rename is a naming and packaging cutover, not a harness-model rewrite.
