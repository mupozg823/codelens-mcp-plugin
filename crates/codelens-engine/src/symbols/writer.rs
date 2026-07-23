use super::SymbolIndex;
use super::parser::{flatten_symbols, parse_symbols};
use super::types::{AnalyzedFile, IndexStats, ParsedSymbol};
use super::{collect_candidate_files, file_modified_ms, language_for_path};
use crate::db::{self, IndexDb, NewCall, NewImport, NewSymbol, content_hash};
use crate::import_graph::{extract_imports_from_source, resolve_module_for_file};
use crate::project::ProjectRoot;
use anyhow::{Context, Result};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard, Weak};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct MutationGeneration(u64);

#[derive(Clone, Copy)]
enum PathObservation {
    Indexed(MutationGeneration),
    Tombstone(MutationGeneration),
}

impl PathObservation {
    const fn generation(self) -> MutationGeneration {
        match self {
            Self::Indexed(generation) | Self::Tombstone(generation) => generation,
        }
    }
}

#[derive(Default)]
struct MutationState {
    next_generation: u64,
    latest_observation: Option<MutationGeneration>,
    in_flight: BTreeSet<MutationGeneration>,
    paths: HashMap<String, PathObservation>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct AnalysisFingerprint {
    relative_path: String,
    mtime: i64,
    content_hash: String,
}

type SharedAnalysisResult = std::result::Result<Arc<PreparedEnsure>, Arc<str>>;

#[derive(Default)]
struct AnalysisFlight {
    result: Mutex<Option<SharedAnalysisResult>>,
    ready: Condvar,
}

#[derive(Default)]
struct AnalysisSingleflight {
    flights: Mutex<HashMap<AnalysisFingerprint, Arc<AnalysisFlight>>>,
    #[cfg(test)]
    probe: Mutex<Option<Arc<AnalysisProbe>>>,
}

#[cfg(test)]
#[derive(Default)]
struct AnalysisProbeState {
    leaders: usize,
    followers: usize,
    released: bool,
}

#[cfg(test)]
#[derive(Default)]
struct AnalysisProbe {
    state: Mutex<AnalysisProbeState>,
    changed: Condvar,
}

impl AnalysisSingleflight {
    fn run<F>(&self, fingerprint: AnalysisFingerprint, analyze: F) -> Result<Arc<PreparedEnsure>>
    where
        F: FnOnce() -> Result<PreparedEnsure>,
    {
        let (flight, is_leader) = {
            let mut flights = self
                .flights
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(flight) = flights.get(&fingerprint) {
                (Arc::clone(flight), false)
            } else {
                let flight = Arc::new(AnalysisFlight::default());
                flights.insert(fingerprint.clone(), Arc::clone(&flight));
                (flight, true)
            }
        };

        if !is_leader {
            #[cfg(test)]
            self.record_follower();
            let mut result = flight
                .result
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            while result.is_none() {
                result = flight
                    .ready
                    .wait(result)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
            return clone_shared_analysis_result(result.as_ref().expect("flight completed"));
        }

        #[cfg(test)]
        self.block_leader();
        let result = analyze()
            .map(Arc::new)
            .map_err(|error| Arc::<str>::from(format!("{error:#}")));
        {
            let mut slot = flight
                .result
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *slot = Some(result.clone());
        }
        flight.ready.notify_all();

        let mut flights = self
            .flights
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if flights
            .get(&fingerprint)
            .is_some_and(|current| Arc::ptr_eq(current, &flight))
        {
            flights.remove(&fingerprint);
        }
        clone_shared_analysis_result(&result)
    }

    #[cfg(test)]
    fn set_probe(&self, probe: Arc<AnalysisProbe>) {
        *self
            .probe
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(probe);
    }

    #[cfg(test)]
    fn block_leader(&self) {
        let probe = self
            .probe
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if let Some(probe) = probe {
            probe.block_leader();
        }
    }

    #[cfg(test)]
    fn record_follower(&self) {
        let probe = self
            .probe
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if let Some(probe) = probe {
            probe.record_follower();
        }
    }
}

fn clone_shared_analysis_result(result: &SharedAnalysisResult) -> Result<Arc<PreparedEnsure>> {
    result
        .clone()
        .map_err(|message| anyhow::anyhow!(message.to_string()))
}

#[cfg(test)]
impl AnalysisProbe {
    fn block_leader(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.leaders += 1;
        self.changed.notify_all();
        while !state.released {
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }

    fn record_follower(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.followers += 1;
        self.changed.notify_all();
    }

    fn wait_for_leaders(&self, expected: usize) {
        self.wait_for_count(expected, |state| state.leaders, "analysis leaders");
    }

    fn wait_for_followers(&self, expected: usize) {
        self.wait_for_count(expected, |state| state.followers, "analysis followers");
    }

    fn wait_for_count(
        &self,
        expected: usize,
        count: impl Fn(&AnalysisProbeState) -> usize,
        label: &str,
    ) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while count(&state) < expected {
            let (next, timeout) = self
                .changed
                .wait_timeout(state, std::time::Duration::from_secs(5))
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state = next;
            assert!(
                !timeout.timed_out() || count(&state) >= expected,
                "timed out waiting for {expected} {label}; observed {}",
                count(&state)
            );
        }
    }

    fn release(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.released = true;
        self.changed.notify_all();
    }

    fn leader_count(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .leaders
    }

    fn follower_count(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .followers
    }
}

/// Process-local newest-wins coordination for every writer connected to one index DB.
#[derive(Default)]
pub(super) struct MutationTracker {
    state: Mutex<MutationState>,
    committed_generation: AtomicU64,
    analysis: AnalysisSingleflight,
}

pub(super) struct MutationTicket {
    tracker: Arc<MutationTracker>,
    generation: MutationGeneration,
}

static PERSISTENT_MUTATION_TRACKERS: LazyLock<Mutex<HashMap<PathBuf, Weak<MutationTracker>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(super) fn persistent_mutation_tracker(db_path: &Path) -> Arc<MutationTracker> {
    let canonical_path = db_path
        .canonicalize()
        .unwrap_or_else(|_| db_path.to_path_buf());
    let mut registry = PERSISTENT_MUTATION_TRACKERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    registry.retain(|_, tracker| tracker.strong_count() > 0);
    if let Some(tracker) = registry.get(&canonical_path).and_then(Weak::upgrade) {
        return tracker;
    }

    let tracker = Arc::new(MutationTracker::default());
    registry.insert(canonical_path, Arc::downgrade(&tracker));
    tracker
}

impl MutationTracker {
    fn lock(&self) -> MutexGuard<'_, MutationState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn begin(self: &Arc<Self>) -> MutationTicket {
        let generation = {
            let mut state = self.lock();
            state.next_generation = state.next_generation.saturating_add(1);
            let generation = MutationGeneration(state.next_generation);
            state.in_flight.insert(generation);
            generation
        };
        MutationTicket {
            tracker: Arc::clone(self),
            generation,
        }
    }

