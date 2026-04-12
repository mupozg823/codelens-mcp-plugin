# ADR-0002: Enterprise Productization, Evaluation, And Release Gates

- Status: Proposed
- Date: 2026-04-13

## Context

CodeLens already has strong local architecture and strong internal retrieval numbers.
What it does **not** yet have is an enterprise-grade proof story.

The current gap is not "missing more tools."
The current gap is a productization gap across four layers:

1. retrieval quality is measured mostly on CodeLens-owned Rust datasets
2. some semantic query improvements are partly project-specific rather than clearly generic
3. release packaging is not yet supply-chain-grade
4. runtime observability and policy gates are weaker than enterprise buyers expect

This creates three concrete risks:

1. benchmark gains may not generalize across languages or repositories
2. release artifacts may be difficult to trust, verify, or operate in controlled environments
3. strong local architecture can still fail enterprise adoption if evaluation and operations are under-specified

## Decision

We will treat CodeLens enterprise readiness as a first-class architecture concern.

The productization strategy is:

- keep the current two-crate split
- keep tree-sitter-first fast paths
- keep bounded MCP workflows
- add a stricter evaluation model
- separate generic retrieval improvements from project-specific adaptation
- raise release packaging, provenance, and runtime observability to enterprise-grade defaults

## Decision Details

### 1. Separate Generic Retrieval From Project-Specific Adaptation

Retrieval/query shaping must be split into two explicit tiers:

- generic adaptation
- project-specific adaptation

Generic adaptation includes changes that are expected to transfer across repositories and languages, such as:

- identifier splitting
- path/name-path shaping
- generic natural-language framing for code search models
- candidate pool sizing
- model-independent rerank blending

Project-specific adaptation includes changes that depend on CodeLens tool names, workflow names, or repository-local conventions, such as:

- `run_stdio`
- `mutation_gate`
- `prepare_harness_session`
- CodeLens-only workflow aliases

Rule:

- generic adaptation may remain on by default
- project-specific adaptation must be isolated behind an explicit registry, profile, or benchmark-only toggle

The default product claim must be based on generic adaptation, not on repository-local prompt bridging.

### 2. Replace Single-Repo Evaluation With Tiered Product Evaluation

Evaluation must move from "internal benchmark success" to "product evidence."

CodeLens will maintain three evaluation tiers:

#### Tier A: Core Regression

Purpose:

- prevent accidental local regressions during day-to-day changes

Inputs:

- self dataset
- role dataset
- token efficiency
- runtime benchmark

These remain useful, but they are no longer sufficient for public or enterprise claims.

#### Tier B: Cross-Repo Generalization

Purpose:

- validate retrieval and workflow behavior on non-CodeLens repositories

Required repo families:

- Python web/service repo
- TypeScript/JavaScript app or framework repo
- systems-language repo (`C`, `C++`, `Go`, or Rust external)
- JVM or Kotlin/Java repo

Minimum rule:

- no retrieval claim is considered product-grade if it is supported only by CodeLens-owned datasets

#### Tier C: Promotion Gate

Purpose:

- decide whether a retrieval/runtime/release change may be shipped

Promotion gates must compare:

- baseline
- generic adaptation only
- project-specific adaptation if enabled

This prevents a CodeLens-specific bridge from being misread as a general model improvement.

### 3. Define Stable Product Quality Gates

Enterprise release quality must be defined by gates, not by intuition.

Every promotion-targeted release should meet these gates:

#### Retrieval Gates

- no regression in internal workflow-critical datasets without explicit sign-off
- cross-repo benchmark evidence exists for the affected feature area
- gains are reported separately for lexical, semantic, and hybrid paths
- project-specific adaptation is measured separately from generic adaptation

#### Contract Gates

- stable MCP structured content for workflow tools
- output schema coverage for enterprise-facing tools
- backward-compatible response evolution unless a breaking change is explicitly versioned

#### Runtime Gates

- startup, stdio, and HTTP transport smoke tests pass
- health status, fallback reason, and active surface are visible through API/resource outputs
- mutation-gate and preflight regressions are blocked by tests

#### Release Gates

- reproducible release build
- SBOM generated
- provenance generated
- artifact signing/verification path documented and tested

