#[derive(Debug, Clone, Copy)]
pub(super) struct RankFusionPolicy {
    pub semantic_limit: usize,
    pub semantic_insertion_floor: f64,
    pub semantic_added_score_cap: i32,
    pub semantic_boosted_score_cap: i32,
    pub sparse_limit: usize,
    pub sparse_insertion_floor: i32,
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
            semantic_insertion_floor: 0.10,
            semantic_added_score_cap: 86,
            semantic_boosted_score_cap: 96,
            sparse_limit: max_sparse.min(4),
            sparse_insertion_floor: 28,
        };
    }
    if word_count >= 2 {
        return RankFusionPolicy {
            semantic_limit: max_semantic.min(2),
            semantic_insertion_floor: 0.18,
            semantic_added_score_cap: 82,
            semantic_boosted_score_cap: 92,
            sparse_limit: max_sparse.min(3),
            sparse_insertion_floor: 35,
        };
    }
    RankFusionPolicy {
        semantic_limit: max_semantic.min(3),
        semantic_insertion_floor: 0.12,
        semantic_added_score_cap: 80,
        semantic_boosted_score_cap: 90,
        sparse_limit: max_sparse.min(2),
        sparse_insertion_floor: 35,
    }
}
