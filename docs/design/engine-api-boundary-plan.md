# codelens-engine API 경계 명확화 — 개선 방향

## 문제
codelens-engine의 mutation 함수들(`rename_symbol`, `replace_content`, `delete_lines` 등)이 `pub`으로 노출되어 있어, 외부 crate이 직접 호출하면 ADR-0009 mutation gate를 우회할 수 있음.

## lib.rs 문서 현황
이미 강한 경고문이 있음: "MUST route mutations through codelens-mcp... Calling mutation primitives directly... will silently bypass the project's principals.toml configuration."

## 컴파일러 레벨 강제 방안 (순위별)

### 1. `#[doc(hidden)]` + `#[deprecated]` (가벼운 변경) ✅
- mutation fn에 `#[deprecated(note = "Route through codelens-mcp dispatch pipeline. Direct calls bypass ADR-0009 mutation gate and audit sink.")]` 추가
- 외부 사용 시 compiler warning 발생
- 워크스페이스 내 사용에 영향 없음

### 2. Marker type 패턴 (중간 변경)
- mutation fn에 `MutationGateToken` 파라미터 추가
- 토큰은 `pub(crate)`로 엔진 내에서만 생성
- codelens-mcp는 엔진과 같은 workspace이므로 friend 패턴 적용 필요

### 3. Crate 분리 (무거운 변경)
- `codelens-engine-read` (pub read API) + `codelens-engine-mutation` (internal)
- codelens-mcp만 mutation crate에 접근

## 권장: 방안 1 적용
`#[deprecated]`는 compiler warning만 생성하므로 workspace 내 사용에도 경고가 뜨지만, `#[allow(deprecated)]`로 codelens-mcp 측에서 명시적 허용 가능. 이는 "의도적 우회"를 문서화하는 효과가 있음.

## 미적용 이유
방안 1도 workspace 전체에 compiler warning을 생성하므로, 실제 적용 전에 codelens-mcp 측에 `#[allow(deprecated)]`를 먼저 배치하는 2단계 커밋이 필요. Phase 2로 이관.
