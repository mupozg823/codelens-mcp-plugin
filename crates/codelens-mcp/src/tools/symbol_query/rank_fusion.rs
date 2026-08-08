//! Stage 4 of the ranked-context pipeline: fuse semantic + sparse
//! retrieval lanes back into the structural ranking.
//!
//! `RankFusionPolicy` keeps the per-query retrieval lane limits in one
//! place so weighted RRF doesn't have to re-derive them. Policies are
//! tuned by query word count today; future query-shape signals can
//! extend the match arms in `rank_fusion_policy` without touching the
//! fusion logic.
//!
//! Visibility: every export here is `pub(super)`. `ranked_context.rs`
//! is the only legitimate caller — these helpers don't make sense in
//! isolation from the pipeline's stage ordering.

use crate::symbol_retrieval::ScoredSymbol;
use codelens_engine::{RankedContextEntry, RankedContextResult, SemanticMatch};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy)]
pub(super) struct RankFusionPolicy {
    pub(super) semantic_limit: usize,
    pub(super) sparse_limit: usize,
}

/// Per-lane weights applied inside weighted RRF. All tunable channel
/// weights live here so the fusion loop never hard-codes them and the
/// query-adaptive experiment (below) has a single source of truth.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct RrfChannelWeights {
    pub(super) w_struct: f64,
    pub(super) w_sem: f64,
    pub(super) w_sparse: f64,
    pub(super) w_user: f64,
}

impl RrfChannelWeights {
    /// Default static weights. The flag-off ranking behaviour depends on
    /// these being byte-for-byte the historical constants — do not retune
    /// without an MRR regression pass.
    pub(super) const DEFAULT: RrfChannelWeights = RrfChannelWeights {
        w_struct: 1.0,
        w_sem: 1.0,
        w_sparse: 0.8,
        w_user: 0.6,
    };
}

/// Experimental (`CODELENS_RRF_ADAPTIVE=1`): shift the semantic/sparse
/// channel weights by query shape. Exact-identifier queries lean on the
/// sparse (BM25F) lane; natural-language queries lean on the dense
/// semantic lane. When `adaptive` is false — the default — this returns
/// [`RrfChannelWeights::DEFAULT`] unchanged so the shipped ranking is
/// untouched.
pub(super) fn resolve_rrf_channel_weights(
    adaptive: bool,
    exact_identifier: bool,
    natural_language: bool,
) -> RrfChannelWeights {
    if !adaptive {
        return RrfChannelWeights::DEFAULT;
    }
    if exact_identifier {
        RrfChannelWeights {
            w_sem: 0.6,
            w_sparse: 1.0,
            ..RrfChannelWeights::DEFAULT
        }
    } else if natural_language {
        RrfChannelWeights {
            w_sem: 1.2,
            w_sparse: 0.6,
            ..RrfChannelWeights::DEFAULT
        }
    } else {
        RrfChannelWeights::DEFAULT
    }
}

/// Which fusion arithmetic stage 4 runs. `RANK_ONLY` is the shipped
/// default: pure reciprocal-rank fusion, lane membership only. The
/// score-aware variant (`CODELENS_RRF_SCORE_AWARE=1`) additionally
/// folds each lane's raw retrieval score into the fused score — see
/// [`score_aware_fused_score`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RankFusionMode {
    pub(super) score_aware: bool,
    /// Score-aware only, opt-in (`CODELENS_RRF_SCORE_AWARE_CAPS=1`),
    /// default off: skip the query-shape lane caps and admit the full
    /// caller-provided lane budget.
    ///
    /// The cap does exclude gold symbols — on the 16-query
    /// issue-localization set 1 of the 9 in-window misses (Q05) has its
    /// gold at sparse lane rank >= 6, outside the natural-language cap
    /// of 4. Lifting it nonetheless bought nothing: A/B on the same
    /// binary scored identically at the default window (hit@1 0.188,
    /// file F1 0.296) and within noise at page_size=10 (file F1 0.206
    /// lifted vs 0.205 capped, function F1 0.129 vs 0.132). The knob
    /// stays so the measurement can be reproduced; the default does not
    /// move on an unpaid-for behaviour change.
    pub(super) lift_policy_caps: bool,
}

impl RankFusionMode {
    /// Shipped default. Every flag-off call must resolve to exactly this.
    pub(super) const RANK_ONLY: RankFusionMode = RankFusionMode {
        score_aware: false,
        lift_policy_caps: false,
    };

