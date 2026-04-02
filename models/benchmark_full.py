"""
Exploratory embedding quality evaluation on CodeLens's own codebase.
This script uses an external sentence-transformers model as a comparative harness.
It is not the same as the runtime-bundled CodeLens embedding model.
"""

import time
import json
import numpy as np
from sentence_transformers import SentenceTransformer

MODEL_ID = "isuruwijesiri/all-MiniLM-L12-v2-code-search-512"

# ── Real code snippets from CodeLens codebase ───────────────────────
CODEBASE = {
    "rename_symbol": "pub fn rename_symbol(project: &ProjectRoot, file_path: &str, symbol_name: &str, new_name: &str, name_path: Option<&str>, scope: RenameScope, dry_run: bool) -> Result<RenameResult>",
    "find_symbol_range": "pub fn find_symbol_range(project: &ProjectRoot, relative_path: &str, symbol_name: &str, name_path: Option<&str>) -> Result<(usize, usize)>",
    "apply_edits": "pub fn apply_edits(project: &ProjectRoot, edits: &[RenameEdit]) -> Result<()> { let mut by_file: HashMap<String, Vec<&RenameEdit>> = HashMap::new(); }",
    "get_ranked_context": 'pub fn get_ranked_context(state: &AppState, arguments: &serde_json::Value) -> ToolResult { let query = required_string(arguments, "query")?; }',
    "inline_function": "pub fn inline_function(project: &ProjectRoot, file_path: &str, function_name: &str, name_path: Option<&str>, dry_run: bool) -> Result<InlineResult>",
    "move_symbol": "pub fn move_symbol(project: &ProjectRoot, file_path: &str, symbol_name: &str, name_path: Option<&str>, target_file: &str, dry_run: bool) -> Result<MoveResult>",
    "change_signature": "pub fn change_signature(project: &ProjectRoot, file_path: &str, function_name: &str, name_path: Option<&str>, new_params: &[ParamSpec], dry_run: bool) -> Result<ChangeSignatureResult>",
    "build_embedding_text": 'fn build_embedding_text(sym: &SymbolWithFile, source: Option<&str>) -> String { format!("passage: {} {}", sym.kind, sym.name) }',
    "cosine_similarity": "fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 { let dot: f64 = a.iter().zip(b).map(|(x,y)| *x as f64 * *y as f64).sum(); }",
    "dispatch_tool": 'pub fn dispatch_tool(state: &AppState, id: Option<Value>, params: Value) -> JsonRpcResponse { let name = params.get("name"); }',
    "run_http": "pub async fn run_http(state: AppState, port: u16) -> Result<()> { let app = build_router(state); let listener = TcpListener::bind(addr).await?; }",
    "run_stdio": "pub fn run_stdio(state: AppState) -> Result<()> { loop { let mut line = String::new(); stdin().read_line(&mut line)?; } }",
    "handle_request": 'pub fn handle_request(state: &AppState, request: JsonRpcRequest) -> Option<JsonRpcResponse> { match request.method.as_str() { "tools/list" => ... } }',
    "find_circular_deps": "pub fn find_circular_dependencies(project: &ProjectRoot, max_cycles: usize, graph_cache: &GraphCache) -> Result<Vec<CircularDependency>>",
    "get_importers": "pub fn get_importers(project: &ProjectRoot, file_path: &str, max_results: usize, graph_cache: &GraphCache) -> Result<Vec<ImporterEntry>>",
    "parse_symbols": "fn parse_symbols_recursive(node: &Node, source: &[u8], config: &LangConfig, depth: usize) -> Vec<SymbolInfo>",
    "index_from_project": "pub fn index_from_project(&self, project: &ProjectRoot) -> Result<usize> { self.store.clear()?; let mut model = self.model.lock()?; }",
    "semantic_search": "pub fn search(&self, query: &str, max_results: usize) -> Result<Vec<SemanticMatch>> { let query_embedding = self.model.embed(vec![query])?; }",
    "find_duplicates": "pub fn find_duplicates(&self, threshold: f64, max_pairs: usize) -> Result<Vec<DuplicatePair>> { let all = self.store.all_with_embeddings()?; }",
    "classify_symbol": "pub fn classify_symbol(&self, file_path: &str, symbol_name: &str, categories: &[&str]) -> Result<Vec<CategoryScore>>",
    "onboard_project": "pub fn onboard_project(state: &AppState, _arguments: &Value) -> ToolResult { let structure = state.symbol_index().get_project_structure()?; }",
    "file_watcher_start": "pub fn start_watching(&self, root: &Path) -> Result<()> { let watcher = notify::recommended_watcher(move |event| { tx.send(event).ok(); })?; }",
    "extract_function": 'pub fn refactor_extract_function(state: &AppState, arguments: &Value) -> ToolResult { let start_line = arguments.get("start_line"); }',
    "build_non_code_ranges": "fn build_non_code_ranges(path: &Path, source: &[u8]) -> Vec<(usize, usize)> { let mut parser = tree_sitter::Parser::new(); }",
}


