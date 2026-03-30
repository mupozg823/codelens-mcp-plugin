---
date: 2026-03-30
phase: Phase 8-9 (품질 최적화 + 치명 버그 수정)
project: codelens-mcp-plugin (self, 29K LOC, 73 Rust files)
binary: target/release/codelens-mcp (arm64-darwin, LTO+strip)
commit: 5327212
---

# Benchmark: 2026-03-30 — Phase 8-9

## 환경

| 항목      | 값                         |
| --------- | -------------------------- |
| OS        | macOS Darwin 25.3.0 arm64  |
| Rust      | 1.93.0                     |
| 프로젝트  | codelens-mcp-plugin (self) |
| LOC       | 29,783                     |
| 파일 수   | 73 (.rs)                   |
| 심볼 수   | 958                        |
| DB schema | v4 (FTS5)                  |

## 핵심 도구 성능 (warm, 인덱스 있음)

| 도구                     | 시간(ms) | 비고                           |
| ------------------------ | -------- | ------------------------------ |
| find_symbol              | 11       | FTS5 인덱스 조회               |
| get_symbols_overview     | 11       | DB 캐시                        |
| get_ranked_context       | 15       | 4-signal 랭킹                  |
| get_impact_analysis      | 12       | 그래프 캐시                    |
| find_referencing_symbols | 93       | tree-sitter 주석필터 포함      |
| rename_symbol (dry_run)  | 83       | 주석/문자열 필터링 포함        |
| refresh_symbol_index     | 82       | 73파일 전체 리인덱싱           |
| onboard_project          | 45,746   | 시맨틱 임베딩 포함 (fastembed) |

## 제로 프로젝트 성능 (첫 사용, auto-index)

| 시나리오     | 첫 호출(ms) | 이후(ms) |
| ------------ | ----------- | -------- |
| Python 3파일 | 45          | 11       |
| 500파일 대형 | 115         | 43       |

## grep 대비

| 작업                 | CodeLens | grep | 배율      | 비고                                  |
| -------------------- | -------- | ---- | --------- | ------------------------------------- |
| find_symbol          | 11ms     | 8ms  | 1.4x 느림 | CodeLens: FTS5+부가정보               |
| find_refs (AppState) | 93ms     | 10ms | 9x 느림   | CodeLens: 선언감지+enclosing+주석필터 |

## 이번 Phase에서 수정한 문제

| #   | 문제                    | 심각도 | 수정                      | 성능 영향                     |
| --- | ----------------------- | ------ | ------------------------- | ----------------------------- |
| 1   | 제로 환경 0결과         | 치명   | auto-index on startup     | 첫 호출 +30-80ms              |
| 2   | find_refs 155ms 병목    | 중     | DB 기반 파일 목록         | 155→93ms (주석필터 전 33ms)   |
| 3   | rename 주석/문자열 치환 | 치명   | tree-sitter non-code 필터 | rename +20ms, find_refs +60ms |
| 4   | 심볼릭 링크 0결과       | 중     | resolve canonicalize      | 영향 없음                     |

## 알려진 한계

| 항목                     | 현상               | 원인                         | 대안                          |
| ------------------------ | ------------------ | ---------------------------- | ----------------------------- |
| onboard_project 45초     | 시맨틱 임베딩 로딩 | fastembed 모델 248MB         | 시맨틱 없이 onboard → <1초    |
| find_refs grep 대비 느림 | 93ms vs 10ms       | 파일별 tree-sitter 파싱 비용 | 정확도와 trade-off, 캐시 가능 |
| 50K함수 파일 1.3초       | 첫 인덱싱          | tree-sitter 파싱 자체 비용   | 한 번만 발생, 이후 캐시       |