    /// Flag off collapses to [`RankFusionMode::RANK_ONLY`] regardless of
    /// the cap knob, so the default path can never pick up a lifted cap.
    pub(super) fn resolve(score_aware: bool, lift_policy_caps: bool) -> RankFusionMode {
        if !score_aware {
            return RankFusionMode::RANK_ONLY;
        }
        RankFusionMode {
            score_aware: true,
            lift_policy_caps,
        }
    }
}

/// Per-lane min/max of the raw retrieval scores actually admitted into
/// fusion. BM25 is unbounded and cosine sits near 0..1, so each lane is
/// normalized independently before the lanes are combined.
#[derive(Debug, Clone, Copy)]
struct LaneNorm {
    min: f64,
    max: f64,
    count: usize,
}

impl LaneNorm {
    /// Below this many candidates a lane carries no usable spread:
    /// min-max over two scores says "best" and "worst" and nothing in
    /// between, so a runner-up that was within a percent of the leader
    /// is handed a hard 0.0. The short-phrase policy caps (semantic 2,
    /// sparse 3) hit exactly that case, and it cost 3 of 112 cases their
    /// place in the cutoff window on the embedding-quality set before
    /// this guard existed.
    const MIN_CANDIDATES_FOR_SPREAD: usize = 3;

    fn observe(slot: &mut Option<LaneNorm>, score: f64) {
        match slot {
            Some(norm) => {
                norm.min = norm.min.min(score);
                norm.max = norm.max.max(score);
                norm.count += 1;
            }
            None => {
                *slot = Some(LaneNorm {
                    min: score,
                    max: score,
                    count: 1,
                })
            }
        }
    }

    /// Min-max to 0..1. A lane with no usable spread — too few
    /// candidates, or every candidate on the same score — normalizes to
    /// 1.0 rather than 0.0: an uninformative lane must not zero out an
    /// otherwise strong candidate's evidence.
    fn normalize(slot: &Option<LaneNorm>, score: Option<f64>) -> Option<f64> {
        let score = score?;
        let norm = slot.as_ref()?;
        if norm.count < Self::MIN_CANDIDATES_FOR_SPREAD {
            return Some(1.0);
        }
        let span = norm.max - norm.min;
        if span <= f64::EPSILON {
            return Some(1.0);
        }
        Some(((score - norm.min) / span).clamp(0.0, 1.0))
    }
}

pub(super) fn rank_fusion_policy(
    query: &str,
    max_semantic: usize,
    max_sparse: usize,
) -> RankFusionPolicy {
    let word_count = query.split_whitespace().count();
    if word_count >= 4 {
        return RankFusionPolicy {
            semantic_limit: max_semantic.min(6),
            sparse_limit: max_sparse.min(4),
        };
    }
    if word_count >= 2 {
        return RankFusionPolicy {
            semantic_limit: max_semantic.min(2),
            sparse_limit: max_sparse.min(3),
        };
    }
    RankFusionPolicy {
        semantic_limit: max_semantic.min(3),
        sparse_limit: max_sparse.min(2),
    }
}

/// Lane caps for the active fusion mode. The query-shape caps of
/// [`rank_fusion_policy`] apply in every shipped configuration; only the
/// opt-in `lift_policy_caps` experiment admits the full caller budget.
pub(super) fn resolve_rank_fusion_policy(
    query: &str,
    max_semantic: usize,
    max_sparse: usize,
    mode: RankFusionMode,
) -> RankFusionPolicy {
    if mode.score_aware && mode.lift_policy_caps {
        return RankFusionPolicy {
            semantic_limit: max_semantic,
            sparse_limit: max_sparse,
        };
    }
    rank_fusion_policy(query, max_semantic, max_sparse)
}

pub(super) fn compact_semantic_evidence(
    result: &RankedContextResult,
    semantic_results: &[SemanticMatch],
    limit: usize,
) -> Vec<Value> {
    let mut final_ranks = std::collections::HashMap::new();
    for (idx, entry) in result.symbols.iter().enumerate() {
        final_ranks.insert(format!("{}:{}", entry.file, entry.name), idx + 1);
    }

    semantic_results
        .iter()
        .take(limit)
        .map(|item| {
            let key = format!("{}:{}", item.file_path, item.symbol_name);
            let final_rank = final_ranks.get(&key).copied();
            json!({
                "symbol": item.symbol_name,
                "file": item.file_path,
                "score": (item.score * 1000.0).round() / 1000.0,
                "selected": final_rank.is_some(),
                "final_rank": final_rank,
            })
        })
        .collect()
}

