# Rust Tree-Sitter Language Expansion Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rust codelens-core의 tree-sitter 심볼 파싱을 4언어(Python/JS/TS/TSX)에서 14언어로 확대하여 Kotlin과 동일한 언어 커버리지를 달성한다.

**Architecture:** `rust/crates/codelens-core/src/symbols.rs`에 새 언어 쿼리를 추가하고 `language_for_path()`에 확장자 매핑을 추가한다. Kotlin 쪽의 tree-sitter 쿼리(`TreeSitterSymbolParser.kt`)를 레퍼런스로 사용하되, Rust tree-sitter 노드 타입에 맞게 조정한다.

**Tech Stack:** Rust, tree-sitter crates (0.23~0.25), S-expression queries

---

## 추가 대상 (10개 언어, 3배치)

| Batch            | 언어                      | 크레이트                                                                                                      | 확장자                                     |
| ---------------- | ------------------------- | ------------------------------------------------------------------------------------------------------------- | ------------------------------------------ |
| 1 (brace-scoped) | Go, Java, Kotlin, Rust    | tree-sitter-go 0.25, tree-sitter-java 0.23, tree-sitter-kotlin 0.3.8, tree-sitter-rust 0.24                   | .go .java .kt .kts .rs                     |
| 2 (C-family)     | C, C++, PHP, Swift, Scala | tree-sitter-c 0.24, tree-sitter-cpp 0.23, tree-sitter-php 0.24, tree-sitter-swift 0.7, tree-sitter-scala 0.25 | .c .h .cpp .cc .hpp .php .swift .scala .sc |
| 3 (end-keyword)  | Ruby                      | tree-sitter-ruby 0.23                                                                                         | .rb                                        |

---

### Task 1: Cargo 의존성 추가 + 빌드 확인

**Files:**

- Modify: `rust/Cargo.toml`
- Modify: `rust/crates/codelens-core/Cargo.toml`

- [ ] **Step 1: Add workspace dependencies to `rust/Cargo.toml`**

`[workspace.dependencies]` 섹션에 추가:

```toml
tree-sitter-go = "0.25"
tree-sitter-java = "0.23"
tree-sitter-kotlin = "0.3.8"
tree-sitter-rust = "0.24"
tree-sitter-c = "0.24"
tree-sitter-cpp = "0.23"
tree-sitter-php = "0.24"
tree-sitter-swift = "0.7"
tree-sitter-scala = "0.25"
tree-sitter-ruby = "0.23"
```

- [ ] **Step 2: Add crate dependencies to `rust/crates/codelens-core/Cargo.toml`**

```toml
tree-sitter-go.workspace = true
tree-sitter-java.workspace = true
tree-sitter-kotlin.workspace = true
tree-sitter-rust.workspace = true
tree-sitter-c.workspace = true
tree-sitter-cpp.workspace = true
tree-sitter-php.workspace = true
tree-sitter-swift.workspace = true
tree-sitter-scala.workspace = true
tree-sitter-ruby.workspace = true
```

- [ ] **Step 3: Build to verify dependencies resolve**

```bash
cd rust && cargo check 2>&1 | tail -5
```

Expected: compiles successfully (no query code yet, just dependency resolution)

- [ ] **Step 4: Commit**

```bash
git add rust/Cargo.toml rust/crates/codelens-core/Cargo.toml rust/Cargo.lock
git commit -m "deps: add tree-sitter grammar crates for 10 new languages"
```

---

### Task 2: Batch 1 — Go, Java, Kotlin, Rust queries

**Files:**

- Modify: `rust/crates/codelens-core/src/symbols.rs`

**Reference:** Kotlin queries in `src/main/kotlin/com/codelens/backend/treesitter/TreeSitterSymbolParser.kt`

- [ ] **Step 1: Add Go query and language config**

Add `GO_QUERY` constant. Go uses `type_declaration`, `function_declaration`, `method_declaration`, `type_spec`:

```rust
const GO_QUERY: &str = r#"
(function_declaration name: (identifier) @function.name) @function.def
(method_declaration name: (field_identifier) @method.name) @method.def
(type_declaration (type_spec name: (type_identifier) @class.name)) @class.def
(var_declaration (var_spec name: (identifier) @variable.name)) @variable.def
(const_declaration (const_spec name: (identifier) @variable.name)) @variable.def
"#;
```

Add to `language_for_path()`:

```rust
"go" => Some(LanguageConfig {
    extension: "go",
    language: tree_sitter_go::LANGUAGE.into(),
    query: GO_QUERY,
}),
```

- [ ] **Step 2: Add Java query and language config**

