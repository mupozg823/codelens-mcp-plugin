# Architecture Decision Records (ADR)

This directory contains all Architecture Decision Records for the CodeLens project.

## Index

| ADR                                                                            | Title                                                                      | Status                           | Date       | Supersedes           |
| ------------------------------------------------------------------------------ | -------------------------------------------------------------------------- | -------------------------------- | ---------- | -------------------- |
| [ADR-0001](ADR-0001-runtime-boundaries-and-single-source-registries.md)        | Runtime Boundaries and Single-Source Registries                            | Proposed                         | 2026-04-12 | —                    |
| [ADR-0002](ADR-0002-enterprise-productization-evaluation-and-release-gates.md) | Enterprise Productization, Evaluation, and Release Gates                   | Proposed                         | 2026-04-13 | —                    |
| [ADR-0004](ADR-0004-multi-agent-concurrency-primitives.md)                     | Multi-Agent Concurrency Primitives (Bounded-Evidence Only)                 | **Accepted**                     | 2026-04-15 | —                    |
| [ADR-0005](ADR-0005-harness-v2.md)                                             | Harness v2 — CodeLens as shared substrate for role-specialized agent hosts | **Accepted**                     | 2026-04-18 | —                    |
| [ADR-0006](ADR-0006-agent-routing-enforcement.md)                              | Agent Routing Enforcement (server-side `preferred_executor` metadata)      | **Accepted**                     | 2026-04-18 | —                    |
| [ADR-0007](ADR-0007-symbiote-rebrand.md)                                       | Symbiote as the v2 product metaphor and candidate rename                   | **Accepted** (pending trademark) | 2026-04-18 | —                    |
| [ADR-0008](ADR-0008-serena-upper-compatible-absorption.md)                     | Serena Upper-Compatible Absorption (P1-P4, passive first)                  | **Accepted**                     | 2026-04-19 | —                    |
| [ADR-0009](ADR-0009-mutation-trust-substrate.md)                               | Mutation Trust Substrate                                                   | Proposed                         | 2026-04-26 | Internal G4/G7 notes |
| [ADR-0010](0010-telemetry-driven-tool-diet.md)                                 | Telemetry-Driven Tool Surface Diet                                         | Proposed                         | 2026-04-30 | —                    |
| [ADR-0011](ADR-0011-control-plane-sprawl-resolution.md)                        | Control-Plane Sprawl Resolution                                            | **Accepted**                     | 2026-05-01 | —                    |
| [ADR-0012](ADR-0012-onnx-default-off.md)                                       | Default-Off Semantic on Cargo-Install Path                                 | **Accepted**                     | 2026-05-02 | —                    |
| [ADR-0013](ADR-0013-tool-defs-codegen.md)                                      | TOML-Driven Codegen for Tool Definitions                                   | **Accepted**                     | 2026-05-02 | —                    |

## Status Legend

- **Proposed** — Under discussion; not yet committed to implementation.
- **Accepted** — Decision made and implementation in progress or complete.
- **Deprecated** — No longer applicable; superseded by a newer ADR.
- **Rejected** — Explicitly decided against.

## Contributing

When adding a new ADR:

1. Use the next sequential number (e.g., `ADR-0011-...`).
2. Copy the template from [design/adr-template.md](../design/adr-template.md) if available, or follow the structure of existing ADRs.
3. Update this index.
4. Link the ADR from relevant code comments where the decision is enforced.
