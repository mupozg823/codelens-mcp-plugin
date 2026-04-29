use codelens_engine::{ProjectRoot, search_symbols_hybrid};
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use std::fs;

/// Create a fixture with enough symbols to make search-path differences visible.
fn create_search_fixture() -> (tempfile::TempDir, ProjectRoot) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();

    // 50 Rust files × 4 symbols each = 200 symbols
    for i in 0..50 {
        let content = format!(
            r#"
pub struct Service{i} {{}}
impl Service{i} {{
    pub fn process(&self) {{}}
    pub fn validate(&self) -> bool {{ true }}
    pub fn handle_request(&self, req: &str) -> String {{ req.to_owned() }}
}}
"#,
            i = i
        );
        fs::write(root.join(format!("src/service_{i}.rs")), content).unwrap();
    }

    let project = ProjectRoot::new(root).expect("project");
    (dir, project)
}

fn bench_search_exact(c: &mut Criterion) {
    let (_dir, project) = create_search_fixture();

    c.bench_function("search exact (Service25)", |b| {
        b.iter(|| {
            search_symbols_hybrid(black_box(&project), "Service25", 10, 0.6).unwrap();
        })
    });
}

fn bench_search_fts(c: &mut Criterion) {
    let (_dir, project) = create_search_fixture();

    // "process" matches many symbols via FTS5 (substring in method names)
    c.bench_function("search fts (process)", |b| {
        b.iter(|| {
            search_symbols_hybrid(black_box(&project), "process", 20, 0.99).unwrap();
        })
    });
}

fn bench_search_fuzzy(c: &mut Criterion) {
    let (_dir, project) = create_search_fixture();

    // "Srvce25" is a fuzzy misspelling that bypasses exact/fts and hits jaro_winkler
    c.bench_function("search fuzzy (Srvce25)", |b| {
        b.iter(|| {
            search_symbols_hybrid(black_box(&project), "Srvce25", 20, 0.6).unwrap();
        })
    });
}

fn bench_search_no_match(c: &mut Criterion) {
    let (_dir, project) = create_search_fixture();

    // "xyz123nonexistent" exercises all paths and returns empty
    c.bench_function("search no-match (xyz123nonexistent)", |b| {
        b.iter(|| {
            search_symbols_hybrid(black_box(&project), "xyz123nonexistent", 20, 0.6).unwrap();
        })
    });
}

criterion_group!(
    benches,
    bench_search_exact,
    bench_search_fts,
    bench_search_fuzzy,
    bench_search_no_match,
);
criterion_main!(benches);
