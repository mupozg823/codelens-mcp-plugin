#!/bin/zsh
# SessionStart: CodeLens 데몬 상태를 1줄 컨텍스트로 주입.
# 목적: "데몬이 살아있고 어디에 바인딩됐는지" 불확실성 제거 → 학습된 회피 해소.
#
# 노이즈 정책 (2026-07-11 재보정):
#  - 출력 ≤350바이트. verb 라우팅 상세는 always-on rules/harness.md 가 담당하므로
#    여기서는 liveness + 바인딩 힌트만 주입한다 (중복 서술 금지).
#  - stdin JSON 의 source=resume 이면 침묵 — resume 은 기존 컨텍스트(이전 주입 포함)를
#    그대로 잇는 이벤트라 재주입이 중복이 된다. settings.json 등록 시에도
#    matcher "startup|clear|compact" 를 권장 (이중 방어; superpowers 플러그인과 동일 정책).
#  - CodeLens 미사용 프로젝트(.codelens 인덱스도 .mcp.json 헤더도 없음)에서는 침묵 — 0 토큰.
#  - 예외: $HOME 직접 세션은 $HOME/.codelens(전역 데이터 디렉토리)에 매칭되어 발화한다.
#    codelens-first.py 는 이를 프로젝트 인덱스에서 제외하지만, 홈 세션=하네스 작업장으로
#    CodeLens 를 실사용하므로 프로브는 의도적으로 발화를 유지한다 (문서화된 예외).
#    단, 안내 메시지는 $HOME 자체 바인딩을 지시하지 않는다 — prepare_harness_session(project=$HOME)
#    은 홈 트리 전체 인덱싱을 시도해 타임아웃한다(2026-07-12 실측). 홈 세션은 조회 대상
#    레포 경로로 바인딩하도록 안내한다.
#
# 참고: 이 스크립트는 host 측 훅이다 (사용자 settings.json 의 SessionStart 에
# 등록). 플러그인 hooks.json 에는 포함되지 않는다.

CARD_URL="${CODELENS_CARD_URL:-http://127.0.0.1:7838/.well-known/mcp.json}"

# resume 이벤트는 침묵 (stdin 이 비어 있으면 startup 으로 간주하고 진행)
HOOK_INPUT=$(cat 2>/dev/null || true)
case "$HOOK_INPUT" in
  *'"source"'*'"resume"'*) exit 0 ;;
esac

GIT_ROOT=$(git -C "$PWD" rev-parse --show-toplevel 2>/dev/null || echo "$PWD")

HAS_INDEX=0
[ -d "$GIT_ROOT/.codelens" ] && HAS_INDEX=1
HAS_HEADER=0
grep -q 'x-codelens-project' "$GIT_ROOT/.mcp.json" 2>/dev/null && HAS_HEADER=1

# CodeLens 미사용 프로젝트 → 침묵 (토큰 0)
if [ "$HAS_INDEX" = "0" ] && [ "$HAS_HEADER" = "0" ]; then
  exit 0
fi

# 단발 0.7s 프로브는 데몬 생존 중에도 일시 부하로 실패해 오탐 "다운"을 주입,
# 심볼 게이트가 세션 내내 꺼진다(2026-07-15 실측: 2.5일 무중단 데몬을 다운 보고).
# 재시도 1회 포함 총 예산 ~1.9s — 훅 timeout 3s 이내 유지 필수.
ALIVE=0
for t in 0.7 1.2; do
  if curl -sf -m "$t" "$CARD_URL" -o /dev/null 2>/dev/null; then
    ALIVE=1
    break
  fi
done
if [ "$ALIVE" = "0" ]; then
  echo "🔍 CodeLens 데몬 다운(:7838) — 쉘 폴백 허용, 심볼 게이트 자동 비활성."
  exit 0
fi

if [ "$HAS_HEADER" = "1" ]; then
  echo "🔍 CodeLens alive(:7838) — .mcp.json 헤더 자동 바인딩(prepare_harness_session 생략 가능). 심볼 라우팅 상세=rules/harness.md CodeLens-First."
elif [ "$GIT_ROOT" = "$HOME" ]; then
  echo "🔍 CodeLens alive(:7838) — 홈 세션: \$HOME 자체 바인딩 금지(홈 전체 인덱싱→타임아웃). 코드 조회 전 대상 레포로 prepare_harness_session(project=<레포 절대경로>) 바인딩. 상세=rules/harness.md."
else
  echo "🔍 CodeLens alive(:7838) — 첫 호출 전 prepare_harness_session(project=\"$GIT_ROOT\") 필수(공유 데몬 오바인딩 방지). 미노출 시 ToolSearch \"select:mcp__codelens__search,mcp__codelens__graph\". 상세=rules/harness.md."
fi
exit 0
