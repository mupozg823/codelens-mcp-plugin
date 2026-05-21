#!/usr/bin/env bash
set -euo pipefail

# Rotate stale CodeLens backups whose presence on disk no longer reflects
# operational value. Encodes the friction discovered during the 2026-05-21
# self-dogfood session: ~2.4 GB of orphaned `.bak-*` files had accumulated
# across two locations without any rotation policy, because every backup
# is created at a discrete decision point (daemon upgrade, db migration,
# readonly conversion) but never retired.
#
# Three backup patterns are managed:
#
#   1. ${REPO_ROOT}/.codelens/bin/codelens-mcp-http.bak-pre-v{version}
#      Created by manual `cp` during daemon redeploy. Accumulates one per
#      release tag that the operator manually preserved.
#
#   2. ${HOME}/.codelens/index/{symbols,embeddings}.db.bak-{date}-pre-*-migration
#      Created before in-place db schema migrations (e.g. the 16k vector
#      width change on 2026-05-18). The previous schema is preserved so
#      a rollback can copy the old file back in place.
#
#   3. ${HOME}/.codelens/index/{symbols,embeddings}.db.bak-readonly-old
#      Created when an index is converted from read-write to read-only;
#      preserves the writable copy in case the conversion was wrong.
#
# Default policy: keep the 2 most recent matches per pattern (by mtime),
# delete the rest. The most recent backup is the only one realistically
# useful for rollback; the second-most-recent is the safety net.
#
# Usage:
#   bash scripts/cleanup-stale-backups.sh [--dry-run] [--keep N] [--repo-root PATH]
#
# Exit codes:
#   0 — completed (with or without deletions)
#   1 — argument or filesystem error

KEEP=2
DRY_RUN=0
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

while [[ $# -gt 0 ]]; do
	case "$1" in
		--dry-run)
			DRY_RUN=1
			shift
			;;
		--keep)
			KEEP="$2"
			shift 2
			;;
		--repo-root)
			REPO_ROOT="$2"
			shift 2
			;;
		--help|-h)
			grep -E '^# ' "$0" | sed 's/^# \{0,1\}//'
			exit 0
			;;
		*)
			echo "unknown option: $1" >&2
			exit 1
			;;
	esac
done

if [[ ! "$KEEP" =~ ^[0-9]+$ ]] || (( KEEP < 1 )); then
	echo "--keep must be a positive integer (got: $KEEP)" >&2
	exit 1
fi

total_freed=0

# Rotate one glob pattern. Keeps the $KEEP most recent files by mtime,
# deletes the rest. Reports size freed in bytes.
rotate_pattern() {
	local pattern="$1"
	local label="$2"
	# shellcheck disable=SC2206
	local matches=( $(ls -t $pattern 2>/dev/null) )
	local count=${#matches[@]}
	if (( count <= KEEP )); then
		printf "  %s: %d file(s) — within keep window (%d), no action\n" \
			"$label" "$count" "$KEEP"
		return
	fi
	local to_delete=$(( count - KEEP ))
	local freed=0
	printf "  %s: %d file(s), keeping %d, deleting %d:\n" \
		"$label" "$count" "$KEEP" "$to_delete"
	for (( i = KEEP; i < count; i++ )); do
		local file="${matches[i]}"
		local size
		size=$(stat -f%z "$file" 2>/dev/null || stat -c%s "$file" 2>/dev/null || echo 0)
		freed=$((freed + size))
		if (( DRY_RUN == 1 )); then
			printf "    [dry-run] would remove %s (%d bytes)\n" "$file" "$size"
		else
			rm -f "$file"
			printf "    removed %s (%d bytes)\n" "$file" "$size"
		fi
	done
	total_freed=$((total_freed + freed))
}

printf "CodeLens backup rotation (keep=%d, dry_run=%d)\n" "$KEEP" "$DRY_RUN"
printf "repo: %s\n" "$REPO_ROOT"
printf "\n"

printf "[1] daemon binary backups (.codelens/bin/*.bak-pre-*)\n"
rotate_pattern "${REPO_ROOT}/.codelens/bin/codelens-mcp-http.bak-pre-*" "  bin"

printf "\n[2] global index migration backups (~/.codelens/index/*.bak-*-migration)\n"
rotate_pattern "${HOME}/.codelens/index/symbols.db.bak-*-migration" "  symbols"
rotate_pattern "${HOME}/.codelens/index/embeddings.db.bak-*-migration" "  embeddings"

printf "\n[3] global index readonly conversion backups (~/.codelens/index/*.bak-readonly-old)\n"
rotate_pattern "${HOME}/.codelens/index/symbols.db.bak-readonly-old" "  symbols"
rotate_pattern "${HOME}/.codelens/index/embeddings.db.bak-readonly-old" "  embeddings"

printf "\ntotal freed: %d bytes (%.1f MB)\n" "$total_freed" "$(echo "scale=1; $total_freed / 1048576" | bc 2>/dev/null || echo "0.0")"
