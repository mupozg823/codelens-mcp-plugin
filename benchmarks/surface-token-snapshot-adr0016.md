# Surface token-cost snapshot — ADR-0016 verification

ADR-0016's verification section calls for a per-host-profile token-cost
snapshot recorded before/after the workflow-first surface landing. Method:
live `tools/list` against the repo-local daemon, one session per host class
(client-profile detection), default (non-expanded) listing plus `full: true`
expansion. Token figures are response-size estimates (`bytes / 4`); the
server-side `estimated_tokens` sum tracks the same quantity.

## After (CORE-20 surface; daemon = git 583fe34, semantic feature on)

| Host class | Default listing | Response bytes (≈ tokens) | `full: true` |
| --- | --- | --- | --- |
| claude-code (deferred on) | **14 tools** (ranked bootstrap slice, output schemas deferred) | 16,706 B (≈ 4.2K tok) | 39 tools |
| codex (deferred on) | **14 tools** (same slice) | 16,705 B (≈ 4.2K tok) | 39 tools |
| generic (deferred off) | **20 tools** (static CORE-20, schemas inline) | 61,980 B (≈ 15.5K tok) | 39 tools |

Reading: every host class satisfies the ADR "default ≤ 20" bound. Deferred
hosts pay ~4.2K tokens for a 14-tool bootstrap and reach the rest through
host-native or namespace expansion; schema-inline generic clients pay ~15.5K
for the full static surface once, with no expansion round-trip. The expanded
surface is 39 because the deprecation filter now drops the remove-wave
entries (coordination quartet + disposition remove-13) from listings.

## Before (references, previously recorded)

- Pre-E2 default listing was **9 verbs** for every host
  (`default_listed_tool_names()` pre-`554044c2`); precision tools required an
  expansion round-trip before first use.
- The 2026-07-19 client-profile token bench (see
  `docs/design/workflow-first-tool-surface-migration.md` context and the
  07-19 surface-diet measurements) recorded the lean Claude Code bootstrap at
  ~5 tools / ≈1.76K tokens with output schemas stripped, and identified
  generic/cursor clients as the oversized surfaces (~40-tool Balanced
  listings with schemas).

Delta summary: default discoverability grew 9 → 20 declared (14 served under
deferral) in exchange for eliminating the first-use expansion round-trip on
the ten precision/analysis entrypoints; the generic surface shrank from the
~40-tool Balanced listing to the static 20.

Regenerate: run the three-host `tools/list` probe against `:7838` (see
`docs/operations/http-daemon.md`) after any surface change and update this
table alongside the ADR-0016 lock tests.
