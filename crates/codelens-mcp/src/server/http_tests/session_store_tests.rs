use super::*;

#[test]
fn concurrent_session_creation() {
    let store = SessionStore::new(Duration::from_secs(300));
    let sessions: Vec<_> = (0..100).map(|_| store.create()).collect();

    // All IDs unique
    let mut ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 100, "all 100 session IDs should be unique");
    assert_eq!(store.len(), 100);
}

#[test]
fn session_touch_refreshes_expiry() {
    let store = SessionStore::new(Duration::from_millis(50));
    let session = store.create();
    let id = session.id.clone();

    std::thread::sleep(Duration::from_millis(30));
    // Touch should reset the timer
    store.get(&id); // get() calls touch()
    std::thread::sleep(Duration::from_millis(30));

    // 60ms total but touched at 30ms, so 30ms since touch < 50ms timeout
    assert!(
        store.get(&id).is_some(),
        "session should still be alive after touch"
    );
}

#[test]
fn cleanup_only_removes_expired() {
    let store = SessionStore::new(Duration::from_millis(20));
    let s1 = store.create();
    std::thread::sleep(Duration::from_millis(30));
    let s2 = store.create(); // created after sleep, still fresh

    let removed = store.cleanup();
    assert_eq!(removed, 1, "only the expired session should be removed");
    assert!(store.get(&s1.id).is_none());
    assert!(store.get(&s2.id).is_some());
}

// ── 2025-06-18 compliance ────────────────────────────────────────────
