#!/usr/bin/env python3
"""Unit tests for retrieval_fbeta_metrics (stdlib unittest, no daemon).

Covers precision/recall extremes, the beta<1 precision emphasis, the 0..1
boundary invariant, and the suffix / line-pair matchers.

Run: python3 -m unittest benchmarks.test_retrieval_fbeta
  or python3 benchmarks/test_retrieval_fbeta.py
"""

import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import retrieval_fbeta_metrics as m  # noqa: E402


class WeightedFBetaTest(unittest.TestCase):
    def test_perfect(self):
        self.assertEqual(m.weighted_f_beta(1.0, 1.0, 0.5), 1.0)

    def test_zero_precision(self):
        self.assertEqual(m.weighted_f_beta(0.0, 1.0, 0.5), 0.0)

    def test_zero_recall(self):
        self.assertEqual(m.weighted_f_beta(1.0, 0.0, 0.5), 0.0)

    def test_both_zero_no_div_by_zero(self):
        self.assertEqual(m.weighted_f_beta(0.0, 0.0, 0.5), 0.0)

    def test_beta_half_favors_precision(self):
        # Same P/R magnitudes swapped: beta=0.5 must reward high precision more.
        high_precision = m.weighted_f_beta(1.0, 0.5, 0.5)
        high_recall = m.weighted_f_beta(0.5, 1.0, 0.5)
        self.assertGreater(high_precision, high_recall)

    def test_beta_two_favors_recall(self):
        high_precision = m.weighted_f_beta(1.0, 0.5, 2.0)
        high_recall = m.weighted_f_beta(0.5, 1.0, 2.0)
        self.assertGreater(high_recall, high_precision)

    def test_f1_symmetry(self):
        # beta=1 is symmetric in precision/recall.
        self.assertAlmostEqual(
            m.weighted_f_beta(0.3, 0.8, 1.0),
            m.weighted_f_beta(0.8, 0.3, 1.0),
        )

    def test_output_within_unit_interval(self):
        for p in (0.0, 0.1, 0.5, 0.9, 1.0):
            for r in (0.0, 0.2, 0.6, 1.0):
                for beta in (0.5, 1.0, 2.0):
                    v = m.weighted_f_beta(p, r, beta)
                    self.assertGreaterEqual(v, 0.0)
                    self.assertLessEqual(v, 1.0)

    def test_rejects_bad_beta(self):
        with self.assertRaises(ValueError):
            m.weighted_f_beta(1.0, 1.0, 0.0)

    def test_rejects_out_of_range_inputs(self):
        with self.assertRaises(ValueError):
            m.weighted_f_beta(1.5, 0.5, 0.5)
        with self.assertRaises(ValueError):
            m.weighted_f_beta(0.5, -0.1, 0.5)


class PrfSetTest(unittest.TestCase):
    def test_partial_precision(self):
        out = m.prf({"a", "b", "c"}, {"a"})
        self.assertAlmostEqual(out["precision"], 1 / 3)
        self.assertEqual(out["recall"], 1.0)

    def test_empty_retrieved_nonempty_gold(self):
        out = m.prf(set(), {"a"})
        self.assertEqual(out["precision"], 0.0)
        self.assertEqual(out["recall"], 0.0)
        self.assertEqual(out["f_beta"], 0.0)

    def test_both_empty(self):
        out = m.prf(set(), set())
        self.assertEqual(out["precision"], 1.0)
        self.assertEqual(out["recall"], 1.0)

    def test_invariant_bounds(self):
        out = m.prf({"x", "y"}, {"y", "z"})
        for key in ("precision", "recall", "f_beta"):
            self.assertGreaterEqual(out[key], 0.0)
            self.assertLessEqual(out[key], 1.0)


class SuffixMatchTest(unittest.TestCase):
    def test_right_file_among_noise(self):
        retrieved = {
            "crates/engine/src/rename.rs",
            "crates/engine/src/move_symbol.rs",
        }
        out = m.suffix_match_prf(retrieved, {"src/rename.rs"})
        self.assertAlmostEqual(out["precision"], 0.5)
        self.assertEqual(out["recall"], 1.0)

    def test_miss(self):
        out = m.suffix_match_prf({"a/b.rs"}, {"c/d.rs"})
        self.assertEqual(out["precision"], 0.0)
        self.assertEqual(out["recall"], 0.0)


class LinePairTest(unittest.TestCase):
    def test_exact_line_hit(self):
        retrieved = {("crates/engine/src/rename.rs", 46), ("x/other.rs", 10)}
        gold = {("src/rename.rs", 46)}
        out = m.line_pair_prf(retrieved, gold)
        self.assertAlmostEqual(out["precision"], 0.5)
        self.assertEqual(out["recall"], 1.0)

    def test_right_file_wrong_line_is_penalized(self):
        # Correct file, wrong line -> line-level recall must be 0.
        retrieved = {("crates/engine/src/rename.rs", 99)}
        gold = {("src/rename.rs", 46)}
        out = m.line_pair_prf(retrieved, gold)
        self.assertEqual(out["precision"], 0.0)
        self.assertEqual(out["recall"], 0.0)


class AggregateTest(unittest.TestCase):
    def test_macro_average_bounds(self):
        per_query = [
            {
                "reciprocal_rank": 1.0,
                "file": {"precision": 1.0, "recall": 1.0, "f_beta": 1.0},
                "line": {"precision": 0.5, "recall": 1.0, "f_beta": 0.8},
            },
            {
                "reciprocal_rank": 0.0,
                "file": {"precision": 0.0, "recall": 0.0, "f_beta": 0.0},
                "line": {"precision": 0.0, "recall": 0.0, "f_beta": 0.0},
            },
        ]
        agg = m.aggregate(per_query)
        self.assertEqual(agg["count"], 2)
        self.assertEqual(agg["line_count"], 2)
        for key in (
            "mrr",
            "file_precision",
            "file_f_beta",
            "line_precision",
            "line_f_beta",
        ):
            self.assertGreaterEqual(agg[key], 0.0)
            self.assertLessEqual(agg[key], 1.0)

    def test_unresolved_gold_line_excluded(self):
        # A query whose gold line is unresolved (line=None) must not count
        # toward the line aggregate, but still counts for file-level.
        per_query = [
            {
                "reciprocal_rank": 1.0,
                "file": {"precision": 1.0, "recall": 1.0, "f_beta": 1.0},
                "line": {"precision": 1.0, "recall": 1.0, "f_beta": 1.0},
            },
            {
                "reciprocal_rank": 0.0,
                "file": {"precision": 0.0, "recall": 0.0, "f_beta": 0.0},
                "line": None,
            },
        ]
        agg = m.aggregate(per_query)
        self.assertEqual(agg["count"], 2)
        self.assertEqual(agg["line_count"], 1)
        self.assertEqual(agg["line_recall"], 1.0)
        self.assertEqual(agg["file_recall"], 0.5)


if __name__ == "__main__":
    unittest.main()
