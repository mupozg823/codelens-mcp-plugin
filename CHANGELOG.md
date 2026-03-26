# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- `get_current_config` — 현재 IDE/프로젝트/도구 등록 상태와 `.serena` 관련 경로 조회
- `get_project_modules` — IntelliJ 모듈 구조, 루트, 의존성 조회
- `get_open_files` — 현재 IDE에서 열린 파일과 선택된 파일 조회
- `get_file_problems` — IntelliJ 하이라이팅 패스를 기반으로 파일 진단 조회

### Changed
- README와 smoke test 기준을 22개 도구 집합으로 갱신

## [0.2.1] - 2026-03-27

### Added
- `jetbrains_sse_bridge.py` — repo-local stdio bridge for the JetBrains IDE SSE MCP transport
- Direct IDE smoke test coverage in `test-mcp-tools.py` without external Python packages

### Changed
- Connection guidance now prefers the bundled bridge when generic MCP proxies are incompatible

### Fixed
- `McpToolAdapter` now preserves numeric and boolean argument types instead of coercing all primitives to strings
- `read_file` line ranges now work correctly when invoked through the MCP adapter layer

## [0.2.0] - 2026-03-27

### Added
- `get_type_hierarchy` — 클래스의 상속/구현 관계와 멤버 구조 조회
- `find_referencing_code_snippets` — 참조 지점 주변의 코드 스니펫 조회
- File operations:
  - `read_file`, `list_dir`, `find_file`
  - `create_text_file`, `delete_lines`, `insert_at_line`, `replace_lines`, `replace_content`

### Changed
- Type hierarchy tool name aligned to Serena-compatible `get_type_hierarchy`

## [0.1.0] - 2026-03-26

### Added
- Initial project structure with Gradle IntelliJ Platform Plugin 2.x
- **Symbol Analysis Tools**
  - `get_symbols_overview` — file/directory symbol structure overview
  - `find_symbol` — search symbols by name with optional body
  - `find_referencing_symbols` — trace all references to a symbol
  - `search_for_pattern` — regex-based code search
- **Symbol Modification Tools**
  - `replace_symbol_body` — replace symbol body with new code
  - `insert_after_symbol` — insert code after a symbol
  - `insert_before_symbol` — insert code before a symbol
  - `rename_symbol` — IDE refactoring-based rename
- **Language Adapters**
  - Java adapter with full PSI support
  - Kotlin adapter with full PSI support
  - Generic fallback adapter for other languages
- **Plugin Infrastructure**
  - Settings page showing registered tools and connection info
  - Startup notification
  - Tools menu with Restart/Status actions
- Serena-compatible tool names and parameters
