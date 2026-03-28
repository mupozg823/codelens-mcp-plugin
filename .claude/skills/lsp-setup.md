---
name: lsp-setup
description: Show LSP server recipes (install command, binary, args) for any language
user-invocable: true
---

# LSP Setup Guide

Use `mcp__codelens__check_lsp_status` to see which LSP servers are installed.

## Recipes

| Language   | Server               | Install                                                | Binary                       | Args                             |
| ---------- | -------------------- | ------------------------------------------------------ | ---------------------------- | -------------------------------- |
| Python     | pyright              | `npm install -g pyright`                               | `pyright-langserver`         | `--stdio`                        |
| TypeScript | typescript-ls        | `npm install -g typescript-language-server typescript` | `typescript-language-server` | `--stdio`                        |
| Rust       | rust-analyzer        | `rustup component add rust-analyzer`                   | `rust-analyzer`              |                                  |
| Go         | gopls                | `go install golang.org/x/tools/gopls@latest`           | `gopls`                      | `serve`                          |
| Java       | jdtls                | `brew install jdtls`                                   | `jdtls`                      |                                  |
| Kotlin     | kotlin-ls            | `brew install kotlin-language-server`                  | `kotlin-language-server`     |                                  |
| C/C++      | clangd               | `brew install llvm`                                    | `clangd`                     |                                  |
| Ruby       | solargraph           | `gem install solargraph`                               | `solargraph`                 | `stdio`                          |
| PHP        | intelephense         | `npm install -g intelephense`                          | `intelephense`               | `--stdio`                        |
| Swift      | sourcekit-lsp        | `xcode-select --install`                               | `sourcekit-lsp`              |                                  |
| C#         | csharp-ls            | `dotnet tool install -g csharp-ls`                     | `csharp-ls`                  |                                  |
| Dart       | dart-language-server | `dart pub global activate dart_language_server`        | `dart`                       | `language-server --protocol=lsp` |