struct RankEntry {
    name: String,
    kind: String,
    file: String,
    line: usize,
    signature: String,
    structural_rank: Option<usize>,
    semantic_rank: Option<usize>,
    sparse_rank: Option<usize>,
    user_context_rank: Option<usize>,
    /// Raw per-lane retrieval scores, kept alongside the ranks so
    /// score-aware fusion can normalize them. Rank-only fusion ignores
    /// these fields entirely.
    structural_score: Option<f64>,
    semantic_score: Option<f64>,
    sparse_score: Option<f64>,
    user_context_score: Option<f64>,
}

/// Reciprocal-rank damping constant, shared by both fusion modes.
const RRF_K: f64 = 60.0;

/// Score-aware only: bounded additive bonus for a candidate corroborated
/// by more than one lane. Deliberately an order of magnitude below the
/// evidence term's spread so corroboration breaks near-ties instead of
/// forming a hard tier above single-lane candidates — lane-membership
/// count dominating raw score is the exact failure this mode corrects.
const CORROBORATION_GAIN: f64 = 0.05;

/// Score-aware only: the classic RRF term is retained, damped, as a
/// within-lane rank prior. It stays a tiebreak rather than a signal on
/// purpose: the semantic lane is not returned in score order (measured
/// on the issue-localization dumps — Q03 lane order 0.243, 0.207,
/// 0.296, 0.196, 0.307), so its rank carries less information than its
/// score.
const RANK_PRIOR_GAIN: f64 = 0.5;

/// Score-aware fusion.
///
/// ```text
/// fused = Σ_member w_l·S_l / Σ_active w_l   (evidence: normalized weighted sum)
///       + G·(1 − 1/m)                       (corroboration: bounded, m = member lanes)
///       + P·Σ_member w_l/(k + r_l)          (rank prior: within-lane ordering)
/// ```
///
/// The denominator is the weight of every lane *active for this query*,
/// not just the lanes this candidate appears in, so it is constant
/// across candidates and only fixes the scale. What changes versus
/// rank-only RRF is that a lane now contributes in proportion to its
/// normalized score: an extra membership carrying a bottom-of-lane
/// score adds nearly nothing, so a candidate leading two lanes on raw
/// score outranks one that merely appears in three.
fn score_aware_fused_score(
    entry: &RankEntry,
    structural: &Option<LaneNorm>,
    semantic: &Option<LaneNorm>,
    sparse: &Option<LaneNorm>,
    user_context: &Option<LaneNorm>,
    weights: RrfChannelWeights,
) -> f64 {
    // A lane is "active for this query" when it produced any candidate,
    // which is exactly when its normalizer exists.
    let active_weight_sum = [
        (weights.w_struct, structural.is_some()),
        (weights.w_sem, semantic.is_some()),
        (weights.w_sparse, sparse.is_some()),
        (weights.w_user, user_context.is_some()),
    ]
    .into_iter()
    .filter(|(_, active)| *active)
    .map(|(weight, _)| weight)
    .sum::<f64>();
    if active_weight_sum <= f64::EPSILON {
        return 0.0;
    }

    let lanes = [
        (
            weights.w_struct,
            entry.structural_rank,
            LaneNorm::normalize(structural, entry.structural_score),
        ),
        (
            weights.w_sem,
            entry.semantic_rank,
            LaneNorm::normalize(semantic, entry.semantic_score),
        ),
        (
            weights.w_sparse,
            entry.sparse_rank,
            LaneNorm::normalize(sparse, entry.sparse_score),
        ),
        (
            weights.w_user,
            entry.user_context_rank,
            LaneNorm::normalize(user_context, entry.user_context_score),
        ),
    ];

    let mut evidence = 0.0;
    let mut rank_prior = 0.0;
    let mut lane_count = 0_u32;
    for (weight, rank, normalized) in lanes {
        let Some(rank) = rank else {
            continue;
        };
        lane_count += 1;
        evidence += weight * normalized.unwrap_or(0.0);
        rank_prior += weight / (RRF_K + rank as f64);
    }
    if lane_count == 0 {
        return 0.0;
    }

    let corroboration = CORROBORATION_GAIN * (1.0 - 1.0 / f64::from(lane_count));
    evidence / active_weight_sum + corroboration + RANK_PRIOR_GAIN * rank_prior
}

