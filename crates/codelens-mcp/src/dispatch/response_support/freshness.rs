/// Whether to attach the volatile `index_freshness` object to a read-hot
/// tool's payload. The full contract always attaches it (documented signal).
/// The lean contract suppresses ONLY the `fresh` bucket — its epoch/age fields
/// change every call and carry no actionable signal. All degraded buckets
/// (`recent`/`possibly_stale`/`stale`) are answer-affecting and attach
/// regardless of contract so a caller can detect a stale daemon (e.g. after a
/// silent file-watcher death) before the 1-hour refresh cliff.
pub(crate) fn should_attach_index_freshness(lean: bool, staleness_is_fresh: bool) -> bool {
    !lean || !staleness_is_fresh
}

#[cfg(test)]
mod tests {
    use super::should_attach_index_freshness;

    #[test]
    fn full_contract_always_attaches_freshness() {
        assert!(should_attach_index_freshness(false, true));
        assert!(should_attach_index_freshness(false, false));
    }

    #[test]
    fn lean_contract_suppresses_only_the_fresh_bucket() {
        // Fresh index under lean: volatile freshness object is suppressed.
        assert!(!should_attach_index_freshness(true, true));
        // Any degraded bucket (recent/possibly_stale/stale) under lean:
        // answer-affecting signal — must stay attached so a caller can
        // detect a stale daemon (e.g. silent watcher death) early.
        assert!(should_attach_index_freshness(true, false));
    }
}