### 4. Add Supply-Chain-Grade Packaging

CodeLens should ship as a verifiable product, not just a local binary.

Target release outputs:

- standalone binary
- OCI image
- Homebrew-installable binary
- air-gapped bundle containing:
  - binary
  - model assets
  - checksums
  - SBOM
  - provenance metadata
  - default config/profile examples

The package format must support environments where online model fetch or dynamic dependency resolution is not acceptable.

### 5. Add Enterprise Observability

Enterprise operation requires correlated metrics, logs, and traces.

CodeLens should emit structured telemetry for:

- tool latency
- index freshness
- semantic fallback reasons
- cache hit rates
- mutation-gate denials
- workflow routing decisions
- benchmark promotion outcomes

Preferred direction:

- OpenTelemetry-aligned traces, metrics, and logs
- one correlation model across stdio, HTTP, and background analysis jobs

### 6. Keep Fast Path / Precise Path Explicit

CodeLens should not pretend one retrieval path solves all precision problems.

The product should explicitly distinguish:

- fast path
- precise path

Fast path:

- tree-sitter
- SQLite
- local hybrid ranking

Precise path:

- optional precise index import or language-server-backed evidence
- provenance/confidence surfaced in the result

This preserves current speed while giving enterprise users a path to higher precision where required.

## Consequences

### Positive

- clearer product claims
- less benchmark overfitting risk
- easier enterprise procurement and security review
- better release trust and operational visibility
- cleaner separation between model quality and repository-local prompt tricks

### Negative

- more benchmark maintenance
- more CI cost
- slower promotion process
- more explicit release engineering work

## Non-Goals

- replacing the current two-crate architecture
- removing tree-sitter-first retrieval
- requiring a precise backend for default use
- claiming universal retrieval gains from CodeLens-internal datasets alone
- shipping every enterprise feature in one release

## Roadmap

### Phase P0: Evidence And Adaptation Hygiene

- split retrieval adaptation into generic vs project-specific layers
- add dataset lint checks for:
  - expected symbol kind alignment
  - positive/negative contradiction
  - file existence
  - query intent vs expected target class
- treat bridge-off / generic-on / project-on as separate benchmark arms
- require at least one external Python and one external TypeScript benchmark in promotion evidence

### Phase P1: Product Evaluation And Contracts

- formalize cross-repo benchmark matrix
- expand workflow-level regression tests
- improve output schema coverage for enterprise-facing tools
- keep health and fallback facts exposed through stable API shapes
- document exact promotion thresholds in the evaluation contract

### Phase P2: Packaging, Provenance, And Operations

- generate CycloneDX SBOM in release pipeline
- add SLSA-style provenance output
- add signing and verification flow for release artifacts
- publish OCI and air-gapped packaging paths
- add OpenTelemetry-aligned runtime instrumentation

### Phase P3: Precision Federation

- add fast-path vs precise-path provenance to response metadata
- prototype precise backend federation (`SCIP` import or equivalent)
- keep precise mode optional and bounded
- evaluate precision uplift separately from generic fast-path quality

## Acceptance Signals

This ADR is succeeding when all of the following become true:

- public retrieval claims are backed by external multi-language evidence
- project-specific bridges are no longer mixed into generic product claims
- release artifacts can be traced, verified, and audited
- runtime health and fallback reasons are observable across transports
- enterprise documentation can point to one promotion gate and one release verification path

## References

- Model Context Protocol tools and structured tool contracts:
  - <https://modelcontextprotocol.io/specification/draft/server/tools>
- Sourcegraph precise code navigation and SCIP:
  - <https://sourcegraph.com/docs/code-navigation/precise-code-navigation>
- NIST AI Risk Management Framework:
  - <https://www.nist.gov/itl/ai-risk-management-framework>
- OpenTelemetry logs, traces, metrics, and collector model:
  - <https://opentelemetry.io/docs/specs/otel/logs/>
  - <https://opentelemetry.io/docs/>
- CycloneDX SBOM guidance:
  - <https://cyclonedx.org/guides/sbom/introduction/>
- SLSA provenance levels:
  - <https://slsa.dev/spec/v1.0/levels>
- Sigstore/cosign verification:
  - <https://docs.sigstore.dev/cosign/verifying/verify/>
