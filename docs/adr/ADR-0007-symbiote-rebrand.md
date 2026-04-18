# ADR-0007: Rebrand CodeLens MCP → Symbiote MCP (v2.0.0)

- Status: Accepted (rebrand intent); execution spans v1.9.x → v2.0.0 transition
- Date: 2026-04-18
- Supersedes: none
- Related: ADR-0005 (harness-v2 substrate), ADR-0006 (routing enforcement)

## Context

The product has grown from a code-intelligence MCP server into a
harness substrate: it hosts multiple specialized agents (planner,
builder, reviewer, analyzer), enforces canonical truth, coordinates
file claims, emits routing hints, and publishes handoff contracts.
The brand "CodeLens" captures a fraction of this — the compressed-view
of code — but no longer captures what the product actually _is_ to its
users. Users don't attach CodeLens to look at code; they attach it to
make their existing agents competent.

The Marvel-inspired "symbiote" metaphor captures this precisely:

- A symbiote attaches to a host.
- The host retains its identity and control.
- Together they form a superhuman capability the host alone does not have.
- The relationship is mutualistic — the symbiote needs the host's
  cognition, the host needs the symbiote's augmentation.

This is exactly the CodeLens ↔ Claude/Codex/Cursor relationship today:

- CodeLens alone cannot plan, discuss, or reason.
- Claude/Codex/Cursor alone cannot efficiently search a 100k-file repo
  or enforce mutation-safety preflights.
- The _pair_ produces results neither can produce solo.

## Decision

Rebrand to **Symbiote MCP** at the v2.0.0 major bump. Tagline:

> Symbiote MCP — harness-engineering as a symbiotic substrate.
> Attach to your agent. Your code intelligence becomes superhuman.

Scope of the rebrand (breaking change — hence v2.0.0):

- crate names: `codelens-engine` → `symbiote-engine`,
  `codelens-mcp` → `symbiote-mcp`, `codelens-tui` → `symbiote-tui`.
- binary: `codelens-mcp` → `symbiote-mcp`.
- repo: `codelens-mcp-plugin` → `symbiote-mcp` (GitHub keeps old URL as
  redirect).
- resource URIs: `codelens://...` → `symbiote://...` with a one-minor-
  version compatibility window emitting both.
- env vars: `CODELENS_*` → `SYMBIOTE_*` with both accepted until v3.0.
- docs, install.sh, Homebrew formula, install channel table.

Old crate names stay on crates.io with a `README.md` pointing at the new
crates; they are not yanked. Existing `1.9.x` users continue to install
and build; new users land on `2.0.0` under the new name.

## Alternatives considered and rejected

| Alternative                                      | Reason rejected                                                                                                                                                                 |
| ------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Keep "CodeLens" name, add symbiotic tagline only | User explicitly requested the rename after reviewing the rootkit / Marvel overlap. Tagline-only leaves the product name mismatched with its actual role as a harness substrate. |
| "Mantle MCP"                                     | Safer brand-conflict profile but weaker metaphor. Symbiote conveys the attachment + augmentation relationship better than a geological layer.                                   |
| "CoEvolve MCP" / "HarnessWeaver"                 | Descriptive but not evocative. Symbiote has immediate visceral recognition.                                                                                                     |
| Stay at `codelens` forever                       | Locks the brand to a stale framing ("just code reading"). Product is already well past that.                                                                                    |

## Known risks (disclosed upfront, accepted by user)

### R1 — Linux rootkit namesake (Intezer Labs, 2022)

A BPF/`LD_PRELOAD`-based Linux userland rootkit was publicly disclosed
as "Symbiote" by Intezer Labs. The name is not registered as a trademark
by Marvel, Intezer, or any operating vendor, so there is no legal block.
But search-engine results for bare "Symbiote" are dominated by the
rootkit story for at least the next 2-3 years. Mitigations:

- Full product name is **Symbiote MCP**, disambiguating from the rootkit
  in every search context (MCP is now a well-indexed term since the
  protocol went mainstream).
