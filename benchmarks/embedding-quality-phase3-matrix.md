# External Embedding Quality Matrix

- Datasets: 8 (`positive=3`, `negative=2`, `flat=3`)
- Flat band: relative delta under `1.0%`
- Strong band: relative delta at or above `5.0%`

| Phase | Dataset | Language / archetype | Baseline | 2e | 2b+2c | Stacked | Δ abs | Δ rel | Band |
|---|---|---|---:|---:|---:|---:|---:|---:|---|
| phase3a | ripgrep external | Rust / tooling | 0.459 | 0.488 | 0.510 | 0.529 | 0.070 | +15.2% | strong positive |
| phase3b | requests external | Python / app library | 0.584 | 0.570 | 0.522 | 0.495 | -0.089 | -15.2% | strong negative |
| phase3c | jest external | TS/JS / tooling | 0.155 | 0.157 | 0.164 | 0.166 | 0.011 | +7.3% | strong positive |
| phase3d | typescript external | TS/JS / compiler | 0.098 | 0.089 | 0.201 | 0.201 | 0.103 | +104.3% | strong positive |
| phase3e | next-js external | TS/JS / typical app | 0.198 | 0.196 | 0.198 | 0.196 | -0.002 | -0.8% | flat |
| phase3f | react-core external | TS/JS / short runtime | 0.123 | 0.123 | 0.123 | 0.123 | 0.000 | +0.0% | flat |
| phase3g | django external | Python / framework | 0.294 | 0.294 | 0.286 | 0.288 | -0.005 | -1.8% | mild negative |
| phase3h | axum external | Rust / framework library | 0.281 | 0.281 | 0.281 | 0.281 | 0.001 | +0.2% | flat |

## Artefacts

- `phase3a` / `ripgrep external`
  - `baseline`: `benchmarks/embedding-quality-v1.5-phase3a-ripgrep-baseline.json`
  - `2e-only`: `benchmarks/embedding-quality-v1.5-phase3a-ripgrep-2e-only.json`
  - `2b2c-only`: `benchmarks/embedding-quality-v1.5-phase3a-ripgrep-2b2c-only.json`
  - `stacked`: `benchmarks/embedding-quality-v1.5-phase3a-ripgrep-stacked.json`
- `phase3b` / `requests external`
  - `baseline`: `benchmarks/embedding-quality-v1.5-phase3b-requests-baseline.json`
  - `2e-only`: `benchmarks/embedding-quality-v1.5-phase3b-requests-2e-only.json`
  - `2b2c-only`: `benchmarks/embedding-quality-v1.5-phase3b-requests-2b2c-only.json`
  - `stacked`: `benchmarks/embedding-quality-v1.5-phase3b-requests-stacked.json`
- `phase3c` / `jest external`
  - `baseline`: `benchmarks/embedding-quality-v1.5-phase3c-jest-baseline.json`
  - `2e-only`: `benchmarks/embedding-quality-v1.5-phase3c-jest-2e-only.json`
  - `2b2c-only`: `benchmarks/embedding-quality-v1.5-phase3c-jest-2b2c-only.json`
  - `stacked`: `benchmarks/embedding-quality-v1.5-phase3c-jest-stacked.json`
- `phase3d` / `typescript external`
  - `baseline`: `benchmarks/embedding-quality-v1.6-phase3d-typescript-baseline.json`
  - `2e-only`: `benchmarks/embedding-quality-v1.6-phase3d-typescript-2e-only.json`
  - `2b2c-only`: `benchmarks/embedding-quality-v1.6-phase3d-typescript-2b2c-only.json`
  - `stacked`: `benchmarks/embedding-quality-v1.6-phase3d-typescript-stacked.json`
- `phase3e` / `next-js external`
  - `baseline`: `benchmarks/embedding-quality-v1.6-phase3e-next-js-baseline.json`
  - `2e-only`: `benchmarks/embedding-quality-v1.6-phase3e-next-js-2e-only.json`
  - `2b2c-only`: `benchmarks/embedding-quality-v1.6-phase3e-next-js-2b2c-only.json`
  - `stacked`: `benchmarks/embedding-quality-v1.6-phase3e-next-js-stacked.json`
- `phase3f` / `react-core external`
  - `baseline`: `benchmarks/embedding-quality-v1.6-phase3f-react-core-baseline.json`
  - `2e-only`: `benchmarks/embedding-quality-v1.6-phase3f-react-core-2e-only.json`
  - `2b2c-only`: `benchmarks/embedding-quality-v1.6-phase3f-react-core-2b2c-only.json`
  - `stacked`: `benchmarks/embedding-quality-v1.6-phase3f-react-core-stacked.json`
- `phase3g` / `django external`
  - `baseline`: `benchmarks/embedding-quality-v1.6-phase3g-django-baseline.json`
  - `2e-only`: `benchmarks/embedding-quality-v1.6-phase3g-django-2e-only.json`
  - `2b2c-only`: `benchmarks/embedding-quality-v1.6-phase3g-django-2b2c-only.json`
  - `stacked`: `benchmarks/embedding-quality-v1.6-phase3g-django-stacked.json`
- `phase3h` / `axum external`
  - `baseline`: `benchmarks/embedding-quality-v1.6-phase3h-axum-baseline.json`
  - `2e-only`: `benchmarks/embedding-quality-v1.6-phase3h-axum-2e-only.json`
  - `2b2c-only`: `benchmarks/embedding-quality-v1.6-phase3h-axum-2b2c-only.json`
  - `stacked`: `benchmarks/embedding-quality-v1.6-phase3h-axum-stacked.json`

