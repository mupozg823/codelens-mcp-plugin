# embedding/mod.rs 분해 계획 — 2026-04-17

> v1.9.32 god module 해소. 2026-04-12 audit에서 4,308줄로 지적됐던 파일이 2026-04-17 기준 **4,711줄**로 증가. 6개 책임 클러스터로 분해.

## 범위 (엄수)

**허용 파일**:

- `crates/codelens-engine/src/embedding/mod.rs` (원본, 축소됨)
- `crates/codelens-engine/src/embedding/ffi.rs` (신규)
- `crates/codelens-engine/src/embedding/runtime.rs` (신규)
- `crates/codelens-engine/src/embedding/cache.rs` (신규)
- `crates/codelens-engine/src/embedding/prompt.rs` (신규)
- `crates/codelens-engine/src/embedding/chunk_ops.rs` (신규)
- `crates/codelens-engine/src/embedding/engine_impl.rs` (신규)
- `crates/codelens-engine/src/embedding/tests.rs` 또는 `tests/` 디렉토리 (신규)

**금지**:

- `vec_store.rs` 수정
- 외부 API 시그니처 변경
- 호출자(`search.rs`, `symbols/`, MCP crate) 수정
- 새 의존성 추가
- 최적화, 리네임, 로직 변경

## 외부 API (보존 원칙)

`crate::embedding::*`로 공개된 아이템은 **정확히 동일한 경로**에서 re-export되어야 함:

```
pub use embedding::{
    SemanticMatch,
    EmbeddingEngine,
    EmbeddingIndexInfo,
    EmbeddingRuntimeInfo,
    DuplicatePair,
    CategoryScore,
    OutlierSymbol,
    configured_embedding_runtime_preference,
    configured_embedding_threads,
    configured_embedding_runtime_info,
    configured_embedding_model_name,
    embedding_model_assets_available,
};
```

상위 레벨에서 `use codelens_engine::*` 또는 `crate::embedding::*`로 쓰는 모든 기존 경로를 `grep`하고 변경 후에도 동일해야 함.

## 분해 매핑

| 신규 파일                | LOC 예측 | 포함 내용 (라인 범위 at v1.9.32)                                                                                                                                                                                                                                                                                     |
| ------------------------ | -------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ffi.rs`                 | ~40      | ffi 모듈 (19–56) — sqlite-vec 등록, macOS sysctl                                                                                                                                                                                                                                                                     |
| `runtime.rs`             | ~480     | ORT*ENV_INIT, resolve_model_dir, parse*_*env, apple_perf_cores, configured_embedding*_, configure_embedding_runtime, recommended_embed_threads, embed_batch_size, max_embed_symbols, build_coreml_execution_provider, cpu_runtime_info, coreml_runtime_info, load_fastembed_builtin, load_codesearch_model (142–808) |
| `cache.rs`               | ~80      | TextEmbeddingCache struct + impl (178–229), reusable_embedding_key\* (89–140)                                                                                                                                                                                                                                        |
| `prompt.rs`              | ~1200    | split*identifier, is_test_only_symbol, build_embedding_text, hint*_*budget, join_hint_lines, extract_body_hint, nl_tokens_enabled, auto_hint*_, language*supports*_, is*nl_shaped, looks_like*_, extract_nl_tokens*, extract_api_calls*, extract_comment_body, extract_leading_doc (1775–2997)                       |
| `chunk_ops.rs`           | ~200     | StoredChunkKey, stored_chunk_key\*, duplicate_candidate_limit, duplicate_pair_key, cosine_similarity, DuplicatePair/CategoryScore/OutlierSymbol 구조체, embedding_to_bytes (1586–1738, 3005–3009)                                                                                                                    |
| `engine_impl.rs`         | ~850     | impl EmbeddingEngine 블록 2개 (832–1585, 1627–1710)                                                                                                                                                                                                                                                                  |
| `mod.rs` (축소)          | ~200     | public re-exports, SemanticMatch struct + From impl, EmbeddingEngine struct, EmbeddingIndexInfo, EmbeddingRuntimeInfo 구조체 정의, `mod` 선언들                                                                                                                                                                      |
| `tests.rs` (또는 tests/) | ~1700    | 기존 `mod tests { ... }` (3010–end)                                                                                                                                                                                                                                                                                  |

목표: 최대 파일 LOC ≤ 1,300 (prompt.rs는 허용, 나머지는 ≤ 900).

## 가시성 규칙

- 내부 helper: 기본 `pub(super)` 또는 `pub(crate)` 사용
- 테스트에서 접근 필요한 private fn: `pub(super)`로 공개 (기존 `pub(super)` 유지)
- 모듈 간 공유되는 타입/fn: `pub(super)` (embedding 내부용) 또는 `pub(crate)` (engine 전체용)
- 외부 공개된 것만 `pub` (위 보존 목록)

## 실행 단계 (builder 체크리스트)

1. [ ] `verify_change_readiness` with target files → `mutation_ready` 확인
2. [ ] 분해 전 `cargo check -p codelens-engine --features semantic` 통과 확인 (baseline)
3. [ ] `cargo test -p codelens-engine --lib --features semantic` 통과 확인 (baseline)
4. [ ] 신규 파일 6개 생성 (내용 복사)
5. [ ] `mod.rs`에 `mod ffi; mod runtime; mod cache; mod prompt; mod chunk_ops; mod engine_impl; #[cfg(test)] mod tests;` 선언
6. [ ] `mod.rs`에서 이동된 아이템 삭제
7. [ ] 외부 `use` 경로 보존: `pub use` 재노출
8. [ ] `cargo check -p codelens-engine --features semantic` (0 error, 0 new warning)
9. [ ] `cargo check -p codelens-engine --no-default-features` (feature-off 경로)
10. [ ] `cargo test -p codelens-engine --lib --features semantic` (전체 테스트 통과)
11. [ ] `cargo test -p codelens-mcp` (소비자측 영향 0)
12. [ ] `cargo clippy --workspace --all-features -- -W clippy::all` (새 경고 0)
13. [ ] `wc -l crates/codelens-engine/src/embedding/*.rs`로 LOC 확인 — 모든 파일 ≤ 1,300

## 수락 기준

- [ ] 모든 검증 명령 PASS
- [ ] `mod.rs` LOC ≤ 300
- [ ] 최대 파일 LOC ≤ 1,300 (prompt.rs 예외)
- [ ] 외부 API 시그니처 파괴 0 (소비 crate 수정 없이 통과)
- [ ] `embedding_tests.rs`가 `build_embedding_text` 등 `pub(super)` 헬퍼에 접근 가능
- [ ] 새 의존성, 새 feature flag, 새 env var 추가 없음

## 하지 말 것

- 로직 개선 / 리네임 / 시그니처 정리 — 모두 **분해만** 수행
- `vec_store.rs` 이동 — 이미 별도 파일
- `EmbeddingEngine` struct 분해 — impl 블록 이동까지만
- 테스트 헬퍼 공용화 — 기존 테스트 코드를 통째로 tests.rs로 이동만

## 롤백 기준

- 어떤 검증 단계라도 FAIL 시 전체 롤백
- 새 warning 1개라도 발생 시 원인 분석 후 수정 (덮어쓰지 말 것)

## 후속 작업 (본 계획 범위 외)

- `prompt.rs` 1,200줄 추가 분해 (nl_tokens/api_calls/comment/hint별) — 별 session
- `engine_impl.rs` EmbeddingEngine 책임 세분화 — 별 session
- tests.rs 테스트 조직 (fixture 공용화, 하위 디렉토리) — 별 session