def cosine_sim(a, b):
    return float(np.dot(a, b) / (np.linalg.norm(a) * np.linalg.norm(b)))


def test_search_quality(model, code_embs, code_names):
    """Test 1: Natural language → code search"""
    print("\n" + "=" * 70)
    print("TEST 1: Natural Language → Code Search")
    print("=" * 70)

    queries = [
        ("rename a variable or function across the project", "rename_symbol"),
        ("find where a symbol is defined in a file", "find_symbol_range"),
        ("apply text edits to multiple files", "apply_edits"),
        ("search code by natural language query", "semantic_search"),
        ("inline a function and remove its definition", "inline_function"),
        ("move code to a different file", "move_symbol"),
        ("change function parameters", "change_signature"),
        ("find circular import dependencies", "find_circular_deps"),
        ("start an HTTP server with routes", "run_http"),
        ("read input from stdin line by line", "run_stdio"),
        ("parse source code into an AST", "parse_symbols"),
        ("build embedding vectors for all symbols", "index_from_project"),
        ("find near-duplicate code in the codebase", "find_duplicates"),
        ("categorize a function by its purpose", "classify_symbol"),
        ("get project structure and key files on first load", "onboard_project"),
        ("watch filesystem for file changes", "file_watcher_start"),
        ("extract lines of code into a new function", "extract_function"),
        ("skip comments and string literals during search", "build_non_code_ranges"),
        ("compute similarity between two vectors", "cosine_similarity"),
        ("route an incoming tool request to the right handler", "dispatch_tool"),
    ]

    mrr_sum = 0
    top1 = top3 = top5 = 0
    n = len(queries)

    for query, expected in queries:
        q_emb = model.encode(query)
        sims = {name: cosine_sim(q_emb, code_embs[name]) for name in code_names}
        ranked = sorted(sims.keys(), key=lambda k: sims[k], reverse=True)
        rank = ranked.index(expected) + 1

        mrr_sum += 1.0 / rank
        if rank == 1:
            top1 += 1
        if rank <= 3:
            top3 += 1
        if rank <= 5:
            top5 += 1

        status = "O" if rank <= 3 else "X"
        top_match = ranked[0]
        print(f"  [{status}] rank={rank:2d} sim={sims[expected]:.3f} | {query[:50]}")
        if rank > 3:
            print(
                f"        expected: {expected}, got: {top_match} (sim={sims[top_match]:.3f})"
            )

    print(f"\n  MRR:   {mrr_sum/n:.3f}")
    print(f"  Acc@1: {top1/n:.1%}  Acc@3: {top3/n:.1%}  Acc@5: {top5/n:.1%}")
    return {"mrr": mrr_sum / n, "acc1": top1 / n, "acc3": top3 / n, "acc5": top5 / n}


