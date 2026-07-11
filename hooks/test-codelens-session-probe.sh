#!/bin/zsh
# codelens-session-probe.sh 회귀 테스트 — `zsh hooks/test-codelens-session-probe.sh`.
#
# 검증 불변식 (2026-07-11 노이즈 재보정):
#  1. source=resume 는 침묵한다 (resume 은 기존 컨텍스트를 잇는 이벤트 — 재주입 중복).
#  2. stdin 이 비어도 발화한다 (fail-open; 수동 실행·구버전 호스트 호환).
#  3. alive 출력은 350바이트 이내다 (verb 라우팅 상세는 호스트 always-on 규칙이 담당).
#  4. .mcp.json 헤더 프로젝트는 "자동 바인딩" 안내, 그 외엔 prepare_harness_session 안내.
#  5. CodeLens 미사용 프로젝트는 어떤 source 에서도 침묵한다.
#  6. 데몬 다운이면 쉘 폴백 허용 1줄만 낸다.
set -u

PROBE="${0:A:h}/codelens-session-probe.sh"
TMP=$(mktemp -d)
FAILS=0

# 데몬 alive 시뮬레이션: curl -sf 는 file:// 을 지원하므로 로컬 파일 = 항상 성공
CARD="$TMP/card.json"
echo '{}' > "$CARD"
ALIVE="file://$CARD"
DOWN="http://127.0.0.1:1/none"

check() { # desc, expect(정규식|EMPTY), actual
  local desc="$1" expect="$2" actual="$3" ok
  if [ "$expect" = "EMPTY" ]; then
    [ -z "$actual" ] && ok=OK || ok=FAIL
  else
    print -r -- "$actual" | grep -qE "$expect" && ok=OK || ok=FAIL
  fi
  [ "$ok" = FAIL ] && FAILS=$((FAILS + 1))
  printf '  [%-4s] %s\n' "$ok" "$desc"
  [ "$ok" = FAIL ] && printf '         기대=%s 실제=%s\n' "$expect" "$actual"
  return 0
}

# 프로젝트 픽스처: .codelens 인덱스 보유 (비 git — GIT_ROOT=PWD 폴백 경로)
IDX="$TMP/proj-index"; mkdir -p "$IDX/.codelens"
# 프로젝트 픽스처: .mcp.json 헤더 바인딩
HDR="$TMP/proj-header"; mkdir -p "$HDR"
printf '{"mcpServers":{"codelens":{"headers":{"x-codelens-project":"%s"}}}}' "$HDR" > "$HDR/.mcp.json"
# 프로젝트 픽스처: CodeLens 미사용
PLAIN="$TMP/proj-plain"; mkdir -p "$PLAIN"

# ── 1. resume 침묵 ──
OUT=$(cd "$IDX" && echo '{"session_id":"t","source":"resume"}' | CODELENS_CARD_URL="$ALIVE" zsh "$PROBE")
check "resume: 침묵" "EMPTY" "$OUT"

# ── 2. startup 발화 + 바이트 상한 ──
OUT=$(cd "$IDX" && echo '{"source":"startup"}' | CODELENS_CARD_URL="$ALIVE" zsh "$PROBE")
check "startup: alive 발화" "CodeLens alive" "$OUT"
check "startup: prepare_harness_session 안내" "prepare_harness_session\(project=\"$IDX\"\)" "$OUT"
BYTES=$(print -r -- "$OUT" | wc -c | tr -d ' ')
if [ "$BYTES" -le 350 ]; then
  check "alive 출력 ≤350B (실측 ${BYTES}B)" "." "ok"
else
  check "alive 출력 ≤350B (실측 ${BYTES}B)" "EMPTY" "over"
fi

# ── 3. compact 발화 / 빈 stdin fail-open ──
OUT=$(cd "$IDX" && echo '{"source":"compact"}' | CODELENS_CARD_URL="$ALIVE" zsh "$PROBE")
check "compact: 발화" "CodeLens alive" "$OUT"
OUT=$(cd "$IDX" && CODELENS_CARD_URL="$ALIVE" zsh "$PROBE" </dev/null)
check "빈 stdin: fail-open 발화" "CodeLens alive" "$OUT"

# ── 4. 헤더 바인딩 프로젝트 ──
OUT=$(cd "$HDR" && echo '{"source":"startup"}' | CODELENS_CARD_URL="$ALIVE" zsh "$PROBE")
check "헤더 프로젝트: 자동 바인딩 안내" "자동 바인딩.*생략 가능" "$OUT"

# ── 5. 미사용 프로젝트 침묵 ──
OUT=$(cd "$PLAIN" && echo '{"source":"startup"}' | CODELENS_CARD_URL="$ALIVE" zsh "$PROBE")
check "미사용 프로젝트: 침묵" "EMPTY" "$OUT"

# ── 6. 데몬 다운 ──
OUT=$(cd "$IDX" && echo '{"source":"startup"}' | CODELENS_CARD_URL="$DOWN" zsh "$PROBE")
check "데몬 다운: 폴백 안내 1줄" "데몬 다운.*쉘 폴백 허용" "$OUT"

rm -rf "$TMP"
echo
if [ "$FAILS" -gt 0 ]; then
  echo "❌ ${FAILS}건 실패"
  exit 1
fi
echo "✅ session-probe 9케이스 전체 통과"
