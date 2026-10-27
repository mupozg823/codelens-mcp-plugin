# Analysis-lite task selection and independent gold

This companion document and `benchmarks/analysis-lite/tasks-v1.json` form the
frozen task bundle. Both hashes must be accepted before any pilot model run.
No candidate result or model invocation was used to select or grade these tasks.
The model-run count at freeze is zero. The six task identities and prompts must
not be replaced after observing candidate results.

## Inputs and source ownership

Both actual user repositories exist and are Git repositories. Selection inspected
their tracked source without changing either original. Winnation HEAD was
`8f8c3aaa83a795caaabb419eae6b4cfb158e3d8b`; tuanbo-broadcast-suite HEAD was
`c23ce26dc9004c9eced0c0435fa8b32d25caf39a`. Neither had tracked modifications;
their untracked WIP remains untouched. The manifest contains canonical local
paths, exact commit/tree IDs, and SHA-256 hashes for 25 relevant source/config
objects. Source-only snapshots must come from those Git trees, without original
untracked files, auth/environment files, model history, gold or later fix history.

E2 deliberately uses Winnation commit
`9f4b439d88601e4e925bb2bcfa7ff158cf732c47`, the parent of the actual decimal
formatting repair `e3cb59af3385917dede2e74a3b0a08535b79f88a` (#395, 2026-10-17).
The old defect exists in that snapshot; it is not injected into current source.
That base lacks `crew-value-format.test.ts`, so the task asks the agent to add it.

## Six tasks and evidence classes

| ID | Class | Actual task and independent basis | Field evidence classification |
|---|---|---|---|
| E1-lifecycle-callers | everyday/read | Exact production callers of channel-scoped cancellation in Tuanbo. Reuses the named real repeated-search task in efficacy research §2c/2d; current source independently confirms two calls. | historical-field-task, gold-refreshed |
| E2-crew-decimal-repair | everyday/edit | Replay the real #395 formatting error, add regression tests and preserve flooring/amount rules. Exact source and real fix independently reproduce 0.28→0.29. | actual-historical-defect-replay |
| E3-pause-decoder | everyday/read | Current broadcast pause decoder and its three claim-response call sites; inspect strict boolean/default behavior. Source inspection and TypeScript binding agree. | real-source-derived, field-failure-unconfirmed |
| H1-trigger-identity | hard/read | ResolumeClient.triggerClip, distinguishing same-name declarations and combining five local plus four external calls. Reuses the real triggerClip task while replacing its suspect historical gold. | historical-field-task, gold-refreshed |
| H2-path-alias-reexport | hard/read | The real overlay route's @ path alias and ranking barrel re-export resolve to parseRankingExclusionVariant. Separate import/re-export from the actual call. | real-source-derived, field-failure-unconfirmed |
| H3-bounded-format-impact | hard/read | Bounded reverse dependencies of the private formatter helper used by the two real on-air renderers; retain callback provenance and duplicate call sites. | real-source-derived, field-failure-unconfirmed |

H2/H3 were selected because these current production structures exercise the
approved hard-task dimensions. No observed agent failure or user issue for those
specific analysis questions has been established. H3's formatter belongs to the
actual #395 repair lineage, but the bounded impact question itself is a new
source-derived analysis task. These distinctions also apply to pilot reporting:
success on a source-derived task does not establish resolution of a field failure.

## Declaration identities and source offsets

`span_semantics = declaration_name_token` applies to every `gold.declaration`
`byte_start`/`byte_end` in the manifest. Ranges are half-open UTF-8 byte offsets
for the identifier token, not the full function/method declaration node. Lines
are one-based. OXC declaration-node spans must be mapped to the name token before
comparing identity. E2 identifies its historical formatter by file/name without
claiming a byte span.

H1's independent TypeScript source position is UTF-16 offset 56741, while its
UTF-8 byte start is 65230. These are the same `triggerClip` identifier in the same
hashed file. Freeze validation asserts the UTF-16→UTF-8 conversion and decoded
byte slice against the identifier; a numeric offset match across encodings would
be incorrect. All other frozen name-token byte slices are similarly checked.

## Independent verification and limits

The task-owned local evidence directory is
`.codex/analysis-lite-evidence/dispatch2/`. `ts-gold-probe.json` records TypeScript
5.9.3 checker symbol identity for all twelve same-name production calls: nine
resolve to the selected H1 method and three resolve elsewhere. Manual receiver,
import and declaration reads confirm the exclusions. `winnation-gold-probe.json`
records import-alias resolution and eight relevant call sites, plus the pure
historical formatter replay using transpileModule and isolated VM contexts.
These probes are source/gold checks, not candidate-tool runs or model evaluations.

Winnation has no local node_modules. The targeted symbol bindings resolve, but
missing unrelated dependency typings mean the checker probe is not a successful
project typecheck. E2's actual npm/Vitest acceptance run and dependency setup are
pending a later dispatch; setup time and failures must remain in the pilot record.
No evaluator implementation, source checkout, Rust build or HTTP server was
created for this selection stage.

H1 gold covers statically bound direct call expressions. Interface-based runtime
dispatch is not silently assigned to the selected method. H2 explicitly grades
non-call locations separately. H3 is limited to lib/overlay, reverse depth two,
four nodes including target and four edges, with named lexical containment for
the map callback. It is not a claim of whole-project runtime or type/import impact.

## Freeze and grading handoff

Each task hash covers its prompt, limits and independent gold. The content hash
covers the complete manifest including task hashes, excluding only the content
hash field itself; canonicalization is specified inside the manifest. Final file
hashes and a combined bundle hash are recorded in local `freeze.json` and supplied
to the parent. The companion document is part of the frozen grading metadata.

Only the explicitly listed agent payload fields may enter an evaluated prompt.
Neither this document nor the manifest/gold may enter evaluated checkouts or host
conversation context. Read tasks need independent exact location/identity grades;
the edit task needs observed tests against actual modified source. A later runner
must retain every failure, timeout, unsupported result and missing usage row and
must not infer task quality from telemetry or tool usage.

This 48-run pilot has six selected tasks and two repetitions per host/arm. It
mixes historical field tasks with source-derived tasks, and one historical repair
replay. It cannot establish broad failure frequency or production-wide gains.
Adoption remains per host/language/task scope under the approved independent
quality/time/token gates; unknown evidence yields an inconclusive decision.
