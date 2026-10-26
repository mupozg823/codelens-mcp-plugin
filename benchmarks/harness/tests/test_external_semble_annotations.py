import importlib.util
import sys
import unittest
from pathlib import Path


BENCHMARKS_DIR = Path(__file__).resolve().parents[2]


def load_script_module(module_name: str, filename: str):
    spec = importlib.util.spec_from_file_location(module_name, BENCHMARKS_DIR / filename)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    spec.loader.exec_module(module)
    return module


SEMBLE = load_script_module("external_semble_annotations_test", "external-semble-annotations.py")


class FailedCallsTests(unittest.TestCase):
    def test_a_failed_batch_is_reported_not_scored_as_misses(self):
        batch = [{"name": "index_embeddings"}, {"name": "semantic_search"}, {"name": "get_ranked_context"}]
        payloads = [
            {"success": False, "error": "model not found"},
            {"success": False, "error": "embedding_unavailable"},
            {"success": True, "data": {"symbols": []}},
        ]

        failed = SEMBLE.failed_calls(batch, payloads, returncode=1)

        self.assertEqual(
            failed,
            ["#0 index_embeddings: model not found", "#1 semantic_search: embedding_unavailable"],
        )

    def test_a_clean_batch_has_no_failures(self):
        batch = [{"name": "index_embeddings"}, {"name": "semantic_search"}]
        payloads = [{"success": True}, {"success": True, "data": {"results": []}}]
        self.assertEqual(SEMBLE.failed_calls(batch, payloads, returncode=0), [])

    def test_a_nonzero_exit_with_successful_payloads_still_fails(self):
        self.assertEqual(
            SEMBLE.failed_calls([{"name": "semantic_search"}], [{"success": True}], returncode=2),
            ["batch exited 2"],
        )


class CacheDirsTests(unittest.TestCase):
    def test_cache_identity_follows_revision_and_benchmark_root(self):
        work = Path("/work")
        base = {"name": "tokio", "revision": "5db10f538b683fe88d699dfd11be31d193db011c", "benchmark_root": "tokio/src"}
        moved = dict(base, revision="0123456789abcdef0123456789abcdef01234567")
        rerooted = dict(base, benchmark_root="tokio")

        dirs = {key: SEMBLE.cache_dirs(repo, work) for key, repo in
                {"base": base, "moved": moved, "rerooted": rerooted}.items()}

        self.assertNotEqual(dirs["base"][0], dirs["moved"][0])
        self.assertNotEqual(dirs["base"][1], dirs["moved"][1])
        self.assertEqual(dirs["base"][0], dirs["rerooted"][0])
        self.assertNotEqual(dirs["base"][1], dirs["rerooted"][1])


if __name__ == "__main__":
    unittest.main()
