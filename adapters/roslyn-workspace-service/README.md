# CodeLens Roslyn Workspace Service

This optional sidecar is the first-party `semantic_edit_backend=roslyn` adapter.
It reads the CodeLens semantic adapter JSON protocol on stdin and returns an
inspectable LSP `WorkspaceEdit` on stdout.

Implemented operation:

- `rename_symbol` / `rename`: uses Roslyn symbol binding and rename APIs over an
  MSBuild solution/project when available, with a file-based C# workspace fallback
  for small fixtures.

Build:

```bash
dotnet build adapters/roslyn-workspace-service/CodeLens.Roslyn.WorkspaceService/CodeLens.Roslyn.WorkspaceService.csproj
```

Use with CodeLens:

```bash
export CODELENS_ROSLYN_ADAPTER_CMD="dotnet"
export CODELENS_ROSLYN_ADAPTER_ARGS="run --quiet --project /path/to/codelens/adapters/roslyn-workspace-service/CodeLens.Roslyn.WorkspaceService"
```

The adapter is intentionally fail-closed: unsupported operations or operations
that cannot produce a concrete `WorkspaceEdit` return an error instead of falling
back to approximate text edits.