- README opens with the rootkit disambiguation explicitly, rather than
  hoping users won't notice.
- All social/announcement copy uses "Symbiote MCP" or "Symbiote — the
  MCP harness substrate", never bare "Symbiote".

### R2 — Marvel IP association

"Symbiote" the biological/colloquial term predates Marvel's 1988
introduction by over a century. Marvel trademarks specific names
(Venom, Carnage) and character designs, not the term "symbiote" itself.
Mitigations:

- No Marvel logos, character likenesses, or codenames ("Venom-mode",
  "Carnage", "Eddie Brock") in any marketing surface.
- If Marvel contacts for trademark concern, respond with documentation
  showing we use the pre-Marvel biological term.

### R3 — Migration cost and user confusion

crates.io publishes are irrevocable; a rebrand cannot hide the old name.
Mitigations:

- v1.9.x remains fully supported under `codelens-*` crate names through
  an explicit sunset horizon (minimum 6 months after v2.0.0 GA).
- `docs/migrate-from-codelens.md` ships in v2.0.0 with line-by-line
  config diffs for Claude Code, Codex, Cursor, Cline, Windsurf, CI.
- Old repo URL and crate pages redirect / link to the new name.
- v2.0.0 release notes open with "What changed" and "Nothing in your
  session breaks unless you upgrade" — the migration is pull-based.

## Execution plan

Rebrand rolls out across three commits + one migration session.

### Phase 1 — tagline + README + ADR (this commit)

- `docs/adr/ADR-0007-symbiote-rebrand.md` (this file).
- `README.md`: opening paragraph adds "**Symbiote MCP** — harness-
  engineering as a symbiotic substrate" tagline alongside the current
  "CodeLens MCP" product name. No crate rename yet; users still
  install `codelens-mcp`. The tagline declares intent and starts
  search indexing for the new name.
- `Cargo.toml` description: augmented with the symbiote tagline.

### Phase 2 — v1.9.45 compatibility groundwork (separate session)

- Emit both `codelens://` and `symbiote://` resource URI variants.
- Accept both `CODELENS_*` and `SYMBIOTE_*` env vars.
- Announcement post or GitHub Discussions thread: "CodeLens MCP is
  becoming Symbiote MCP at v2.0.0".

### Phase 3 — v2.0.0 cutover (dedicated session)

- Crate rename across workspace.
- New crates.io publishes under `symbiote-*`.
- Homebrew tap, install.sh, GitHub repo rename.
- `docs/migrate-from-codelens.md` with migration recipes.
- Old crates.io entries keep their READMEs pointing at the new name.

### Phase 4 — v1.9.x maintenance sunset (6+ months after v2.0.0)

- Bug fixes only on v1.9.x.
- New features land only on `symbiote-*` v2.x.
- v1.9.x goes into passive-archive status.

## Consequences

### Positive

- Product name matches product role. "Symbiote MCP" telegraphs "attach
  this to your existing agent" to any first-time reader; "CodeLens MCP"
  does not.
- Clears the conceptual debt of the brand lagging the architecture —
  we've been explaining "CodeLens is actually a harness substrate" in
  every ADR since ADR-0005.
- Gives a clean v2.0.0 story for external audiences (blog posts,
  benchmark submissions, conference talks).

### Negative

- Users on v1.9.x must opt into the migration. Some will stay behind.
- Rootkit-namesake friction in search for an estimated 2-3 years.
- Dual-namespace maintenance burden in v1.9.45 (Phase 2) until v2.0.0.

### Neutral

- The architecture does not change. ADR-0005 harness-v2, ADR-0006
  routing enforcement, and the handoff artifact v1 schema all carry
  over unchanged except for the prefix rename.

## References

- [ADR-0005 Harness v2](ADR-0005-harness-v2.md)
- [ADR-0006 Agent routing enforcement](ADR-0006-agent-routing-enforcement.md)
- Intezer Labs (2022). Symbiote Linux userland rootkit disclosure.
- Heinrich Anton de Bary (1879). _Die Erscheinung der Symbiose_
  (first formal definition of symbiosis in biology).
