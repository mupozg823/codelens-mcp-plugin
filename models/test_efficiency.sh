#!/bin/bash
# CodeLens 효율성 측정: 같은 질문에 대해 도구별 토큰 비용 비교
set -euo pipefail
cd /Users/bagjaeseog/codelens-mcp-plugin
BIN="./target/release/codelens-mcp"

echo "=== TEST: '인증 로직 관련 코드 찾기' ==="
echo ""

# 1. CodeLens get_ranked_context
echo "--- Method 1: get_ranked_context ---"
T1=$(date +%s%N)
R1=$($BIN . --cmd get_ranked_context --args '{"query":"authentication login verification","max_results":5}' 2>/dev/null)
T2=$(date +%s%N)
TOKENS1=$(echo "$R1" | wc -c)
MS1=$(((T2 - T1) / 1000000))
echo "  Time: ${MS1}ms"
echo "  Output size: ${TOKENS1} bytes (~$((TOKENS1 / 4)) tokens)"
echo "  Files returned:"
echo "$R1" | python3 -c "import sys,json; d=json.load(sys.stdin); [print(f'    {e[\"file_path\"]}:{e[\"line\"]} ({e[\"kind\"]}) {e[\"name\"]}') for e in d.get('data',d.get('results',[]))[:5]]" 2>/dev/null || echo "  (parse error)"
echo ""

# 2. CodeLens find_symbol
echo "--- Method 2: find_symbol ---"
T1=$(date +%s%N)
R2=$($BIN . --cmd find_symbol --args '{"name":"auth","include_body":false,"max_matches":10}' 2>/dev/null)
T2=$(date +%s%N)
TOKENS2=$(echo "$R2" | wc -c)
MS2=$(((T2 - T1) / 1000000))
echo "  Time: ${MS2}ms"
echo "  Output size: ${TOKENS2} bytes (~$((TOKENS2 / 4)) tokens)"
echo ""

# 3. grep (what an agent without CodeLens would do)
echo "--- Method 3: grep (no CodeLens) ---"
T1=$(date +%s%N)
R3=$(grep -rn "auth\|login\|verify\|credential" crates/ --include="*.rs" 2>/dev/null | head -50)
T2=$(date +%s%N)
TOKENS3=$(echo "$R3" | wc -c)
MS3=$(((T2 - T1) / 1000000))
echo "  Time: ${MS3}ms"
echo "  Output size: ${TOKENS3} bytes (~$((TOKENS3 / 4)) tokens)"
echo "  Lines returned: $(echo "$R3" | wc -l)"
echo ""

echo "=== TEST: 'rename_symbol 함수의 영향 범위' ==="
echo ""

# 4. CodeLens get_impact_analysis
echo "--- Method 4: get_impact_analysis ---"
T1=$(date +%s%N)
R4=$($BIN . --cmd get_impact_analysis --args '{"file_path":"crates/codelens-core/src/rename.rs"}' 2>/dev/null)
T2=$(date +%s%N)
TOKENS4=$(echo "$R4" | wc -c)
MS4=$(((T2 - T1) / 1000000))
echo "  Time: ${MS4}ms"
echo "  Output size: ${TOKENS4} bytes (~$((TOKENS4 / 4)) tokens)"
echo ""

# 5. Manual: agent would read the file + grep for imports
echo "--- Method 5: Read file + grep imports (no CodeLens) ---"
T1=$(date +%s%N)
R5A=$(cat crates/codelens-core/src/rename.rs)
R5B=$(grep -rn "use.*rename\|rename::" crates/ --include="*.rs" 2>/dev/null)
T2=$(date +%s%N)
TOKENS5=$(($(echo "$R5A" | wc -c) + $(echo "$R5B" | wc -c)))
MS5=$(((T2 - T1) / 1000000))
echo "  Time: ${MS5}ms"
echo "  Output size: ${TOKENS5} bytes (~$((TOKENS5 / 4)) tokens)"
echo ""

echo "=== TEST: 'onboard_project - 프로젝트 이해' ==="
echo ""

# 6. CodeLens onboard_project
echo "--- Method 6: onboard_project ---"
T1=$(date +%s%N)
R6=$($BIN . --cmd onboard_project --args '{}' 2>/dev/null)
T2=$(date +%s%N)
TOKENS6=$(echo "$R6" | wc -c)
MS6=$(((T2 - T1) / 1000000))
echo "  Time: ${MS6}ms"
echo "  Output size: ${TOKENS6} bytes (~$((TOKENS6 / 4)) tokens)"
echo ""

# 7. Manual: ls + find + read multiple files
echo "--- Method 7: Manual exploration (no CodeLens) ---"
T1=$(date +%s%N)
R7A=$(find crates/ -name "*.rs" -type f | head -30)
R7B=$(cat crates/codelens-core/src/lib.rs)
R7C=$(cat crates/codelens-mcp/src/main.rs)
R7D=$(cat Cargo.toml)
R7E=$(head -50 README.md)
T2=$(date +%s%N)
TOKENS7=$(($(echo "$R7A$R7B$R7C$R7D$R7E" | wc -c)))
MS7=$(((T2 - T1) / 1000000))
echo "  Time: ${MS7}ms"
echo "  Output size: ${TOKENS7} bytes (~$((TOKENS7 / 4)) tokens)"
echo ""

echo "=== SUMMARY ==="
echo ""
printf "%-40s %8s %12s\n" "Task / Method" "Time(ms)" "Tokens(est)"
echo "---------------------------------------------------------------"
printf "%-40s %8d %12d\n" "Search: get_ranked_context" "$MS1" "$((TOKENS1 / 4))"
printf "%-40s %8d %12d\n" "Search: find_symbol" "$MS2" "$((TOKENS2 / 4))"
printf "%-40s %8d %12d\n" "Search: grep (no CodeLens)" "$MS3" "$((TOKENS3 / 4))"
echo ""
printf "%-40s %8d %12d\n" "Impact: get_impact_analysis" "$MS4" "$((TOKENS4 / 4))"
printf "%-40s %8d %12d\n" "Impact: read file + grep (no CodeLens)" "$MS5" "$((TOKENS5 / 4))"
echo ""
printf "%-40s %8d %12d\n" "Onboard: onboard_project" "$MS6" "$((TOKENS6 / 4))"
printf "%-40s %8d %12d\n" "Onboard: manual explore (no CodeLens)" "$MS7" "$((TOKENS7 / 4))"
