#!/usr/bin/env bash
# CodeLens MCP — Benchmark Comparison
# Usage: ./benchmarks/compare.sh results/old.md results/new.md
set -euo pipefail

OLD="${1:?Usage: compare.sh <old.md> <new.md>}"
NEW="${2:?Usage: compare.sh <old.md> <new.md>}"

echo "============================================="
echo "  Benchmark Comparison"
echo "============================================="
echo "  OLD: $OLD"
echo "  NEW: $NEW"
echo ""

# Extract tool performance tables
extract_tools() {
	awk '/## 핵심 도구 성능/,/^$/' "$1" | grep "^|" | grep -v "도구\|---"
}

echo "--- 핵심 도구 성능 비교 ---"
printf "%-30s %8s %8s %8s\n" "도구" "OLD(ms)" "NEW(ms)" "변화"
printf "%-30s %8s %8s %8s\n" "------------------------------" "--------" "--------" "--------"

paste <(extract_tools "$OLD") <(extract_tools "$NEW") | while IFS=$'\t' read -r old_line new_line; do
	old_name=$(echo "$old_line" | awk -F'|' '{print $2}' | xargs)
	old_ms=$(echo "$old_line" | awk -F'|' '{print $3}' | xargs)
	new_ms=$(echo "$new_line" | awk -F'|' '{print $3}' | xargs)

	if [ -n "$old_ms" ] && [ -n "$new_ms" ] && [ "$old_ms" -gt 0 ] 2>/dev/null; then
		if [ "$new_ms" -lt "$old_ms" ]; then
			pct=$(((old_ms - new_ms) * 100 / old_ms))
			change="-${pct}% faster"
		elif [ "$new_ms" -gt "$old_ms" ]; then
			pct=$(((new_ms - old_ms) * 100 / old_ms))
			change="+${pct}% slower"
		else
			change="same"
		fi
		printf "%-30s %8s %8s %8s\n" "$old_name" "$old_ms" "$new_ms" "$change"
	fi
done

echo ""
echo "--- 커밋 ---"
echo "  OLD: $(grep "^commit:" "$OLD" | awk '{print $2}')"
echo "  NEW: $(grep "^commit:" "$NEW" | awk '{print $2}')"