def test_code_similarity(model, code_embs):
    """Test 2: Code-to-code similarity (finds functionally similar code)"""
    print("\n" + "=" * 70)
    print("TEST 2: Code-to-Code Similarity")
    print("=" * 70)

    expected_similar = [
        ("rename_symbol", "apply_edits", "both do multi-file edits"),
        ("inline_function", "extract_function", "inverse refactoring operations"),
        ("move_symbol", "rename_symbol", "both modify code across files"),
        ("run_http", "run_stdio", "both are transport entry points"),
        ("semantic_search", "find_duplicates", "both use embeddings"),
        ("find_symbol_range", "parse_symbols", "both parse AST"),
    ]

    correct = 0
    for sym_a, sym_b, reason in expected_similar:
        sim = cosine_sim(code_embs[sym_a], code_embs[sym_b])
        # Check if sym_b is in top-5 most similar to sym_a
        all_sims = {
            k: cosine_sim(code_embs[sym_a], v)
            for k, v in code_embs.items()
            if k != sym_a
        }
        ranked = sorted(all_sims.keys(), key=lambda k: all_sims[k], reverse=True)
        rank = ranked.index(sym_b) + 1
        status = "O" if rank <= 5 else "X"
        if rank <= 5:
            correct += 1
        print(f"  [{status}] {sym_a} ↔ {sym_b}: sim={sim:.3f} rank={rank} ({reason})")

    print(
        f"\n  Pair accuracy (top-5): {correct}/{len(expected_similar)} ({correct/len(expected_similar):.0%})"
    )
    return {"pair_accuracy": correct / len(expected_similar)}


def test_classification(model, code_embs, code_names):
    """Test 3: Zero-shot code classification"""
    print("\n" + "=" * 70)
    print("TEST 3: Zero-Shot Code Classification")
    print("=" * 70)

    categories = [
        "code refactoring and transformation",
        "file and symbol search",
        "network and transport",
        "embedding and machine learning",
        "code analysis and metrics",
        "file system operations",
    ]
    cat_embs = model.encode(categories)

    expected_map = {
        "rename_symbol": "code refactoring and transformation",
        "inline_function": "code refactoring and transformation",
        "extract_function": "code refactoring and transformation",
        "find_symbol_range": "file and symbol search",
        "semantic_search": "embedding and machine learning",
        "run_http": "network and transport",
        "run_stdio": "network and transport",
        "find_circular_deps": "code analysis and metrics",
        "file_watcher_start": "file system operations",
        "index_from_project": "embedding and machine learning",
    }

    correct = 0
    for sym, expected_cat in expected_map.items():
        sims = [cosine_sim(code_embs[sym], ce) for ce in cat_embs]
        predicted_idx = np.argmax(sims)
        predicted = categories[predicted_idx]
        ok = predicted == expected_cat
        if ok:
            correct += 1
        status = "O" if ok else "X"
        print(f"  [{status}] {sym:25s} → {predicted} (expected: {expected_cat})")

    print(
        f"\n  Classification accuracy: {correct}/{len(expected_map)} ({correct/len(expected_map):.0%})"
    )
    return {"classification_accuracy": correct / len(expected_map)}


def test_duplicate_detection(model, code_embs, code_names):
    """Test 4: Duplicate/similar code detection"""
    print("\n" + "=" * 70)
    print("TEST 4: Duplicate Detection (threshold=0.75)")
    print("=" * 70)

    threshold = 0.75
    pairs = []
    names = list(code_names)
    for i in range(len(names)):
        for j in range(i + 1, len(names)):
            sim = cosine_sim(code_embs[names[i]], code_embs[names[j]])
            if sim >= threshold:
                pairs.append((names[i], names[j], sim))

    pairs.sort(key=lambda x: x[2], reverse=True)
    print(f"  Found {len(pairs)} pairs above threshold {threshold}:")
    for a, b, sim in pairs[:15]:
        print(f"    {sim:.3f} | {a} ↔ {b}")

    return {"pairs_found": len(pairs), "threshold": threshold}


