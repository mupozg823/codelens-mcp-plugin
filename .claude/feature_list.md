# Feature List — CodeLens 임베딩 모델 V9 학습 데이터 정제

Generated: 2026-04-08 | Pattern: long-running

## Subtasks

- [x] T1: 벤치마크 확장 (89 → 436 심볼, 다국어) — score: 1.0
- [ ] T2: 고품질 학습 데이터 1,000쌍 큐레이션 (273/1000 완료)
- [ ] T3: 데이터 품질 검증 (dedup, 포맷, 언어 배분)
- [ ] T4: LoRA 파인튜닝 (V6 기반, MNRL, early stopping)
- [ ] T5: 벤치마크 + 실사용 테스트
- [ ] T6: 모델 배포 (ONNX → codesearch/model.onnx)

## Current Focus

T2: 고품질 학습 데이터 — 나머지 727쌍 생성 필요

## Decisions Log

- T1: 벤치마크 89→436, 다국어 7개 언어, 중복 0, query_type 비율 맞춤
- T2 진행 중: 273/1000 생성. TS 95, Py 55, Go 40, Rust 25, Java 18, Ruby 19, PHP 20
- V7 (rule-based 증강) 실패 확인 → 데이터 양 아닌 질이 핵심
- V8-A (LLM synthetic LoRA on V6) MRR 0.807→0.891 성공 but 실사용 gap 존재
- INT8 양자화 시 경계 케이스 품질 손실 확인 (cosine sim 0.967)
- CSN에 Rust/TypeScript 없음 → 다국어 데이터 필수
- FTS5 tokenizer underscore separator 추가 (v6 마이그레이션) — "parse" → parse_symbols 매칭 가능
- hybrid 결과를 semantic_search에 병합 — score \* 0.35로 스케일링
- 남은 문제: hybrid FTS가 NL 쿼리 전체로 매칭 → 키워드 추출 후 FTS 필요
- semantic threshold 0.5 → 0.15, boost threshold 0.3 → 0.1
