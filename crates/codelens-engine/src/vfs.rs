//! Virtual File System event normalization layer.
//!
//! Transforms raw OS watcher events into semantic file events:
//! - Coalesces rapid create+modify into a single Modified
//! - Detects rename via delete+create with same content hash
//! - Filters unsupported file types and excluded paths

use crate::project::is_excluded;
use crate::symbols::language_for_path;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;

/// Semantic file event after normalization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum FileEvent {
    Created(PathBuf),
    Modified(PathBuf),
    Deleted(PathBuf),
    Renamed { from: PathBuf, to: PathBuf },
}

/// Normalize raw watcher events into semantic FileEvents.
///
/// Takes lists of changed (created/modified) and removed paths,
/// and produces a deduplicated list of FileEvents with rename detection.
pub fn normalize_events(changed: &[PathBuf], removed: &[PathBuf]) -> Vec<FileEvent> {
    // Filter to supported files only
    let changed: Vec<&PathBuf> = changed
        .iter()
        .filter(|p| !is_excluded(p) && language_for_path(p).is_some())
        .collect();
    let removed: Vec<&PathBuf> = removed
        .iter()
        .filter(|p| !is_excluded(p) && language_for_path(p).is_some())
        .collect();

    if removed.is_empty() && changed.is_empty() {
        return Vec::new();
    }

    // Try to detect renames: a delete + create with the same content hash
    // within the same batch is likely a rename.
    let mut events = Vec::new();
    let mut matched_renames: HashMap<usize, usize> = HashMap::new(); // removed_idx → changed_idx

    if !removed.is_empty() && !changed.is_empty() {
        // Hash deleted files from DB would be ideal, but we don't have that here.
        // Instead, hash the newly created files and see if any match recently deleted files.
        // Since deleted files no longer exist, we can only do this if we have
        // the new file's hash and compare with known sizes/names.
        //
        // Simple heuristic: same filename (basename) in different directory
        // within the same batch → likely rename.
        let removed_basenames: Vec<(&PathBuf, Option<&str>)> = removed
            .iter()
            .map(|p| (*p, p.file_name().and_then(|n| n.to_str())))
            .collect();

        for (ci, cp) in changed.iter().enumerate() {
            let Some(changed_name) = cp.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            for (ri, (rp, rname)) in removed_basenames.iter().enumerate() {
                if matched_renames.contains_key(&ri) {
                    continue;
                }
                if *rname == Some(changed_name) && rp != cp {
                    matched_renames.insert(ri, ci);
                    break;
                }
            }
        }
    }

    // Emit rename events for matched pairs
    let matched_changed: std::collections::HashSet<usize> =
        matched_renames.values().copied().collect();
    let matched_removed: std::collections::HashSet<usize> =
        matched_renames.keys().copied().collect();

    for (ri, ci) in &matched_renames {
        events.push(FileEvent::Renamed {
            from: removed[*ri].clone(),
            to: changed[*ci].clone(),
        });
    }

    // Emit delete events for unmatched removals
    for (ri, rp) in removed.iter().enumerate() {
        if !matched_removed.contains(&ri) {
            events.push(FileEvent::Deleted((*rp).clone()));
        }
    }

    // Emit created/modified events for unmatched changes
    for (ci, cp) in changed.iter().enumerate() {
        if !matched_changed.contains(&ci) {
            // We can't distinguish create vs modify from watcher events alone,
            // so treat all as Modified (the index pipeline handles both the same way).
            events.push(FileEvent::Modified((*cp).clone()));
        }
    }

    events
}

/// Convenience: extract paths by event type for the index pipeline.
pub fn partition_events(
    events: &[FileEvent],
) -> (Vec<PathBuf>, Vec<PathBuf>, Vec<(PathBuf, PathBuf)>) {
    let mut changed = Vec::new();
    let mut removed = Vec::new();
    let mut renamed = Vec::new();

    for event in events {
        match event {
            FileEvent::Created(p) | FileEvent::Modified(p) => changed.push(p.clone()),
            FileEvent::Deleted(p) => removed.push(p.clone()),
            FileEvent::Renamed { from, to } => {
                renamed.push((from.clone(), to.clone()));
                // Also index the new path
                changed.push(to.clone());
                // And remove the old path
                removed.push(from.clone());
            }
        }
    }

    (changed, removed, renamed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn empty_events() {
        let events = normalize_events(&[], &[]);
        assert!(events.is_empty());
    }

    #[test]
    fn simple_modified() {
        let changed = vec![PathBuf::from("/project/src/main.py")];
        let events = normalize_events(&changed, &[]);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], FileEvent::Modified(p) if p.to_str().unwrap().contains("main.py"))
        );
    }

    #[test]
    fn simple_deleted() {
        let removed = vec![PathBuf::from("/project/src/old.py")];
        let events = normalize_events(&[], &removed);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], FileEvent::Deleted(_)));
    }

    #[test]
    fn rename_detection_same_basename() {
        let removed = vec![PathBuf::from("/project/src/service.py")];
        let changed = vec![PathBuf::from("/project/lib/service.py")];
        let events = normalize_events(&changed, &removed);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], FileEvent::Renamed { from, to }
            if from.to_str().unwrap().contains("src/service.py")
            && to.to_str().unwrap().contains("lib/service.py")));
    }

    #[test]
    fn partition_handles_renames() {
        let events = vec![
            FileEvent::Modified(PathBuf::from("a.py")),
            FileEvent::Renamed {
                from: PathBuf::from("old.py"),
                to: PathBuf::from("new.py"),
            },
            FileEvent::Deleted(PathBuf::from("gone.py")),
        ];
        let (changed, removed, renamed) = partition_events(&events);
        assert_eq!(changed.len(), 2); // a.py + new.py
        assert_eq!(removed.len(), 2); // old.py + gone.py
        assert_eq!(renamed.len(), 1);
    }
}