def test_outlier_detection(model, code_embs, code_names):
    """Test 5: Outlier detection (semantically distant symbols)"""
    print("\n" + "=" * 70)
    print("TEST 5: Outlier Detection")
    print("=" * 70)

    names = list(code_names)
    avg_sims = {}
    for name in names:
        sims = [
            cosine_sim(code_embs[name], code_embs[other])
            for other in names
            if other != name
        ]
        avg_sims[name] = np.mean(sims)

    sorted_names = sorted(avg_sims.keys(), key=lambda k: avg_sims[k])

    print("  Most isolated (potential outliers):")
    for name in sorted_names[:5]:
        print(f"    avg_sim={avg_sims[name]:.3f} | {name}")

    print("\n  Most central (core symbols):")
    for name in sorted_names[-5:]:
        print(f"    avg_sim={avg_sims[name]:.3f} | {name}")

    return {"most_isolated": sorted_names[0], "most_central": sorted_names[-1]}


def test_latency(model):
    """Test 6: Latency benchmarks"""
    print("\n" + "=" * 70)
    print("TEST 6: Latency Benchmarks")
    print("=" * 70)

    # Single embedding
    times = []
    for _ in range(20):
        t0 = time.perf_counter()
        model.encode("find a function that handles HTTP requests")
        times.append(time.perf_counter() - t0)
    single_ms = np.median(times) * 1000

    # Batch embedding (simulating indexing)
    batch = list(CODEBASE.values())
    times = []
    for _ in range(5):
        t0 = time.perf_counter()
        model.encode(batch)
        times.append(time.perf_counter() - t0)
    batch_ms = np.median(times) * 1000
    per_item_ms = batch_ms / len(batch)

    print(f"  Single query:     {single_ms:.1f}ms (median of 20)")
    print(
        f"  Batch ({len(batch)} items): {batch_ms:.0f}ms total, {per_item_ms:.1f}ms/item"
    )
    print(f"  Throughput:       {1000/per_item_ms:.0f} embeddings/sec")

    return {"single_ms": single_ms, "batch_ms": batch_ms, "per_item_ms": per_item_ms}


def main():
    print("Loading MiniLM-L12-CodeSearchNet...")
    model = SentenceTransformer(MODEL_ID)

    # Pre-compute all code embeddings
    code_names = list(CODEBASE.keys())
    code_texts = list(CODEBASE.values())
    code_embs = {name: emb for name, emb in zip(code_names, model.encode(code_texts))}

    results = {}
    results["search"] = test_search_quality(model, code_embs, code_names)
    results["similarity"] = test_code_similarity(model, code_embs)
    results["classification"] = test_classification(model, code_embs, code_names)
    results["duplicates"] = test_duplicate_detection(model, code_embs, code_names)
    results["outliers"] = test_outlier_detection(model, code_embs, code_names)
    results["latency"] = test_latency(model)

    print("\n" + "=" * 70)
    print("SUMMARY")
    print("=" * 70)
    print(
        f"  NL→Code Search MRR:    {results['search']['mrr']:.3f} (Acc@1={results['search']['acc1']:.0%}, @3={results['search']['acc3']:.0%})"
    )
    print(
        f"  Code Similarity:       {results['similarity']['pair_accuracy']:.0%} pair accuracy"
    )
    print(
        f"  Zero-shot Classification: {results['classification']['classification_accuracy']:.0%}"
    )
    print(f"  Duplicate pairs (≥0.75): {results['duplicates']['pairs_found']}")
    print(f"  Query latency:         {results['latency']['single_ms']:.1f}ms")
    print(
        f"  Index throughput:      {1000/results['latency']['per_item_ms']:.0f} symbols/sec"
    )

    with open("benchmark_full_results.json", "w") as f:
        json.dump(results, f, indent=2)


if __name__ == "__main__":
    main()