#[allow(clippy::too_many_arguments)]
pub(super) fn fuse_ranked_entries_weighted_rrf(
    query: &str,
    result: &mut RankedContextResult,
    semantic_results: Vec<SemanticMatch>,
    sparse_results: Vec<ScoredSymbol>,
    max_semantic_entries: usize,
    max_sparse_entries: usize,
    user_context_scores: Option<&std::collections::HashMap<String, f64>>,
    weights: RrfChannelWeights,
    mode: RankFusionMode,
) {
    let policy = resolve_rank_fusion_policy(query, max_semantic_entries, max_sparse_entries, mode);
    let mut entries_map = std::collections::HashMap::new();

    // 1) Structural lane.
    for (idx, item) in result.symbols.iter().enumerate() {
        let key = format!("{}:{}", item.file, item.name);
        entries_map.insert(
            key,
            RankEntry {
                name: item.name.clone(),
                kind: item.kind.clone(),
                file: item.file.clone(),
                line: item.line,
                signature: item.signature.clone(),
                structural_rank: Some(idx + 1),
                semantic_rank: None,
                sparse_rank: None,
                user_context_rank: None,
                structural_score: Some(f64::from(item.relevance_score)),
                semantic_score: None,
                sparse_score: None,
                user_context_score: None,
            },
        );
    }

    // 2) Semantic lane, capped by policy.
    for (idx, item) in semantic_results
        .into_iter()
        .take(policy.semantic_limit)
        .enumerate()
    {
        let key = format!("{}:{}", item.file_path, item.symbol_name);
        if let Some(entry) = entries_map.get_mut(&key) {
            entry.semantic_rank = Some(idx + 1);
            entry.semantic_score = Some(item.score);
        } else {
            entries_map.insert(
                key,
                RankEntry {
                    name: item.symbol_name,
                    kind: item.kind,
                    file: item.file_path,
                    line: item.line,
                    signature: item.signature,
                    structural_rank: None,
                    semantic_rank: Some(idx + 1),
                    sparse_rank: None,
                    user_context_rank: None,
                    structural_score: None,
                    semantic_score: Some(item.score),
                    sparse_score: None,
                    user_context_score: None,
                },
            );
        }
    }

    // 3) Sparse lane, capped by policy.
    for (idx, item) in sparse_results
        .into_iter()
        .take(policy.sparse_limit)
        .enumerate()
    {
        let key = format!("{}:{}", item.document.file_path, item.document.name);
        if let Some(entry) = entries_map.get_mut(&key) {
            entry.sparse_rank = Some(idx + 1);
            entry.sparse_score = Some(item.score);
        } else {
            entries_map.insert(
                key,
                RankEntry {
                    name: item.document.name,
                    kind: item.document.kind,
                    file: item.document.file_path,
                    line: item.document.line_start,
                    signature: item.document.signature,
                    structural_rank: None,
                    semantic_rank: None,
                    sparse_rank: Some(idx + 1),
                    user_context_rank: None,
                    structural_score: None,
                    semantic_score: None,
                    sparse_score: Some(item.score),
                    user_context_score: None,
                },
            );
        }
    }

    // 4) User context lane.
    if let Some(uc_scores) = user_context_scores {
        let mut uc_sorted: Vec<(&String, &f64)> = uc_scores.iter().collect();
        uc_sorted.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
        for (idx, (file_key, score)) in uc_sorted.into_iter().enumerate().take(5) {
            for entry in entries_map.values_mut() {
                if &entry.file == file_key {
                    entry.user_context_rank = Some(idx + 1);
                    entry.user_context_score = Some(*score);
                }
            }
        }
    }

    // Weighted reciprocal-rank fusion.
    let k = RRF_K;
    let RrfChannelWeights {
        w_struct,
        w_sem,
        w_sparse,
        w_user,
    } = weights;

    // Score-aware mode needs each lane's score range before it can score
    // any single entry, so the normalizers are collected in one pass.
    let (mut structural_norm, mut semantic_norm, mut sparse_norm, mut user_context_norm) =
        (None, None, None, None);
    if mode.score_aware {
        for entry in entries_map.values() {
            if let (Some(_), Some(score)) = (entry.structural_rank, entry.structural_score) {
                LaneNorm::observe(&mut structural_norm, score);
            }
            if let (Some(_), Some(score)) = (entry.semantic_rank, entry.semantic_score) {
                LaneNorm::observe(&mut semantic_norm, score);
            }
            if let (Some(_), Some(score)) = (entry.sparse_rank, entry.sparse_score) {
                LaneNorm::observe(&mut sparse_norm, score);
            }
            if let (Some(_), Some(score)) = (entry.user_context_rank, entry.user_context_score) {
                LaneNorm::observe(&mut user_context_norm, score);
            }
        }
    }

    let mut scored_entries = Vec::new();
    for (_, entry) in entries_map {
        if mode.score_aware {
            let fused = score_aware_fused_score(
                &entry,
                &structural_norm,
                &semantic_norm,
                &sparse_norm,
                &user_context_norm,
                weights,
            );
            scored_entries.push((entry, fused));
            continue;
        }
        let mut rrf_score = 0.0;
        if let Some(r) = entry.structural_rank {
            rrf_score += w_struct / (k + r as f64);
        }
        if let Some(r) = entry.semantic_rank {
            rrf_score += w_sem / (k + r as f64);
        }
        if let Some(r) = entry.sparse_rank {
            rrf_score += w_sparse / (k + r as f64);
        }
        if let Some(r) = entry.user_context_rank {
            rrf_score += w_user / (k + r as f64);
        }
        scored_entries.push((entry, rrf_score));
    }

    // Sort by descending RRF score.
    if mode.score_aware {
        // `entries_map` is a HashMap, so ties would otherwise resolve by
        // iteration order and differ run to run. Score-aware mode pins
        // them to a stable content key.
        scored_entries.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.file.cmp(&b.0.file))
                .then_with(|| a.0.name.cmp(&b.0.name))
                .then_with(|| a.0.line.cmp(&b.0.line))
        });
    } else {
        scored_entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    }

    if scored_entries.is_empty() {
        result.symbols.clear();
        result.count = 0;
    } else if scored_entries.len() == 1 {
        let (entry, _) = scored_entries.remove(0);
        result.symbols = vec![RankedContextEntry {
            name: entry.name,
            kind: entry.kind,
            file: entry.file,
            line: entry.line,
            signature: entry.signature,
            body: None,
            relevance_score: 100,
        }];
        result.count = 1;
    } else {
        let max_rrf = scored_entries.first().map(|x| x.1).unwrap_or(0.0);
        let min_rrf = scored_entries.last().map(|x| x.1).unwrap_or(0.0);
        let diff = max_rrf - min_rrf;

        let mut final_symbols = Vec::new();
        for (entry, rrf_score) in scored_entries {
            let relevance_score = if diff > 1e-9 {
                let norm = (rrf_score - min_rrf) / diff;
                (norm * 99.0 + 1.0).round() as i32
            } else {
                100
            };

            final_symbols.push(RankedContextEntry {
                name: entry.name,
                kind: entry.kind,
                file: entry.file,
                line: entry.line,
                signature: entry.signature,
                body: None,
                relevance_score,
            });
        }

        result.symbols = final_symbols;
        result.count = result.symbols.len();
    }
}

