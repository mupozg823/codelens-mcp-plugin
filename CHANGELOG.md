# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
