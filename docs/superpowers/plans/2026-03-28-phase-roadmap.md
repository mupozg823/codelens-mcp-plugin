# CodeLens MCP Plugin — Phase Roadmap

## Phase 1: Rust Bridge Expansion (현재 → 다음)

**목표:** 현재 8개 도구만 Rust 위임 → editor-independent 도구 전체를 Rust로 위임

**현재 브릿지 완료:**

- get_symbols_overview, find_symbol, find_referencing_symbols
- find_referencing_code_snippets, search_for_pattern
- get_blast_radius, get_ranked_context, get_type_hierarchy

**추가 위임 대상:**

- File ops: read_file, list_dir, find_file, create_text_file, delete_lines, insert_at_line, replace_lines, replace_content
- Git: get_diff_symbols, get_changed_files
- Analysis: get_complexity, find_tests, find_annotations, find_importers, get_symbol_importance, find_dead_code
- Symbol edit: replace_symbol_body, insert_after_symbol, insert_before_symbol, rename_symbol

**산출물:** RustMcpBridge에 새 메서드 추가, SymbolToolHandler/FileToolHandler/GitToolHandler/AnalysisToolHandler에서 Rust 우선 위임

---

## Phase 2: MCP Streamable HTTP

**목표:** stdio 기반 → HTTP 스트리밍 전환 (설계문서 완료: `specs/2026-03-28-mcp-streamable-http-design.md`)

**핵심:**

- SSE 기반 서버→클라이언트 스트리밍
- 기존 stdio 모드 유지 (하위 호환)
- Claude Code / Cursor 등 MCP 클라이언트 호환

---

## Phase 3: Rust Symbol Parsing Expansion

**목표:** tree-sitter 파싱 언어 확대 (현재 Python/JS/TS/TSX → +Kotlin/Java/Go/Rust/Ruby/C/C++)

**핵심:**

- codelens-core에 언어별 tree-sitter 쿼리 추가
- 심볼 인덱스가 새 언어를 커버
- Kotlin standalone의 tree-sitter 백엔드는 이미 14언어 지원 → Rust 쪽도 동일하게 맞춤

---

## Phase 4: Metadata Contract Unification

**목표:** Rust MCP의 `backend_used`/`confidence`/`degraded_reason` 엔벨로프를 Kotlin/Serena 응답에도 통일

**핵심:**

- 모든 도구 응답에 메타데이터 엔벨로프 추가
- 클라이언트가 어떤 백엔드(PSI/tree-sitter/workspace/Rust)가 결과를 생성했는지 투명하게 확인

---

## Phase 5: Symbol Index Structured Store

**목표:** JSON 파일 기반 심볼 인덱스 → SQLite 또는 RocksDB

**핵심:**

- 대규모 프로젝트 성능 개선
- 증분 업데이트 지원
- 쿼리 성능 향상 (현재 전체 로드 후 필터링)

---

## Phase 6: JetBrains Marketplace 배포

**목표:** 플러그인 마켓플레이스 등록 + 자동 업데이트

**핵심:**

- plugin.xml 메타데이터 정리
- 서명 및 호환성 검증
- CI/CD 파이프라인 (GitHub Actions → Marketplace 자동 배포)

---

## 실행 순서 근거

```
Phase 1 (Rust Bridge) → 기존 인프라 활용, 기계적 확장
  ↓
Phase 2 (Streamable HTTP) → 설계 완료, 통신 레이어 현대화
  ↓
Phase 3 (Parsing Expansion) → Rust가 더 많은 도구를 처리하려면 더 많은 언어 필요
  ↓
Phase 4 (Metadata) → 응답 품질 투명성, 클라이언트 신뢰도 향상
  ↓
Phase 5 (Index Store) → 대규모 프로젝트 스케일링
  ↓
Phase 6 (Marketplace) → 외부 배포
```

각 Phase는 독립적으로 릴리스 가능. Phase 1이 완료되면 Rust가 주 런타임, IntelliJ는 PSI 어댑터로 전환.