    fn finish(&self, generation: MutationGeneration) {
        let mut state = self.lock();
        state.in_flight.remove(&generation);
        let oldest = state.in_flight.first().copied();
        match oldest {
            Some(oldest) => state
                .paths
                .retain(|_, observation| oldest < observation.generation()),
            None => state.paths.clear(),
        }
    }

    pub(super) fn committed_generation(&self) -> u64 {
        self.committed_generation.load(Ordering::Acquire)
    }

    fn record_commit(&self) {
        self.committed_generation.fetch_add(1, Ordering::AcqRel);
    }
}

impl MutationState {
    fn observe(&mut self, paths: &[String], observation: PathObservation) {
        let generation = observation.generation();
        for path in paths {
            let is_newest = self
                .paths
                .get(path)
                .is_none_or(|observation| observation.generation() <= generation);
            if is_newest {
                self.paths.insert(path.clone(), observation);
            }
        }
        self.latest_observation = Some(
            self.latest_observation
                .map_or(generation, |latest| latest.max(generation)),
        );
    }

    fn allows(&self, generation: MutationGeneration, path: &str) -> bool {
        self.paths
            .get(path)
            .is_none_or(|observation| observation.generation() <= generation)
    }

    fn has_newer_observation(&self, generation: MutationGeneration) -> bool {
        self.latest_observation
            .is_some_and(|latest| latest > generation)
    }

    fn has_older_in_flight(&self, generation: MutationGeneration) -> bool {
        self.in_flight
            .first()
            .is_some_and(|oldest| *oldest < generation)
    }

    fn newer_observation(
        &self,
        generation: MutationGeneration,
        path: &str,
    ) -> Option<PathObservation> {
        self.paths
            .get(path)
            .copied()
            .filter(|observation| observation.generation() > generation)
    }

    fn tombstone_absent_observations(
        &mut self,
        generation: MutationGeneration,
        snapshot: &HashSet<String>,
    ) {
        let absent: Vec<String> = self
            .paths
            .iter()
            .filter(|(path, observation)| {
                !snapshot.contains(*path) && observation.generation() < generation
            })
            .map(|(path, _)| path.clone())
            .collect();
        self.observe(&absent, PathObservation::Tombstone(generation));
    }
}

impl MutationTicket {
    fn observe_index_paths(&self, paths: &[String]) {
        self.tracker
            .lock()
            .observe(paths, PathObservation::Indexed(self.generation));
    }

    fn observe_tombstones(&self, paths: &[String]) {
        self.tracker
            .lock()
            .observe(paths, PathObservation::Tombstone(self.generation));
    }
}

impl Drop for MutationTicket {
    fn drop(&mut self) {
        self.tracker.finish(self.generation);
    }
}

fn should_bulk_rebuild_symbol_index(before: &IndexStats, candidate_count: usize) -> bool {
    let large_overhang = before.indexed_files > candidate_count.saturating_add(512);
    let stale_heavy = before.stale_files > candidate_count.saturating_div(2).max(256);
    // A discovery-set shrink (new exclude patterns, project reconfiguration)
    // leaves stale_files at 0 while indexed_files dwarfs the candidate set.
    // The per-file delete fallback then has to walk tens of thousands of
    // orphan rows in one transaction, which cannot finish inside an MCP
    // request window — rebuild wholesale instead.
    let discovery_shrink =
        before.indexed_files > candidate_count.saturating_mul(4).saturating_add(512);
    large_overhang && (stale_heavy || discovery_shrink)
}

/// Analyze a single file: read, hash, parse symbols/imports/calls.
/// Returns None if the file cannot be read or has no supported language.
#[cfg(test)]
fn analyze_file(project: &ProjectRoot, file: &Path) -> Option<AnalyzedFile> {
    let relative = project.to_relative(file);
    let content = fs::read(file).ok()?;
    let mtime = file_modified_ms(file).ok()? as i64;
    let hash = content_hash(&content);
    let source = String::from_utf8_lossy(&content);
    // Mirror `language_for_path`: extension-less well-known files
    // (Makefile, Dockerfile, Containerfile) key by lowercased file name.
    // The old `file.extension()?` dropped them here even though candidate
    // collection had already accepted them via `language_for_path`.
    let ext = match file.extension().and_then(|e| e.to_str()) {
        Some(e) => e.to_ascii_lowercase(),
        None => file.file_name()?.to_str()?.to_ascii_lowercase(),
    };

    let symbols = language_for_path(file)
        .and_then(|config| parse_symbols(&config, &relative, &source, false).ok())
        .unwrap_or_default();

    let raw_imports = extract_imports_from_source(file, &source);
    let imports: Vec<NewImport> = raw_imports
        .iter()
        .filter_map(|raw| {
            resolve_module_for_file(project, file, raw).map(|target| NewImport {
                target_path: target,
                raw_import: raw.clone(),
            })
        })
        .collect();

    let calls: Vec<NewCall> = crate::call_graph::extract_calls_from_source(file, &source)
        .into_iter()
        .map(|e| NewCall {
            caller_name: e.caller_name,
            callee_name: e.callee_name,
            line: e.line as i64,
        })
        .collect();

    Some(AnalyzedFile {
        relative_path: relative,
        mtime,
        content_hash: hash,
        size_bytes: content.len() as i64,
        language_ext: ext,
        symbols,
        imports,
        calls,
    })
}

struct EnsureSource<'a> {
    file: &'a Path,
    relative: &'a str,
    mtime: i64,
    content: Vec<u8>,
    content_hash: Option<String>,
}

