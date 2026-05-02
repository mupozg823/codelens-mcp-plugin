//! Multi-language call-graph accuracy bench (v1.12.0).
//!
//! Runs `extract_calls` against the fixtures under
//! `benchmarks/call-graph-accuracy/fixtures/{rust,js,python,go,java}/` and
//! computes precision / recall / F1 against the ground truth in
//! `benchmarks/call-graph-accuracy/expected.json`. Fails the test (and
//! therefore CI) when overall F1 dips below the threshold declared in
//! `expected.json` (`f1_threshold`).
//!
//! Why a regression test rather than `cargo bench`:
//! - we want CI to fail on accuracy regressions, which `cargo test`
//!   already does automatically; criterion's bench harness does not
//!   gate CI by default.
//! - precision / recall / F1 are exact numbers — there is no
//!   meaningful "warmup" phase that criterion would help with.
//! - keeping this in the engine's `tests/` directory means contributors
//!   running `cargo test -p codelens-engine` see the bench results
//!   inline with the rest of the test suite.
//!
//! Adding a new fixture: drop the source file under `fixtures/<lang>/`,
//! append a `{"path": ..., "edges": [...]}` entry to `expected.json`,
//! and rerun `cargo test -p codelens-engine call_graph_accuracy`. If the
//! tree-sitter extractor disagrees with your hand-written ground truth,
//! the test prints a diff so you can decide which side is wrong.

use codelens_engine::extract_calls;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct Manifest {
    f1_threshold: f64,
    fixtures: Vec<Fixture>,
}

#[derive(Debug, Deserialize)]
struct Fixture {
    path: String,
    edges: Vec<ExpectedEdge>,
}

#[derive(Debug, Deserialize, Clone, Eq, PartialEq, Hash)]
struct ExpectedEdge {
    caller: String,
    callee: String,
}

fn workspace_root() -> PathBuf {
    // tests/ runs from the engine crate dir; back up to the workspace root.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn load_manifest() -> Manifest {
    let manifest_path = workspace_root()
        .join("benchmarks")
        .join("call-graph-accuracy")
        .join("expected.json");
    let body = std::fs::read_to_string(&manifest_path).unwrap_or_else(|err| {
        panic!(
            "failed to read manifest at {}: {err}",
            manifest_path.display()
        )
    });
    serde_json::from_str(&body).expect("manifest must parse as v1 schema")
}

fn extract_observed(path: &Path) -> HashSet<ExpectedEdge> {
    extract_calls(path)
        .into_iter()
        .map(|edge| ExpectedEdge {
            caller: edge.caller_name,
            callee: edge.callee_name,
        })
        .collect()
}

#[derive(Debug, Default)]
struct FixtureScore {
    name: String,
    tp: usize,
    fp: usize,
    fn_: usize,
    missing: Vec<ExpectedEdge>,
    spurious: Vec<ExpectedEdge>,
}

impl FixtureScore {
    fn precision(&self) -> f64 {
        let denom = (self.tp + self.fp) as f64;
        if denom == 0.0 {
            0.0
        } else {
            self.tp as f64 / denom
        }
    }

    fn recall(&self) -> f64 {
        let denom = (self.tp + self.fn_) as f64;
        if denom == 0.0 {
            0.0
        } else {
            self.tp as f64 / denom
        }
    }

    fn f1(&self) -> f64 {
        let p = self.precision();
        let r = self.recall();
        if p + r == 0.0 {
            0.0
        } else {
            2.0 * p * r / (p + r)
        }
    }
}

fn score(
    name: String,
    expected: &[ExpectedEdge],
    observed: &HashSet<ExpectedEdge>,
) -> FixtureScore {
    let expected_set: HashSet<ExpectedEdge> = expected.iter().cloned().collect();
    let mut s = FixtureScore {
        name,
        ..Default::default()
    };
    for edge in &expected_set {
        if observed.contains(edge) {
            s.tp += 1;
        } else {
            s.fn_ += 1;
            s.missing.push(edge.clone());
        }
    }
    for edge in observed {
        if !expected_set.contains(edge) {
            s.fp += 1;
            s.spurious.push(edge.clone());
        }
    }
    s.missing.sort_by(|a, b| {
        (a.caller.as_str(), a.callee.as_str()).cmp(&(b.caller.as_str(), b.callee.as_str()))
    });
    s.spurious.sort_by(|a, b| {
        (a.caller.as_str(), a.callee.as_str()).cmp(&(b.caller.as_str(), b.callee.as_str()))
    });
    s
}

#[test]
fn call_graph_accuracy_meets_threshold() {
    let manifest = load_manifest();
    let root = workspace_root();
    let mut total_tp = 0usize;
    let mut total_fp = 0usize;
    let mut total_fn = 0usize;
    let mut per_fixture = Vec::new();
    for fixture in &manifest.fixtures {
        let abs = root.join(&fixture.path);
        assert!(abs.exists(), "fixture missing: {}", abs.display());
        let observed = extract_observed(&abs);
        let s = score(fixture.path.clone(), &fixture.edges, &observed);
        total_tp += s.tp;
        total_fp += s.fp;
        total_fn += s.fn_;
        per_fixture.push(s);
    }

    println!("\n── Call-graph accuracy bench (v1.12.0) ──");
    println!(
        "{:<54}  {:>3}  {:>3}  {:>3}  {:>6}  {:>6}  {:>6}",
        "fixture", "TP", "FP", "FN", "P", "R", "F1"
    );
    for s in &per_fixture {
        println!(
            "{:<54}  {:>3}  {:>3}  {:>3}  {:>6.3}  {:>6.3}  {:>6.3}",
            s.name,
            s.tp,
            s.fp,
            s.fn_,
            s.precision(),
            s.recall(),
            s.f1()
        );
        if !s.missing.is_empty() {
            println!("  missing edges (FN):");
            for edge in &s.missing {
                println!("    - {}->{}", edge.caller, edge.callee);
            }
        }
        if !s.spurious.is_empty() {
            println!("  spurious edges (FP):");
            for edge in &s.spurious {
                println!("    - {}->{}", edge.caller, edge.callee);
            }
        }
    }

    let total_p = if total_tp + total_fp == 0 {
        0.0
    } else {
        total_tp as f64 / (total_tp + total_fp) as f64
    };
    let total_r = if total_tp + total_fn == 0 {
        0.0
    } else {
        total_tp as f64 / (total_tp + total_fn) as f64
    };
    let total_f1 = if total_p + total_r == 0.0 {
        0.0
    } else {
        2.0 * total_p * total_r / (total_p + total_r)
    };
    println!("\nOVERALL  TP={total_tp}  FP={total_fp}  FN={total_fn}  P={total_p:.3}  R={total_r:.3}  F1={total_f1:.3}  threshold={threshold:.3}",
        threshold = manifest.f1_threshold);

    assert!(
        total_f1 >= manifest.f1_threshold,
        "call-graph accuracy regression: overall F1={total_f1:.3} < threshold {:.3}. \
         See per-fixture diffs above; either the extractor regressed or \
         expected.json needs an explicit update.",
        manifest.f1_threshold
    );
}