pub(super) fn compact_sparse_evidence(
    result: &RankedContextResult,
    sparse_results: &[ScoredSymbol],
    limit: usize,
) -> Vec<Value> {
    let mut final_ranks = std::collections::HashMap::new();
    for (idx, entry) in result.symbols.iter().enumerate() {
        final_ranks.insert(format!("{}:{}", entry.file, entry.name), idx + 1);
    }

    sparse_results
        .iter()
        .take(limit)
        .map(|item| {
            let key = format!("{}:{}", item.document.file_path, item.document.name);
            let final_rank = final_ranks.get(&key).copied();
            json!({
                "symbol": item.document.name,
                "file": item.document.file_path,
                "score": (item.score * 1000.0).round() / 1000.0,
                "matched_terms": item.matched_terms,
                "selected": final_rank.is_some(),
                "final_rank": final_rank,
            })
        })
        .collect()
}

pub(super) fn annotate_ranked_context_provenance(
    payload: &mut Value,
    structural_keys: &std::collections::HashSet<String>,
    semantic_results: &[SemanticMatch],
    sparse_results: &[ScoredSymbol],
) {
    let semantic_scores = semantic_results
        .iter()
        .map(|item| {
            (
                format!("{}:{}", item.file_path, item.symbol_name),
                (item.score * 1000.0).round() / 1000.0,
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    let sparse_scores = sparse_results
        .iter()
        .map(|item| {
            (
                format!("{}:{}", item.document.file_path, item.document.name),
                (item.score * 1000.0).round() / 1000.0,
            )
        })
        .collect::<std::collections::HashMap<_, _>>();

    let Some(symbols) = payload.get_mut("symbols").and_then(Value::as_array_mut) else {
        return;
    };

    for entry in symbols {
        let Some(map) = entry.as_object_mut() else {
            continue;
        };
        let Some(file) = map.get("file").and_then(Value::as_str) else {
            continue;
        };
        let Some(name) = map.get("name").and_then(Value::as_str) else {
            continue;
        };

        let key = format!("{file}:{name}");
        let semantic_score = semantic_scores.get(&key).copied();
        let sparse_score = sparse_scores.get(&key).copied();
        let structural_candidate = structural_keys.contains(&key);
        let source = match (semantic_score, sparse_score, structural_candidate) {
            (Some(_), _, true) => "semantic_boosted",
            (Some(_), _, false) => "semantic_added",
            (None, Some(_), true) => "sparse_boosted",
            (None, Some(_), false) => "sparse_added",
            (None, None, _) => "structural",
        };
        let confidence = match source {
            "semantic_added" => "medium",
            "sparse_added" => "medium_high",
            "semantic_boosted" | "sparse_boosted" => "high",
            _ => "medium",
        };
        map.insert(
            "provenance".to_owned(),
            json!({
                "source": source,
                "confidence": confidence,
                "corroborated": structural_candidate && (semantic_score.is_some() || sparse_score.is_some()),
                "structural_candidate": structural_candidate,
                "semantic_score": semantic_score,
                "sparse_score": sparse_score,
            }),
        );
    }
}

#[cfg(test)]
mod score_aware_fusion_tests {
    use super::{
        RankFusionMode, RrfChannelWeights, fuse_ranked_entries_weighted_rrf, rank_fusion_policy,
        resolve_rank_fusion_policy,
    };
    use crate::symbol_corpus::SymbolDocument;
    use crate::symbol_retrieval::ScoredSymbol;
    use codelens_engine::{RankedContextEntry, RankedContextResult, SemanticMatch};

    /// Natural-language query (>= 4 words) so the fixtures exercise the
    /// semantic 6 / sparse 4 policy caps, matching the benchmark queries.
    const NL_QUERY: &str = "accept path as soft alias of scope";

    /// The mode `CODELENS_RRF_SCORE_AWARE=1` actually ships: scores in
    /// fusion, query-shape lane caps untouched.
    const SCORE_AWARE: RankFusionMode = RankFusionMode {
        score_aware: true,
        lift_policy_caps: false,
    };

    fn structural(name: &str, file: &str, line: usize, relevance_score: i32) -> RankedContextEntry {
        RankedContextEntry {
            name: name.to_owned(),
            kind: "function".to_owned(),
            file: file.to_owned(),
            line,
            signature: format!("fn {name}"),
            body: None,
            relevance_score,
        }
    }

    fn semantic(name: &str, file: &str, line: usize, score: f64) -> SemanticMatch {
        SemanticMatch {
            symbol_name: name.to_owned(),
            kind: "function".to_owned(),
            file_path: file.to_owned(),
            line,
            signature: format!("fn {name}"),
            name_path: name.to_owned(),
            score,
        }
    }

    fn sparse(name: &str, file: &str, line: usize, score: f64) -> ScoredSymbol {
        ScoredSymbol {
            document: SymbolDocument {
                symbol_id: format!("{file}:{name}"),
                name: name.to_owned(),
                name_path: name.to_owned(),
                kind: "function".to_owned(),
                signature: format!("fn {name}"),
                file_path: file.to_owned(),
                module_path: "m".to_owned(),
                doc_comment: String::new(),
                body_lexical_chunk: String::new(),
                language: "rust",
                line_start: line,
                is_test: false,
                is_generated: false,
                exported: true,
            },
            score,
            matched_terms: vec!["path".to_owned(), "scope".to_owned()],
        }
    }

    /// Q06 shape (dump `raw-w10-v2/Q06`): the gold symbol leads BOTH
    /// retrieval lanes on raw score but sits in two lanes, while the
    /// symbol that actually won rank 1 sits in three lanes with weaker
    /// scores in both of them.
    fn q06_fixture() -> (RankedContextResult, Vec<SemanticMatch>, Vec<ScoredSymbol>) {
        let result = RankedContextResult {
            query: NL_QUERY.to_owned(),
            count: 3,
            token_budget: 16384,
            chars_used: 512,
            symbols: vec![
                structural("refactor_safety_report", "src/impact/refactor.rs", 11, 92),
                structural("architecture_report", "src/impact/arch.rs", 30, 71),
                structural("filler_symbol", "src/other/filler.rs", 5, 44),
            ],
        };
        let semantic_results = vec![
            semantic("dead_code_report", "src/impact/boundary.rs", 40, 0.397),
            semantic("unused_export_report", "src/impact/unused.rs", 12, 0.331),
            semantic(
                "refactor_safety_report",
                "src/impact/refactor.rs",
                11,
                0.289,
            ),
            semantic("architecture_report", "src/impact/arch.rs", 30, 0.244),
        ];
        let sparse_results = vec![
            sparse("diff_aware_references", "src/impact/impact.rs", 88, 96.786),
            sparse("dead_code_report", "src/impact/boundary.rs", 40, 90.688),
            sparse("unused_export_report", "src/impact/unused.rs", 12, 85.4),
            sparse(
                "refactor_safety_report",
                "src/impact/refactor.rs",
                11,
                82.133,
            ),
        ];
        (result, semantic_results, sparse_results)
    }

    fn fuse(
        fixture: (RankedContextResult, Vec<SemanticMatch>, Vec<ScoredSymbol>),
        mode: RankFusionMode,
    ) -> RankedContextResult {
        let (mut result, semantic_results, sparse_results) = fixture;
        fuse_ranked_entries_weighted_rrf(
            NL_QUERY,
            &mut result,
            semantic_results,
            sparse_results,
            8,
            6,
            Some(&std::collections::HashMap::new()),
            RrfChannelWeights::DEFAULT,
            mode,
        );
        result
    }

    #[test]
    fn rank_only_mode_lets_lane_membership_beat_raw_score() {
        // Regression pin for the shipped default: the three-lane symbol
        // wins even though the two-lane symbol leads both retrieval
        // lanes on raw score. This is the behaviour score-aware mode
        // corrects — if this assertion ever flips, the default path
        // changed and the flag-off contract is broken.
        let fused = fuse(q06_fixture(), RankFusionMode::RANK_ONLY);
        assert_eq!(fused.symbols[0].name, "refactor_safety_report");
    }

    #[test]
    fn score_aware_mode_promotes_the_dual_lane_score_leader() {
        let fused = fuse(q06_fixture(), SCORE_AWARE);
        assert_eq!(fused.symbols[0].name, "dead_code_report");
        assert_eq!(fused.symbols[0].relevance_score, 100);
        let refactor_rank = fused
            .symbols
            .iter()
            .position(|s| s.name == "refactor_safety_report")
            .expect("previous winner stays in the window");
        assert!(
            refactor_rank > 0,
            "weakly-scored corroborated symbol must lose rank 1"
        );
    }

    /// Q14 shape (dump `raw-w10-v2/Q14`). The semantic lane is NOT
    /// returned in score order — measured lane order there is 0.173,
    /// 0.182, 0.238, 0.231, 0.229 — so the gold symbol carries the
    /// lane's best cosine while sitting at lane position 3, and the
    /// symbol holding lane position 1 carries the lane's worst. Under
    /// rank-only fusion the lane position decides; under score-aware
    /// fusion the score does.
    fn q14_fixture() -> (RankedContextResult, Vec<SemanticMatch>, Vec<ScoredSymbol>) {
        let result = RankedContextResult {
            query: NL_QUERY.to_owned(),
            count: 4,
            token_budget: 16384,
            chars_used: 512,
            symbols: vec![
                structural("record_stale_file", "src/jobs/store.rs", 100, 100),
                structural("changed_files", "src/jobs/changed.rs", 12, 97),
                structural("cleanup_stale_files", "src/jobs/cleanup.rs", 55, 95),
                structural("unrelated_tail", "src/other/tail.rs", 4, 20),
            ],
        };
        let semantic_results = vec![
            semantic("record_stale_file", "src/jobs/store.rs", 100, 0.173),
            semantic("changed_files", "src/jobs/changed.rs", 12, 0.182),
            semantic("cleanup_stale_files", "src/jobs/cleanup.rs", 55, 0.238),
        ];
        let sparse_results = vec![
            sparse("concurrent_updates", "src/jobs/tests.rs", 300, 59.187),
            sparse("STAGING_SEQ", "src/jobs/staging.rs", 8, 46.115),
        ];
        (result, semantic_results, sparse_results)
    }

    #[test]
    fn rank_only_mode_follows_an_unsorted_semantic_lane_position() {
        let fused = fuse(q14_fixture(), RankFusionMode::RANK_ONLY);
        assert_eq!(fused.symbols[0].name, "record_stale_file");
    }

    #[test]
    fn score_aware_mode_follows_the_semantic_score_not_the_lane_position() {
        let fused = fuse(q14_fixture(), SCORE_AWARE);
        assert_eq!(fused.symbols[0].name, "cleanup_stale_files");
    }

    #[test]
    fn score_aware_mode_still_prefers_corroboration_when_scores_tie() {
        // Corroboration is a bounded bonus, not a hard tier: with equal
        // lane-leading scores the multi-lane candidate must still win.
        let result = RankedContextResult {
            query: NL_QUERY.to_owned(),
            count: 1,
            token_budget: 16384,
            chars_used: 128,
            symbols: vec![structural("both_lanes", "src/a.rs", 1, 100)],
        };
        let semantic_results = vec![
            semantic("both_lanes", "src/a.rs", 1, 0.5),
            semantic("semantic_only", "src/b.rs", 2, 0.5),
        ];
        let fused = fuse((result, semantic_results, Vec::new()), SCORE_AWARE);
        assert_eq!(fused.symbols[0].name, "both_lanes");
    }

    #[test]
    fn score_aware_mode_orders_exact_ties_by_stable_content_key() {
        // A genuine tie: two candidates, each the sole member of a
        // different lane at rank 1 with equal channel weight. Their
        // fused scores are bit-identical, so without the (file, name,
        // line) tiebreak the order would fall out of HashMap iteration
        // and differ between runs.
        let make = || {
            let result = RankedContextResult {
                query: NL_QUERY.to_owned(),
                count: 1,
                token_budget: 16384,
                chars_used: 128,
                symbols: vec![structural("structural_only", "src/z.rs", 1, 70)],
            };
            let semantic_results = vec![semantic("semantic_only", "src/a.rs", 2, 0.4)];
            (result, semantic_results, Vec::new())
        };
        for _ in 0..8 {
            let fused = fuse(make(), SCORE_AWARE);
            let order: Vec<&str> = fused.symbols.iter().map(|s| s.name.as_str()).collect();
            assert_eq!(order, vec!["semantic_only", "structural_only"]);
        }
    }

    #[test]
    fn flag_off_resolves_to_rank_only_whatever_the_cap_knob_says() {
        assert_eq!(
            RankFusionMode::resolve(false, true),
            RankFusionMode::RANK_ONLY
        );
        assert_eq!(
            RankFusionMode::resolve(false, false),
            RankFusionMode::RANK_ONLY
        );
    }

    #[test]
    fn policy_caps_only_lift_under_score_aware_mode() {
        let legacy = rank_fusion_policy(NL_QUERY, 8, 6);
        assert_eq!((legacy.semantic_limit, legacy.sparse_limit), (6, 4));

        // Flag off: caps unchanged even with the lift knob on.
        let off = resolve_rank_fusion_policy(NL_QUERY, 8, 6, RankFusionMode::resolve(false, true));
        assert_eq!((off.semantic_limit, off.sparse_limit), (6, 4));

        // Score-aware with the lift pinned off: still the legacy caps,
        // which is what isolates the scoring change during A/B runs.
        let pinned =
            resolve_rank_fusion_policy(NL_QUERY, 8, 6, RankFusionMode::resolve(true, false));
        assert_eq!((pinned.semantic_limit, pinned.sparse_limit), (6, 4));

        // Score-aware default: the full caller budget reaches fusion.
        let lifted =
            resolve_rank_fusion_policy(NL_QUERY, 8, 6, RankFusionMode::resolve(true, true));
        assert_eq!((lifted.semantic_limit, lifted.sparse_limit), (8, 6));
    }
}

#[cfg(test)]
mod adaptive_weight_tests {
    use super::{RrfChannelWeights, resolve_rrf_channel_weights};

    #[test]
    fn flag_off_always_returns_default_regardless_of_query_shape() {
        // Default (flag off) must never diverge from the shipped weights,
        // whatever the query classifier reports.
        assert_eq!(
            resolve_rrf_channel_weights(false, true, false),
            RrfChannelWeights::DEFAULT
        );
        assert_eq!(
            resolve_rrf_channel_weights(false, false, true),
            RrfChannelWeights::DEFAULT
        );
        assert_eq!(
            resolve_rrf_channel_weights(false, false, false),
            RrfChannelWeights::DEFAULT
        );
    }

    #[test]
    fn flag_on_exact_identifier_leans_sparse() {
        let w = resolve_rrf_channel_weights(true, true, false);
        assert!(w.w_sparse > w.w_sem);
        assert_eq!(w.w_sparse, 1.0);
        assert_eq!(w.w_sem, 0.6);
        // Structural / user lanes stay at their defaults.
        assert_eq!(w.w_struct, RrfChannelWeights::DEFAULT.w_struct);
        assert_eq!(w.w_user, RrfChannelWeights::DEFAULT.w_user);
    }

    #[test]
    fn flag_on_natural_language_leans_dense() {
        let w = resolve_rrf_channel_weights(true, false, true);
        assert!(w.w_sem > w.w_sparse);
        assert_eq!(w.w_sem, 1.2);
        assert_eq!(w.w_sparse, 0.6);
    }

    #[test]
    fn flag_on_ambiguous_shape_falls_back_to_default() {
        // Neither exact-identifier nor natural-language (e.g. a 2-word
        // short phrase): keep the default channel mix.
        assert_eq!(
            resolve_rrf_channel_weights(true, false, false),
            RrfChannelWeights::DEFAULT
        );
    }
}
