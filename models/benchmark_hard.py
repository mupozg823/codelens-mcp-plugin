"""
Hard benchmark: ambiguous queries, similar-looking code, cross-language confusion.
Tests whether the model understands code SEMANTICS, not just keywords.
"""

import time
import json
import numpy as np
from sentence_transformers import SentenceTransformer


def cosine_sim(a, b):
    return np.dot(a, b) / (np.linalg.norm(a) * np.linalg.norm(b))


# 20 code snippets - some are semantically similar but different
CODE_POOL = [
    # 0: Python sort
    "def bubble_sort(arr):\n    n = len(arr)\n    for i in range(n):\n        for j in range(0, n-i-1):\n            if arr[j] > arr[j+1]:\n                arr[j], arr[j+1] = arr[j+1], arr[j]\n    return arr",
    # 1: Python binary search
    "def binary_search(arr, target):\n    low, high = 0, len(arr) - 1\n    while low <= high:\n        mid = (low + high) // 2\n        if arr[mid] == target: return mid\n        elif arr[mid] < target: low = mid + 1\n        else: high = mid - 1\n    return -1",
    # 2: JS fetch API
    "async function fetchUserData(userId) {\n  const res = await fetch(`/api/users/${userId}`);\n  if (!res.ok) throw new Error('Failed');\n  return res.json();\n}",
    # 3: JS DOM manipulation
    "function updateDOM(elementId, content) {\n  const el = document.getElementById(elementId);\n  if (el) el.innerHTML = content;\n}",
    # 4: Rust file read
    "fn read_config(path: &Path) -> Result<Config, Box<dyn Error>> {\n    let content = fs::read_to_string(path)?;\n    let config: Config = toml::from_str(&content)?;\n    Ok(config)\n}",
    # 5: Rust TCP server
    "fn start_server(addr: &str) -> io::Result<()> {\n    let listener = TcpListener::bind(addr)?;\n    for stream in listener.incoming() {\n        handle_client(stream?);\n    }\n    Ok(())\n}",
    # 6: Go goroutine worker pool
    "func workerPool(jobs <-chan int, results chan<- int) {\n    for j := range jobs {\n        results <- process(j)\n    }\n}",
    # 7: Go HTTP handler
    'func handleRequest(w http.ResponseWriter, r *http.Request) {\n    var data RequestBody\n    json.NewDecoder(r.Body).Decode(&data)\n    w.Header().Set("Content-Type", "application/json")\n    json.NewEncoder(w).Encode(Response{Status: "ok"})\n}',
    # 8: Python decorator
    "def retry(max_retries=3):\n    def decorator(func):\n        def wrapper(*args, **kwargs):\n            for i in range(max_retries):\n                try: return func(*args, **kwargs)\n                except Exception as e:\n                    if i == max_retries - 1: raise\n        return wrapper\n    return decorator",
    # 9: Python context manager
    "class DatabaseConnection:\n    def __enter__(self):\n        self.conn = sqlite3.connect(self.db_path)\n        return self.conn\n    def __exit__(self, exc_type, exc_val, exc_tb):\n        self.conn.close()",
    # 10: Java thread pool
    "ExecutorService executor = Executors.newFixedThreadPool(4);\nfor (Task task : tasks) {\n    executor.submit(() -> {\n        task.process();\n        return task.getResult();\n    });\n}\nexecutor.shutdown();",
    # 11: Java stream API
    "List<String> result = users.stream()\n    .filter(u -> u.isActive())\n    .map(User::getName)\n    .sorted()\n    .collect(Collectors.toList());",
    # 12: Python async
    "async def fetch_all(urls):\n    async with aiohttp.ClientSession() as session:\n        tasks = [session.get(url) for url in urls]\n        responses = await asyncio.gather(*tasks)\n        return [await r.json() for r in responses]",
    # 13: Rust pattern matching
    'fn classify_number(n: i32) -> &\'static str {\n    match n {\n        0 => "zero",\n        1..=9 => "single digit",\n        10..=99 => "double digit",\n        _ => "large",\n    }\n}',
    # 14: Python list comprehension filter
    "active_users = [u for u in users if u.is_active and u.last_login > cutoff_date]",
    # 15: Go channel select
    "func multiplex(ch1, ch2 <-chan string) <-chan string {\n    out := make(chan string)\n    go func() {\n        for {\n            select {\n            case v := <-ch1: out <- v\n            case v := <-ch2: out <- v\n            }\n        }\n    }()\n    return out\n}",
    # 16: Python tree traversal
    "def inorder(node):\n    if node is None: return []\n    return inorder(node.left) + [node.val] + inorder(node.right)",
    # 17: JS event listener
    "document.addEventListener('DOMContentLoaded', () => {\n  const btn = document.querySelector('#submit');\n  btn.addEventListener('click', handleSubmit);\n});",
    # 18: Rust iterator chain
    "let total: f64 = prices.iter()\n    .filter(|p| **p > 0.0)\n    .map(|p| p * tax_rate)\n    .sum();",
    # 19: Python hash map / counter
    "from collections import Counter\ndef most_common_words(text, n=10):\n    words = text.lower().split()\n    return Counter(words).most_common(n)",
]