struct PreparedEnsure {
    relative_path: String,
    mtime: i64,
    content_hash: String,
    size_bytes: i64,
    language_ext: Option<String>,
    symbols: Vec<ParsedSymbol>,
    flat_symbols: Vec<ParsedSymbol>,
    imports: Vec<NewImport>,
    calls: Vec<NewCall>,
}

impl PreparedEnsure {
    fn as_analyzed_file(&self) -> Option<AnalyzedFile> {
        Some(AnalyzedFile {
            relative_path: self.relative_path.clone(),
            mtime: self.mtime,
            content_hash: self.content_hash.clone(),
            size_bytes: self.size_bytes,
            language_ext: self.language_ext.clone()?,
            symbols: self.symbols.clone(),
            imports: self.imports.clone(),
            calls: self.calls.clone(),
        })
    }
}

fn analyze_ensure(project: &ProjectRoot, source: EnsureSource<'_>) -> Result<PreparedEnsure> {
    let hash = source
        .content_hash
        .unwrap_or_else(|| content_hash(&source.content));
    let text = String::from_utf8_lossy(&source.content);
    let symbols = if let Some(config) = language_for_path(source.file) {
        parse_symbols(&config, source.relative, &text, false)?
    } else {
        Vec::new()
    };
    let language_ext = source
        .file
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .or_else(|| {
            language_for_path(source.file)?;
            source
                .file
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_ascii_lowercase)
        });
    let raw_imports = extract_imports_from_source(source.file, &text);
    let imports = raw_imports
        .iter()
        .filter_map(|raw| {
            resolve_module_for_file(project, source.file, raw).map(|target| NewImport {
                target_path: target,
                raw_import: raw.clone(),
            })
        })
        .collect();
    let calls = crate::call_graph::extract_calls_from_source(source.file, &text)
        .into_iter()
        .map(|edge| NewCall {
            caller_name: edge.caller_name,
            callee_name: edge.callee_name,
            line: edge.line as i64,
        })
        .collect();
    let flat_symbols = flatten_symbols(symbols.clone());

    Ok(PreparedEnsure {
        relative_path: source.relative.to_owned(),
        mtime: source.mtime,
        content_hash: hash,
        size_bytes: source.content.len() as i64,
        language_ext,
        symbols,
        flat_symbols,
        imports,
        calls,
    })
}

fn indexed_symbols(db: &IndexDb, file_id: i64, relative: &str) -> Result<Vec<ParsedSymbol>> {
    Ok(db
        .get_file_symbols(file_id)?
        .into_iter()
        .map(|row| ParsedSymbol {
            name: row.name,
            kind: super::types::SymbolKind::from_str_label(&row.kind),
            file_path: relative.to_owned(),
            line: row.line as usize,
            column: row.column_num as usize,
            start_byte: row.start_byte as u32,
            end_byte: row.end_byte as u32,
            signature: row.signature,
            body: None,
            name_path: row.name_path,
            children: Vec::new(),
        })
        .collect())
}

/// Commit an AnalyzedFile to the DB within an existing connection/transaction.
/// Skips if the file is already fresh (same hash+mtime).
/// Returns true if the file was actually written.
fn commit_analyzed(conn: &rusqlite::Connection, analyzed: &AnalyzedFile) -> Result<bool> {
    if db::get_fresh_file(
        conn,
        &analyzed.relative_path,
        analyzed.mtime,
        &analyzed.content_hash,
    )?
    .is_some()
    {
        return Ok(false);
    }

    let file_id = db::upsert_file(
        conn,
        &analyzed.relative_path,
        analyzed.mtime,
        &analyzed.content_hash,
        analyzed.size_bytes,
        Some(&analyzed.language_ext),
    )?;

    let flat = flatten_symbols(analyzed.symbols.clone());
    let new_syms: Vec<NewSymbol<'_>> = flat
        .iter()
        .map(|s| NewSymbol {
            name: &s.name,
            kind: s.kind.as_label(),
            line: s.line as i64,
            column_num: s.column as i64,
            start_byte: s.start_byte as i64,
            end_byte: s.end_byte as i64,
            signature: &s.signature,
            name_path: &s.name_path,
            parent_id: None,
        })
        .collect();
    db::insert_symbols(conn, file_id, &new_syms)?;

    if !analyzed.imports.is_empty() {
        db::insert_imports(conn, file_id, &analyzed.imports)?;
    }
    if !analyzed.calls.is_empty() {
        db::insert_calls(conn, file_id, &analyzed.calls)?;
    }

    Ok(true)
}

impl SymbolIndex {
    fn analyze_singleflight(&self, mut source: EnsureSource<'_>) -> Result<Arc<PreparedEnsure>> {
        let hash = source
            .content_hash
            .clone()
            .unwrap_or_else(|| content_hash(&source.content));
        let fingerprint = AnalysisFingerprint {
            relative_path: source.relative.to_owned(),
            mtime: source.mtime,
            content_hash: hash.clone(),
        };
        source.content_hash = Some(hash);
        self.mutations
            .analysis
            .run(fingerprint, || analyze_ensure(&self.project, source))
    }

    fn analyze_file_singleflight(&self, file: &Path) -> Option<AnalyzedFile> {
        let relative = self.project.to_relative(file);
        let content = fs::read(file).ok()?;
        let mtime = file_modified_ms(file).ok()? as i64;
        let hash = content_hash(&content);
        self.analyze_singleflight(EnsureSource {
            file,
            relative: &relative,
            mtime,
            content,
            content_hash: Some(hash),
        })
        .ok()?
        .as_analyzed_file()
    }

    /// One-time migration from legacy symbols-v1.json to SQLite.
    pub(super) fn migrate_from_json(&mut self) -> Result<()> {
        let json_path = self
            .project
            .as_path()
            .join(".codelens/index/symbols-v1.json");
        if !json_path.is_file() {
            return Ok(());
        }
        let stats = self.refresh_all()?;
        if stats.indexed_files > 0 || stats.stale_files == 0 {
            let _ = fs::remove_file(&json_path);
        } else {
            tracing::warn!(
                path = %json_path.display(),
                "migration from JSON produced 0 indexed files, keeping legacy file"
            );
        }
        Ok(())
    }

