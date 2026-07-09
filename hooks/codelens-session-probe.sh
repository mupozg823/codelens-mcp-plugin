#!/bin/zsh
# SessionStart: CodeLens 데몬 상태를 1줄 컨텍스트로 주입.
# 목적: "데몬이 살아있고 어디에 바인딩됐는지" 불확실성 제거 → 학습된 회피 해소.
#
# 토큰 절감: CodeLens 를 쓰지 않는 프로젝트(git root 에 .codelens 인덱스도,
# .mcp.json 헤더 바인딩도 없음)에서는 아무것도 출력하지 않는다 — 0 토큰.
# 출력은 350바이트 이내 유지 (캐시 위생).
#
# 참고: 이 스크립트는 host 측 훅이다 (사용자 settings.json 의 SessionStart 에
# 등록). 플러그인 hooks.json 에는 포함되지 않는다.

CARD_URL="${CODELENS_CARD_URL:-http://127.0.0.1:7839/.well-known/mcp.json}"

GIT_ROOT=$(git -C "$PWD" rev-parse --show-toplevel 2>/dev/null || echo "$PWD")

HAS_INDEX=0
[ -d "$GIT_ROOT/.codelens" ] && HAS_INDEX=1
HAS_HEADER=0
grep -q 'x-codelens-project' "$GIT_ROOT/.mcp.json" 2>/dev/null && HAS_HEADER=1

# CodeLens 미사용 프로젝트 → 침묵 (토큰 0)
if [ "$HAS_INDEX" = "0" ] && [ "$HAS_HEADER" = "0" ]; then
  exit 0
fi

if ! curl -sf -m 0.7 "$CARD_URL" -o /dev/null 2>/dev/null; then
  echo "🔍 CodeLens 데몬 다운(:7839) — 쉘 폴백 허용, 심볼 게이트 자동 비활성."
  exit 0
fi

if [ "$HAS_HEADER" = "1" ]; then
  BIND_LINE="이 프로젝트는 .mcp.json 헤더로 자동 바인딩됨 — prepare_harness_session 생략 가능."
else
  BIND_LINE="첫 호출 전 prepare_harness_session(project=\"$GIT_ROOT\") 필수 (공유 데몬 오바인딩 방지)."
fi
cat <<EOF
🔍 CodeLens 데몬 alive(:7839). 심볼 조회(정의·참조·구조·영향)는 rg 대신 CodeLens:
ToolSearch "select:mcp__codelens__find_symbol,mcp__codelens__find_referencing_symbols" 로드 → $BIND_LINE
rg/grep 은 텍스트 감사 전용 (심볼형 grep 은 strict 게이트가 세션당 3회 차단 — 예외 마커: [cl-text]/[cl-fallback]).
EOF
exit 0