```rust
const JAVA_QUERY: &str = r#"
(class_declaration name: (identifier) @class.name) @class.def
(interface_declaration name: (identifier) @interface.name) @interface.def
(enum_declaration name: (identifier) @enum.name) @enum.def
(method_declaration name: (identifier) @method.name) @method.def
(constructor_declaration name: (identifier) @method.name) @method.def
(field_declaration declarator: (variable_declarator name: (identifier) @variable.name)) @variable.def
"#;
```

Extensions: `"java"`

- [ ] **Step 3: Add Kotlin query and language config**

```rust
const KOTLIN_QUERY: &str = r#"
(class_declaration (type_identifier) @class.name) @class.def
(object_declaration (type_identifier) @class.name) @class.def
(interface_declaration (type_identifier) @interface.name) @interface.def
(function_declaration (simple_identifier) @function.name) @function.def
(property_declaration (variable_declaration (simple_identifier) @variable.name)) @variable.def
(type_alias (type_identifier) @typealias.name) @typealias.def
"#;
```

Extensions: `"kt"`, `"kts"`

- [ ] **Step 4: Add Rust query and language config**

```rust
const RUST_QUERY: &str = r#"
(struct_item name: (type_identifier) @class.name) @class.def
(enum_item name: (type_identifier) @enum.name) @enum.def
(trait_item name: (type_identifier) @interface.name) @interface.def
(impl_item type: (type_identifier) @class.name) @class.def
(function_item name: (identifier) @function.name) @function.def
(const_item name: (identifier) @variable.name) @variable.def
(static_item name: (identifier) @variable.name) @variable.def
(type_item name: (type_identifier) @typealias.name) @typealias.def
"#;
```

Extensions: `"rs"`

- [ ] **Step 5: Run tests**

```bash
cd rust && cargo test -p codelens-core 2>&1 | tail -10
```

Expected: existing tests pass, no regressions

- [ ] **Step 6: Write test fixtures for Batch 1**

Create test fixtures and test functions in `symbols.rs` `#[cfg(test)]` module for Go, Java, Kotlin, Rust symbol parsing. Each test should verify that classes, functions, and variables are correctly parsed.

- [ ] **Step 7: Run tests**

```bash
cd rust && cargo test -p codelens-core -- symbols 2>&1 | tail -20
```

Expected: all new tests pass

- [ ] **Step 8: Commit**

```bash
git add rust/crates/codelens-core/src/symbols.rs
git commit -m "feat: tree-sitter queries for Go, Java, Kotlin, Rust"
```

---

### Task 3: Batch 2 — C, C++, PHP, Swift, Scala queries

**Files:**

- Modify: `rust/crates/codelens-core/src/symbols.rs`

- [ ] **Step 1: Add C query**

```rust
const C_QUERY: &str = r#"
(function_definition declarator: (function_declarator declarator: (identifier) @function.name)) @function.def
(struct_specifier name: (type_identifier) @class.name) @class.def
(enum_specifier name: (type_identifier) @enum.name) @enum.def
(type_definition declarator: (type_identifier) @typealias.name) @typealias.def
(declaration declarator: (init_declarator declarator: (identifier) @variable.name)) @variable.def
"#;
```

Extensions: `"c"`, `"h"`

- [ ] **Step 2: Add C++ query**

```rust
const CPP_QUERY: &str = r#"
(function_definition declarator: (function_declarator declarator: (qualified_identifier) @function.name)) @function.def
(function_definition declarator: (function_declarator declarator: (identifier) @function.name)) @function.def
(class_specifier name: (type_identifier) @class.name) @class.def
(struct_specifier name: (type_identifier) @class.name) @class.def
(enum_specifier name: (type_identifier) @enum.name) @enum.def
(namespace_definition name: (identifier) @module.name) @module.def
(type_definition declarator: (type_identifier) @typealias.name) @typealias.def
"#;
```

Extensions: `"cpp"`, `"cc"`, `"cxx"`, `"hpp"`, `"hh"`, `"hxx"`

- [ ] **Step 3: Add PHP query**

```rust
const PHP_QUERY: &str = r#"
(class_declaration name: (name) @class.name) @class.def
(interface_declaration name: (name) @interface.name) @interface.def
(trait_declaration name: (name) @interface.name) @interface.def
(enum_declaration name: (name) @enum.name) @enum.def
(function_definition name: (name) @function.name) @function.def
(method_declaration name: (name) @method.name) @method.def
(property_declaration (property_element (variable_name (name) @variable.name))) @variable.def
"#;
```

Extensions: `"php"`

- [ ] **Step 4: Add Swift query**

```rust
const SWIFT_QUERY: &str = r#"
(class_declaration name: (type_identifier) @class.name) @class.def
(struct_declaration name: (type_identifier) @class.name) @class.def
(protocol_declaration name: (type_identifier) @interface.name) @interface.def
(enum_declaration name: (type_identifier) @enum.name) @enum.def
(function_declaration name: (simple_identifier) @function.name) @function.def
(property_declaration (pattern (simple_identifier) @variable.name)) @variable.def
(typealias_declaration name: (type_identifier) @typealias.name) @typealias.def
"#;
```