    fn commit_index_batch(&self, ticket: &MutationTicket, analyzed: &[AnalyzedFile]) -> Result<()> {
        let mutations = self.mutations.lock();
        let mut writer = self.writer();
        let did_write = writer.with_transaction(|conn| {
            let mut did_write = false;
            for file in analyzed {
                if mutations.allows(ticket.generation, &file.relative_path) {
                    did_write |= commit_analyzed(conn, file)?;
                }
            }
            Ok(did_write)
        })?;
        if did_write {
            self.mutations.record_commit();
        }
        Ok(())
    }

    fn commit_ensure_analysis(
        &self,
        ticket: &MutationTicket,
        prepared: &PreparedEnsure,
    ) -> Result<Vec<ParsedSymbol>> {
        let mutations = self.mutations.lock();
        let mut writer = self.writer();
        match mutations.newer_observation(ticket.generation, &prepared.relative_path) {
            Some(PathObservation::Tombstone(_)) => Ok(Vec::new()),
            Some(PathObservation::Indexed(_)) => {
                let Some(file_row) = writer.get_file(&prepared.relative_path)? else {
                    return Ok(Vec::new());
                };
                indexed_symbols(&writer, file_row.id, &prepared.relative_path)
            }
            None => {
                writer.with_transaction(|conn| {
                    let file_id = db::upsert_file(
                        conn,
                        &prepared.relative_path,
                        prepared.mtime,
                        &prepared.content_hash,
                        prepared.size_bytes,
                        prepared.language_ext.as_deref(),
                    )?;
                    let new_symbols: Vec<NewSymbol<'_>> = prepared
                        .flat_symbols
                        .iter()
                        .map(|symbol| NewSymbol {
                            name: &symbol.name,
                            kind: symbol.kind.as_label(),
                            line: symbol.line as i64,
                            column_num: symbol.column as i64,
                            start_byte: symbol.start_byte as i64,
                            end_byte: symbol.end_byte as i64,
                            signature: &symbol.signature,
                            name_path: &symbol.name_path,
                            parent_id: None,
                        })
                        .collect();
                    db::insert_symbols(conn, file_id, &new_symbols)?;
                    if !prepared.imports.is_empty() {
                        db::insert_imports(conn, file_id, &prepared.imports)?;
                    }
                    if !prepared.calls.is_empty() {
                        db::insert_calls(conn, file_id, &prepared.calls)?;
                    }
                    Ok(())
                })?;
                self.mutations.record_commit();
                Ok(prepared.symbols.clone())
            }
        }
    }

    fn commit_refresh_snapshot(
        &self,
        ticket: &MutationTicket,
        analyzed: &[AnalyzedFile],
        snapshot: &HashSet<String>,
        bulk_rebuild_requested: bool,
    ) -> Result<()> {
        let mut mutations = self.mutations.lock();
        let analysis_complete = analyzed.len() == snapshot.len();
        let bulk_rebuild = bulk_rebuild_requested
            && analysis_complete
            && !mutations.has_newer_observation(ticket.generation);
        mutations.tombstone_absent_observations(ticket.generation, snapshot);

        let mut writer = self.writer();
        let did_write = writer.with_transaction(|conn| {
            let mut did_write = false;
            let indexed_paths = db::all_file_paths(conn)?;
            for indexed_path in &indexed_paths {
                if !snapshot.contains(indexed_path)
                    && mutations.allows(ticket.generation, indexed_path)
                {
                    mutations.observe(
                        std::slice::from_ref(indexed_path),
                        PathObservation::Tombstone(ticket.generation),
                    );
                }
            }

            if bulk_rebuild {
                db::clear_symbol_index(conn)?;
                did_write |= !indexed_paths.is_empty();
            }

            for file in analyzed {
                if mutations.allows(ticket.generation, &file.relative_path) {
                    did_write |= commit_analyzed(conn, file)?;
                }
            }

            if !bulk_rebuild {
                for indexed_path in indexed_paths {
                    if !snapshot.contains(&indexed_path)
                        && mutations.allows(ticket.generation, &indexed_path)
                    {
                        db::delete_file(conn, &indexed_path)?;
                        did_write = true;
                    }
                }
            }

            Ok(did_write)
        })?;
        if did_write {
            self.mutations.record_commit();
        }
        Ok(())
    }

    pub fn refresh_all(&self) -> Result<IndexStats> {
        use rayon::prelude::*;

        let ticket = self.mutations.begin();
        let mut files = collect_candidate_files(self.project.as_path())?;
        let snapshot_paths: Vec<String> = files
            .iter()
            .map(|file| self.project.to_relative(file))
            .collect();
        ticket.observe_index_paths(&snapshot_paths);
        let snapshot: HashSet<String> = snapshot_paths.into_iter().collect();
        let before_stats = self.stats().ok();
        let bulk_rebuild = before_stats
            .as_ref()
            .is_some_and(|before| should_bulk_rebuild_symbol_index(before, files.len()));
        files.sort_by(|a, b| {
            let sa = a.metadata().map(|m| m.len()).unwrap_or(0);
            let sb = b.metadata().map(|m| m.len()).unwrap_or(0);
            sb.cmp(&sa)
        });

        // Phase 1: parallel analysis (CPU-bound, no DB access)
        let analyzed: Vec<AnalyzedFile> = files
            .par_iter()
            .filter_map(|file| self.analyze_file_singleflight(file))
            .collect();

        // Phase 2: sequential newest-wins DB commit.
        self.commit_refresh_snapshot(&ticket, &analyzed, &snapshot, bulk_rebuild)?;
        if let Err(error) = self.checkpoint_wal_passive() {
            tracing::debug!(%error, "symbol index WAL checkpoint skipped after refresh");
        }
        self.stats()
    }

    /// Incrementally re-index only the given files (changed/created).
    pub fn index_files(&self, paths: &[PathBuf]) -> Result<usize> {
        use rayon::prelude::*;

        let ticket = self.mutations.begin();
        let relative_paths: Vec<String> = paths
            .iter()
            .map(|path| self.project.to_relative(path))
            .collect();
        ticket.observe_index_paths(&relative_paths);
        let analyzed: Vec<AnalyzedFile> = paths
            .par_iter()
            .filter(|f| f.is_file())
            .filter_map(|file| self.analyze_file_singleflight(file))
            .collect();

        let count = analyzed.len();
        if count == 0 {
            return Ok(0);
        }

        self.commit_index_batch(&ticket, &analyzed)?;
        Ok(count)
    }