# Harder queries - require understanding semantics, not just keywords
QUERIES = [
    ("implement a sorting algorithm", 0),  # bubble_sort, not binary_search
    ("search for an element in a sorted array", 1),  # binary_search
    ("call a REST API endpoint", 2),  # fetchUserData, not handleRequest
    ("update the HTML page content", 3),  # updateDOM
    ("load configuration from a file", 4),  # read_config, not read_file
    ("accept incoming network connections", 5),  # start_server
    ("concurrent task processing with workers", 6),  # workerPool, not thread pool
    ("handle incoming HTTP request and send JSON", 7),  # handleRequest
    ("automatically retry a failed operation", 8),  # retry decorator
    (
        "manage database lifecycle open and close",
        9,
    ),  # DatabaseConnection context manager
    ("run tasks in parallel using threads", 10),  # Java thread pool
    ("filter and transform a collection", 11),  # Java stream, not list comprehension
    ("fetch multiple URLs concurrently", 12),  # async fetch_all
    ("categorize input based on value ranges", 13),  # pattern matching
    ("select items from a list based on conditions", 14),  # list comprehension
    ("merge multiple data streams into one", 15),  # Go channel select/multiplex
    ("traverse a binary tree in sorted order", 16),  # inorder traversal
    ("respond to user clicking a button", 17),  # event listener
    ("calculate total with filtering and tax", 18),  # Rust iterator chain
    ("count word frequencies in text", 19),  # Counter
]


def evaluate_model(model_name, model):
    print(f"\n{'='*60}")
    print(f"Model: {model_name}")
    print(f"{'='*60}")

    code_embeddings = model.encode(CODE_POOL, show_progress_bar=False)
    queries_text = [q for q, _ in QUERIES]
    query_embeddings = model.encode(queries_text, show_progress_bar=False)

    mrr_sum = 0.0
    top1 = 0
    top3 = 0
    top5 = 0

    for i, (query, expected_idx) in enumerate(QUERIES):
        sims = [cosine_sim(query_embeddings[i], ce) for ce in code_embeddings]
        ranked = np.argsort(sims)[::-1]
        rank = np.where(ranked == expected_idx)[0][0] + 1

        mrr_sum += 1.0 / rank
        if rank == 1:
            top1 += 1
        if rank <= 3:
            top3 += 1
        if rank <= 5:
            top5 += 1

        status = "O" if rank <= 3 else "X"
        print(f"  [{status}] rank={rank:2d} | {query[:55]}")

    n = len(QUERIES)
    print(f"\n  MRR:   {mrr_sum/n:.3f}")
    print(f"  Acc@1: {top1/n:.1%}")
    print(f"  Acc@3: {top3/n:.1%}")
    print(f"  Acc@5: {top5/n:.1%}")

    return {
        "model": model_name,
        "mrr": mrr_sum / n,
        "acc1": top1 / n,
        "acc3": top3 / n,
        "acc5": top5 / n,
    }


def main():
    models = [
        ("BGE-Small-EN-v1.5 (current)", "BAAI/bge-small-en-v1.5"),
        ("MiniLM-L12-CodeSearchNet", "isuruwijesiri/all-MiniLM-L12-v2-code-search-512"),
        ("MiniLM-L6-v2 (base)", "sentence-transformers/all-MiniLM-L6-v2"),
    ]

    results = []
    for name, model_id in models:
        print(f"\nLoading {name}...")
        model = SentenceTransformer(model_id)
        r = evaluate_model(name, model)
        results.append(r)
        del model

    print(f"\n{'='*60}")
    print("HARD BENCHMARK SUMMARY")
    print(f"{'='*60}")
    print(f"{'Model':<35} {'MRR':>6} {'@1':>5} {'@3':>5} {'@5':>5}")
    print("-" * 58)
    for r in results:
        print(
            f"{r['model']:<35} {r['mrr']:>6.3f} {r['acc1']:>4.0%} {r['acc3']:>4.0%} {r['acc5']:>4.0%}"
        )

    with open("benchmark_hard_results.json", "w") as f:
        json.dump(results, f, indent=2)


if __name__ == "__main__":
    main()