Extensions: `"swift"`

- [ ] **Step 5: Add Scala query**

```rust
const SCALA_QUERY: &str = r#"
(class_definition name: (identifier) @class.name) @class.def
(object_definition name: (identifier) @class.name) @class.def
(trait_definition name: (identifier) @interface.name) @interface.def
(function_definition name: (identifier) @function.name) @function.def
(val_definition pattern: (identifier) @variable.name) @variable.def
(var_definition pattern: (identifier) @variable.name) @variable.def
(type_definition name: (type_identifier) @typealias.name) @typealias.def
"#;
```

Extensions: `"scala"`, `"sc"`

- [ ] **Step 6: Update `language_for_path()` with all Batch 2 extensions**

- [ ] **Step 7: Write test fixtures and run tests**

```bash
cd rust && cargo test -p codelens-core -- symbols 2>&1 | tail -20
```

- [ ] **Step 8: Commit**

```bash
git add rust/crates/codelens-core/src/symbols.rs
git commit -m "feat: tree-sitter queries for C, C++, PHP, Swift, Scala"
```

---

### Task 4: Batch 3 — Ruby query

**Files:**

- Modify: `rust/crates/codelens-core/src/symbols.rs`

- [ ] **Step 1: Add Ruby query**

```rust
const RUBY_QUERY: &str = r#"
(class name: (constant) @class.name) @class.def
(module name: (constant) @module.name) @module.def
(method name: (identifier) @function.name) @function.def
(singleton_method name: (identifier) @function.name) @function.def
(assignment left: (identifier) @variable.name) @variable.def
"#;
```

Extensions: `"rb"`

- [ ] **Step 2: Run all tests**

```bash
cd rust && cargo test -p codelens-core -- symbols 2>&1 | tail -20
```

- [ ] **Step 3: Commit**

```bash
git add rust/crates/codelens-core/src/symbols.rs
git commit -m "feat: tree-sitter query for Ruby"
```

---

### Task 5: Import Graph Language Expansion

**Files:**

- Modify: `rust/crates/codelens-core/src/symbols.rs` (or import graph module)

Currently `supportsImportGraph` in `RustMcpBridge.kt` only supports Python/JS/TS. Check if the Rust side's import graph builder can be expanded for the new languages.

- [ ] **Step 1: Check Rust import graph code**

Read the import graph module in `rust/crates/codelens-core/src/` — identify where import patterns are defined.

- [ ] **Step 2: Add import patterns for Go, Java, Kotlin, Rust, Ruby**

Each language has distinct import syntax:

- Go: `import "path"` / `import (...)`
- Java: `import pkg.Class;`
- Kotlin: `import pkg.Class`
- Rust: `use crate::module;` / `mod module;`
- Ruby: `require "file"` / `require_relative "file"`

- [ ] **Step 3: Run tests**

```bash
cd rust && cargo test -p codelens-core 2>&1 | tail -10
```

- [ ] **Step 4: Commit**

```bash
git add rust/crates/codelens-core/
git commit -m "feat: import graph patterns for Go, Java, Kotlin, Rust, Ruby"
```

---

### Task 6: Integration Verification

- [ ] **Step 1: Full Rust build**

```bash
cd rust && cargo build 2>&1 | tail -5
```

- [ ] **Step 2: Full Rust test suite**

```bash
cd rust && cargo test 2>&1 | tail -10
```

- [ ] **Step 3: Kotlin compile check**

```bash
./gradlew compileKotlin 2>&1 | tail -5
```

- [ ] **Step 4: Kotlin test suite**

```bash
./gradlew test 2>&1 | tail -10
```

- [ ] **Step 5: Update architecture memory**

Update `.serena/memories/architecture/rust-migration.md`:

- Rust symbol parsing: 4 → 14 languages
- Import graph: expanded for Go, Java, Kotlin, Rust, Ruby

- [ ] **Step 6: Commit and tag**

```bash
git add .serena/memories/architecture/rust-migration.md
git commit -m "docs: update rust migration memory with 14-language coverage"
git tag v1.2.0-14lang
```

---

## Important Notes

- Tree-sitter 쿼리의 노드 타입은 각 grammar crate의 `grammar.js`에서 확인 가능
- 쿼리가 파싱 에러를 내면 (잘못된 노드 타입) Rust에서 panic → `Query::new()` 반환값을 반드시 에러 처리
- Kotlin 쪽 쿼리와 100% 동일할 필요 없음 — Rust grammar crate 버전에 따라 노드 타입이 다를 수 있음
- 각 언어 테스트에 최소 class + function + variable 파싱을 검증