    /// Re-index a single file by relative path (convenience for post-mutation refresh).
    pub fn refresh_file(&self, relative_path: &str) -> Result<usize> {
        let abs = self.project.as_path().join(relative_path);
        self.index_files(&[abs])
    }

    /// Remove deleted files from the index.
    pub fn remove_files(&self, paths: &[PathBuf]) -> Result<usize> {
        let ticket = self.mutations.begin();
        let count = paths.len();
        let relatives: Vec<String> = paths.iter().map(|p| self.project.to_relative(p)).collect();
        ticket.observe_tombstones(&relatives);
        let mutations = self.mutations.lock();
        let mut writer = self.writer();
        let mut existing = HashSet::new();
        for relative in &relatives {
            if mutations.allows(ticket.generation, relative) && writer.get_file(relative)?.is_some()
            {
                existing.insert(relative.clone());
            }
        }
        if existing.is_empty() {
            return Ok(count);
        }
        writer.with_transaction(|conn| {
            for relative in &existing {
                db::delete_file(conn, relative)?;
            }
            Ok(())
        })?;
        self.mutations.record_commit();
        Ok(count)
    }

    /// Ensure a file is indexed; returns parsed symbols for immediate use.
    /// Fast path: if mtime unchanged, reads symbols from DB (no re-parse).
    pub(super) fn ensure_indexed(&self, file: &Path, relative: &str) -> Result<Vec<ParsedSymbol>> {
        let ticket = self.mutations.begin();
        ticket.observe_index_paths(&[relative.to_owned()]);
        let mtime = file_modified_ms(file)? as i64;
        let overlap_content = if self.mutations.lock().has_older_in_flight(ticket.generation) {
            Some(fs::read(file).with_context(|| format!("failed to read {}", file.display()))?)
        } else {
            None
        };
        let overlap_hash = overlap_content.as_deref().map(content_hash);

        // Fast path: mtime unchanged → read symbols from DB instead of re-parsing
        {
            let mutations = self.mutations.lock();
            let db = self.writer();
            if mutations.allows(ticket.generation, relative)
                && let Some(file_row) = db.get_fresh_file_by_mtime(relative, mtime)?
                && overlap_hash
                    .as_ref()
                    .is_none_or(|hash| file_row.content_hash == *hash)
            {
                return indexed_symbols(&db, file_row.id, relative);
            }
        }

        // Slow path: analyze without either commit lock.
        let content = match overlap_content {
            Some(content) => content,
            None => fs::read(file).with_context(|| format!("failed to read {}", file.display()))?,
        };
        let prepared = self.analyze_singleflight(EnsureSource {
            file,
            relative,
            mtime,
            content,
            content_hash: overlap_hash,
        })?;
        self.commit_ensure_analysis(&ticket, &prepared)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn bulk_rebuild_triggers_for_large_stale_overhang() {
        let before = IndexStats {
            indexed_files: 3_854,
            supported_files: 2_021,
            stale_files: 3_147,
        };

        assert!(should_bulk_rebuild_symbol_index(&before, 1_978));
    }

    #[test]
    fn bulk_rebuild_does_not_trigger_for_normal_stale_refresh() {
        let before = IndexStats {
            indexed_files: 1_978,
            supported_files: 1_978,
            stale_files: 40,
        };

        assert!(!should_bulk_rebuild_symbol_index(&before, 1_978));
    }

    #[test]
    fn bulk_rebuild_triggers_for_discovery_shrink_with_zero_stale() {
        // New exclude patterns shrink discovery from 24,965 to 724 files while
        // stale_files stays 0 — the per-file delete fallback would walk 24k
        // orphan rows in one transaction and time out.
        let before = IndexStats {
            indexed_files: 24_965,
            supported_files: 24_965,
            stale_files: 0,
        };

        assert!(should_bulk_rebuild_symbol_index(&before, 724));
    }

    #[test]
    fn bulk_rebuild_does_not_trigger_for_mild_shrink_with_zero_stale() {
        // Deleting a subdirectory is a mild shrink; the per-file delete
        // fallback handles it fine and preserves incremental freshness.
        let before = IndexStats {
            indexed_files: 2_400,
            supported_files: 2_400,
            stale_files: 0,
        };

        assert!(!should_bulk_rebuild_symbol_index(&before, 2_000));
    }

    fn race_project() -> (PathBuf, ProjectRoot) {
        let root = std::env::temp_dir().join(format!(
            "codelens-symbol-generation-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        fs::create_dir_all(root.join("src")).expect("create fixture source directory");
        fs::write(root.join(".git"), "gitdir: fixture\n").expect("write root marker");
        let project = ProjectRoot::new_exact(&root).expect("fixture project");
        (project.as_path().to_path_buf(), project)
    }

    #[test]
    fn overlapping_entry_paths_and_persistent_indexes_share_one_fingerprint_analysis() {
        // Given: two persistent indexes share one database and one unchanged source fingerprint.
        let (root, project) = race_project();
        let path = root.join("src/singleflight.rs");
        let relative = "src/singleflight.rs";
        fs::write(&path, "pub fn shared_analysis() {}\n").expect("write source");
        let first = Arc::new(SymbolIndex::new(project.clone()).expect("open first index"));
        let second = Arc::new(SymbolIndex::new(project).expect("open second index"));
        let probe = Arc::new(AnalysisProbe::default());
        first.mutations.analysis.set_probe(Arc::clone(&probe));

        // When: refresh analysis is blocked and slow ensure overlaps through the other index.
        let refresh_index = Arc::clone(&first);
        let refresh = std::thread::spawn(move || refresh_index.refresh_all());
        probe.wait_for_leaders(1);
        let ensure_index = Arc::clone(&second);
        let ensure_path = path.clone();
        let ensure =
            std::thread::spawn(move || ensure_index.ensure_indexed(&ensure_path, relative));
        probe.wait_for_followers(1);
        probe.release();
        refresh
            .join()
            .expect("join refresh")
            .expect("refresh index");
        let symbols = ensure.join().expect("join ensure").expect("ensure index");

        // Then: both entry paths reuse one analysis owned by the shared persistent tracker.
        assert!(Arc::ptr_eq(&first.mutations, &second.mutations));
        assert_eq!(probe.leader_count(), 1);
        assert_eq!(probe.follower_count(), 1);
        assert!(
            symbols
                .iter()
                .any(|symbol| symbol.name == "shared_analysis")
        );
    }

    #[test]
    fn different_fingerprints_analyze_without_global_serialization() {
        // Given: two different source fingerprints share one index-level singleflight registry.
        let (root, project) = race_project();
        let first_path = root.join("src/first_fingerprint.rs");
        let second_path = root.join("src/second_fingerprint.rs");
        fs::write(&first_path, "pub fn first_fingerprint() {}\n").expect("write first");
        fs::write(&second_path, "pub fn second_fingerprint() {}\n").expect("write second");
        let index = Arc::new(SymbolIndex::new_memory(project));
        let probe = Arc::new(AnalysisProbe::default());
        index.mutations.analysis.set_probe(Arc::clone(&probe));

        // When: both analyses enter before either leader is released.
        let first_index = Arc::clone(&index);
        let first = std::thread::spawn(move || {
            first_index.ensure_indexed(&first_path, "src/first_fingerprint.rs")
        });
        probe.wait_for_leaders(1);
        let second_index = Arc::clone(&index);
        let second = std::thread::spawn(move || {
            second_index.ensure_indexed(&second_path, "src/second_fingerprint.rs")
        });
        probe.wait_for_leaders(2);

        // Then: distinct keys have independent leaders instead of one global analysis lock.
        assert_eq!(probe.leader_count(), 2);
        assert_eq!(probe.follower_count(), 0);
        probe.release();
        assert!(
            first
                .join()
                .expect("join first")
                .expect("index first")
                .iter()
                .any(|symbol| symbol.name == "first_fingerprint")
        );
        assert!(
            second
                .join()
                .expect("join second")
                .expect("index second")
                .iter()
                .any(|symbol| symbol.name == "second_fingerprint")
        );
    }

    #[test]
    fn content_change_uses_two_analyses_and_newest_ticket_wins() {
        // Given: an old content fingerprint is blocked after receiving the older ticket.
        let (root, project) = race_project();
        let path = root.join("src/content_race.rs");
        fs::write(&path, "pub fn old_fingerprint() {}\n").expect("write old source");
        let index = Arc::new(SymbolIndex::new_memory(project));
        let probe = Arc::new(AnalysisProbe::default());
        index.mutations.analysis.set_probe(Arc::clone(&probe));
        let old_index = Arc::clone(&index);
        let old_path = path.clone();
        let old =
            std::thread::spawn(move || old_index.ensure_indexed(&old_path, "src/content_race.rs"));
        probe.wait_for_leaders(1);

        // When: the content changes and a newer ticket analyzes the new fingerprint concurrently.
        fs::write(&path, "pub fn new_fingerprint() {}\n").expect("write new source");
        let new_index = Arc::clone(&index);
        let new_path = path.clone();
        let new = std::thread::spawn(move || new_index.index_files(&[new_path]));
        probe.wait_for_leaders(2);
        probe.release();
        old.join().expect("join old").expect("index old");
        assert_eq!(new.join().expect("join new").expect("index new"), 1);

        // Then: both content versions were analyzed, while CAS rejects the stale result.
        assert_eq!(probe.leader_count(), 2);
        assert_eq!(
            index
                .find_symbol_cached("new_fingerprint", None, false, false, 10)
                .expect("find new symbol")
                .len(),
            1
        );
        assert!(
            index
                .find_symbol_cached("old_fingerprint", None, false, false, 10)
                .expect("find old symbol")
                .is_empty()
        );
    }

    fn pending_analysis(index: &SymbolIndex, path: &Path) -> (MutationTicket, AnalyzedFile) {
        let ticket = index.mutations.begin();
        ticket.observe_index_paths(&[index.project.to_relative(path)]);
        let analyzed = analyze_file(&index.project, path).expect("analyze fixture file");
        (ticket, analyzed)
    }

    fn pending_ensure_analysis(
        index: &SymbolIndex,
        path: &Path,
        relative: &str,
    ) -> (MutationTicket, PreparedEnsure) {
        let ticket = index.mutations.begin();
        ticket.observe_index_paths(&[relative.to_owned()]);
        let source = EnsureSource {
            file: path,
            relative,
            mtime: file_modified_ms(path).expect("pending ensure mtime") as i64,
            content: fs::read(path).expect("pending ensure source"),
            content_hash: None,
        };
        let analyzed = analyze_ensure(&index.project, source).expect("analyze pending ensure");
        (ticket, analyzed)
    }

    #[test]
    fn newer_index_commit_wins_over_older_analysis() {
        // Given: an old analysis remains in flight for the same path.
        let (root, project) = race_project();
        let path = root.join("src/race.rs");
        fs::write(&path, "pub fn old_symbol() {}\n").expect("write old source");
        let index = SymbolIndex::new_memory(project);
        let (old_ticket, old_analysis) = pending_analysis(&index, &path);

        // When: a newer index operation commits before the old analysis.
        fs::write(&path, "pub fn new_symbol() {}\n").expect("write new source");
        index
            .index_files(std::slice::from_ref(&path))
            .expect("index new source");
        index
            .commit_index_batch(&old_ticket, std::slice::from_ref(&old_analysis))
            .expect("attempt stale commit");

        // Then: only the newer symbol remains observable.
        assert!(
            index
                .find_symbol_cached("old_symbol", None, false, false, 10)
                .expect("find old symbol")
                .is_empty()
        );
        assert_eq!(
            index
                .find_symbol_cached("new_symbol", None, false, false, 10)
                .expect("find new symbol")
                .len(),
            1
        );
    }

    #[test]
    fn slow_ensure_indexed_commit_wins_over_older_analysis() {
        // Given: an old analysis is pending while the index has no DB row for the file.
        let (root, project) = race_project();
        let path = root.join("src/ensure.rs");
        fs::write(&path, "pub fn old_ensure() {}\n").expect("write old source");
        let index = SymbolIndex::new_memory(project);
        let (old_ticket, old_analysis) = pending_analysis(&index, &path);

        // When: ensure_indexed takes its slow parse-and-commit path with newer content.
        fs::write(&path, "pub fn new_ensure() {}\n").expect("write new source");
        index
            .ensure_indexed(&path, "src/ensure.rs")
            .expect("ensure newer source");
        index
            .commit_index_batch(&old_ticket, std::slice::from_ref(&old_analysis))
            .expect("attempt stale commit");

        // Then: the slow ensure commit remains authoritative.
        assert_eq!(
            index
                .find_symbol_cached("new_ensure", None, false, false, 10)
                .expect("find ensured symbol")
                .len(),
            1
        );
        assert!(
            index
                .find_symbol_cached("old_ensure", None, false, false, 10)
                .expect("find stale ensure symbol")
                .is_empty()
        );
    }

    #[test]
    fn ensure_indexed_hashes_same_mtime_content_during_overlap() {
        // Given: the DB row is fresh by mtime, but an older mutation overlaps a rewrite.
        let (root, project) = race_project();
        let path = root.join("src/same_mtime.rs");
        fs::write(&path, "pub fn old_fast() {}\n").expect("write original source");
        let index = SymbolIndex::new_memory(project);
        index
            .index_files(std::slice::from_ref(&path))
            .expect("seed original source");
        let original_modified = fs::metadata(&path)
            .expect("original metadata")
            .modified()
            .expect("original mtime");
        let original_mtime_ms = file_modified_ms(&path).expect("original mtime ms");
        let older_ticket = index.mutations.begin();
        older_ticket.observe_index_paths(&[index.project.to_relative(&path)]);

        // When: content changes but the filesystem mtime is restored exactly.
        fs::write(&path, "pub fn new_fast() {}\n").expect("rewrite source");
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open rewritten source")
            .set_modified(original_modified)
            .expect("restore original mtime");
        assert_eq!(
            file_modified_ms(&path).expect("rewritten mtime ms"),
            original_mtime_ms,
            "fixture must exercise the mtime-only fast path"
        );
        let symbols = index
            .ensure_indexed(&path, "src/same_mtime.rs")
            .expect("ensure rewritten source");

        // Then: overlap hashing bypasses the stale mtime-only DB row.
        assert!(symbols.iter().any(|symbol| symbol.name == "new_fast"));
        assert!(symbols.iter().all(|symbol| symbol.name != "old_fast"));
    }

    #[test]
    fn slow_ensure_cas_loss_returns_newer_indexed_symbols() {
        // Given: slow ensure has parsed an older version but has not committed it.
        let (root, project) = race_project();
        let path = root.join("src/ensure_winner.rs");
        let relative = "src/ensure_winner.rs";
        fs::write(&path, "pub fn stale_result() {}\n").expect("write stale source");
        let index = SymbolIndex::new_memory(project);
        let (old_ticket, old_analysis) = pending_ensure_analysis(&index, &path, relative);

        // When: a newer index write wins before slow ensure reaches its CAS commit.
        fs::write(&path, "pub fn winning_row() {}\n").expect("write winning source");
        index
            .index_files(std::slice::from_ref(&path))
            .expect("commit winning source");
        let returned = index
            .commit_ensure_analysis(&old_ticket, &old_analysis)
            .expect("resolve losing ensure result");

        // Then: ensure returns the winning DB row, never its stale parsed symbols.
        assert!(returned.iter().any(|symbol| symbol.name == "winning_row"));
        assert!(returned.iter().all(|symbol| symbol.name != "stale_result"));
    }

    #[test]
    fn slow_ensure_cas_loss_returns_empty_for_newer_tombstone() {
        // Given: slow ensure has parsed a file before deletion.
        let (root, project) = race_project();
        let path = root.join("src/ensure_deleted.rs");
        let relative = "src/ensure_deleted.rs";
        fs::write(&path, "pub fn stale_deleted() {}\n").expect("write stale source");
        let index = SymbolIndex::new_memory(project);
        let (old_ticket, old_analysis) = pending_ensure_analysis(&index, &path, relative);

        // When: a newer deletion tombstone wins before slow ensure commits.
        fs::remove_file(&path).expect("delete source");
        index
            .remove_files(std::slice::from_ref(&path))
            .expect("commit winning tombstone");
        let returned = index
            .commit_ensure_analysis(&old_ticket, &old_analysis)
            .expect("resolve tombstoned ensure result");

        // Then: ensure reports no symbols for the winning tombstone.
        assert!(returned.is_empty());
    }

    #[test]
    fn deletion_tombstone_blocks_stale_resurrection() {
        // Given: a file was analyzed before its deletion was observed.
        let (root, project) = race_project();
        let path = root.join("src/deleted.rs");
        fs::write(&path, "pub fn deleted_symbol() {}\n").expect("write source");
        let index = SymbolIndex::new_memory(project);
        let (old_ticket, old_analysis) = pending_analysis(&index, &path);

        // When: deletion commits before the old analysis tries to commit.
        fs::remove_file(&path).expect("delete source");
        index
            .remove_files(std::slice::from_ref(&path))
            .expect("record deletion");
        index
            .commit_index_batch(&old_ticket, std::slice::from_ref(&old_analysis))
            .expect("attempt stale resurrection");

        // Then: the tombstoned path is absent from the index.
        let indexed_paths = index.writer().all_file_paths().expect("indexed paths");
        assert!(
            indexed_paths.is_empty(),
            "tombstone must reject stale paths, got {indexed_paths:?}"
        );
    }

    #[test]
    fn full_refresh_snapshot_preserves_file_indexed_by_newer_ticket() {
        // Given: a full refresh captured a snapshot before a new file existed.
        let (root, project) = race_project();
        let original = root.join("src/original.rs");
        fs::write(&original, "pub fn original_symbol() {}\n").expect("write original");
        let index = SymbolIndex::new_memory(project);
        let refresh_ticket = index.mutations.begin();
        let snapshot = HashSet::from([index.project.to_relative(&original)]);
        refresh_ticket.observe_index_paths(&snapshot.iter().cloned().collect::<Vec<_>>());
        let analyzed = vec![analyze_file(&index.project, &original).expect("analyze original")];

        // When: a newer ticket indexes a file, then the old refresh requests bulk rebuild.
        let newer = root.join("src/newer.rs");
        fs::write(&newer, "pub fn newer_symbol() {}\n").expect("write newer source");
        index
            .index_files(std::slice::from_ref(&newer))
            .expect("index newer source");
        index
            .commit_refresh_snapshot(&refresh_ticket, &analyzed, &snapshot, true)
            .expect("commit old refresh");

        // Then: fallback commit preserves the file outside the old snapshot.
        assert_eq!(
            index
                .find_symbol_cached("newer_symbol", None, false, false, 10)
                .expect("find newer symbol")
                .len(),
            1
        );
    }

    #[test]
    fn incomplete_refresh_analysis_disables_bulk_clear() {
        // Given: two indexed files are in the refresh snapshot.
        let (root, project) = race_project();
        let first = root.join("src/first.rs");
        let second = root.join("src/second.rs");
        fs::write(&first, "pub fn first_symbol() {}\n").expect("write first source");
        fs::write(&second, "pub fn second_symbol() {}\n").expect("write second source");
        let index = SymbolIndex::new_memory(project);
        index
            .index_files(&[first.clone(), second.clone()])
            .expect("seed index");
        let refresh_ticket = index.mutations.begin();
        let snapshot = HashSet::from([
            index.project.to_relative(&first),
            index.project.to_relative(&second),
        ]);
        refresh_ticket.observe_index_paths(&snapshot.iter().cloned().collect::<Vec<_>>());
        let analyzed = vec![analyze_file(&index.project, &first).expect("analyze first")];

        // When: the incomplete refresh requests a bulk rebuild.
        index
            .commit_refresh_snapshot(&refresh_ticket, &analyzed, &snapshot, true)
            .expect("commit incomplete refresh");

        // Then: the unanalyzed file's existing symbols survive.
        assert_eq!(
            index
                .find_symbol_cached("second_symbol", None, false, false, 10)
                .expect("find second symbol")
                .len(),
            1
        );
    }

    #[test]
    fn persistent_indexes_share_mutation_tracker_for_canonical_db_path() {
        // Given: two persistent index instances open the same project database.
        let (root, project) = race_project();
        let path = root.join("src/shared.rs");
        fs::write(&path, "pub fn old_shared() {}\n").expect("write old source");
        let first = SymbolIndex::new(project.clone()).expect("open first index");
        let second = SymbolIndex::new(project).expect("open second index");
        let (old_ticket, old_analysis) = pending_analysis(&first, &path);

        // When: the second instance commits a newer observation first.
        fs::write(&path, "pub fn new_shared() {}\n").expect("write new source");
        second
            .index_files(std::slice::from_ref(&path))
            .expect("index through second instance");
        first
            .commit_index_batch(&old_ticket, std::slice::from_ref(&old_analysis))
            .expect("attempt first-instance stale commit");

        // Then: both instances share coordination and the newer data wins.
        assert!(Arc::ptr_eq(&first.mutations, &second.mutations));
        assert_eq!(first.committed_generation(), second.committed_generation());
        assert!(first.committed_generation() > 0);
        assert_eq!(
            first
                .find_symbol_cached("new_shared", None, false, false, 10)
                .expect("find shared symbol")
                .len(),
            1
        );
        assert!(
            first
                .find_symbol_cached("old_shared", None, false, false, 10)
                .expect("find stale shared symbol")
                .is_empty()
        );
    }

    #[test]
    fn mutation_tracker_prunes_paths_after_older_tickets_finish() {
        // Given: a newer observation must remain while an older ticket is in flight.
        let tracker = Arc::new(MutationTracker::default());
        let old_ticket = tracker.begin();
        old_ticket.observe_index_paths(&["src/pruned.rs".to_owned()]);
        let newer_ticket = tracker.begin();
        newer_ticket.observe_tombstones(&["src/pruned.rs".to_owned()]);

        // When: the newer ticket finishes before the older ticket.
        drop(newer_ticket);

        // Then: protection remains until the older ticket finishes, then is pruned.
        assert_eq!(tracker.lock().paths.len(), 1);
        drop(old_ticket);
        assert!(tracker.lock().paths.is_empty());
    }

    #[test]
    fn committed_generation_advances_only_for_successful_db_mutations() {
        // Given: a new index has no committed mutation generation.
        let (root, project) = race_project();
        let path = root.join("src/generation.rs");
        fs::write(&path, "pub fn generation_symbol() {}\n").expect("write source");
        let index = SymbolIndex::new_memory(project);
        assert_eq!(index.committed_generation(), 0);

        // When: a ticket is allocated, then one real write and read/no-op paths run.
        drop(index.mutations.begin());
        assert_eq!(index.committed_generation(), 0);
        index
            .index_files(std::slice::from_ref(&path))
            .expect("commit initial index");
        let indexed_generation = index.committed_generation();
        assert!(indexed_generation > 0);
        index
            .ensure_indexed(&path, "src/generation.rs")
            .expect("mtime fast path");
        index
            .index_files(std::slice::from_ref(&path))
            .expect("no-op fresh index");

        // Then: reads/no-ops do not advance, while an existing-row delete does.
        assert_eq!(index.committed_generation(), indexed_generation);
        index
            .remove_files(std::slice::from_ref(&path))
            .expect("delete indexed row");
        let deleted_generation = index.committed_generation();
        assert!(deleted_generation > indexed_generation);
        index
            .remove_files(std::slice::from_ref(&path))
            .expect("no-op missing-row delete");
        assert_eq!(index.committed_generation(), deleted_generation);
        index
            .ensure_indexed(&path, "src/generation.rs")
            .expect("slow ensure commit");
        assert!(index.committed_generation() > deleted_generation);
    }

    #[test]
    fn failed_ensure_does_not_advance_committed_generation() {
        // Given: an index with no committed writes and a missing source path.
        let (root, project) = race_project();
        let index = SymbolIndex::new_memory(project);
        let missing = root.join("src/missing.rs");

        // When: ensure fails before reaching a DB transaction.
        let _error = index
            .ensure_indexed(&missing, "src/missing.rs")
            .expect_err("missing source must fail ensure");

        // Then: the failure is reported and committed generation remains unchanged.
        assert_eq!(index.committed_generation(), 0);
    }
}
