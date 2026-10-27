"""Mechanical grading fixtures, explicitly separate from real pilot answers."""

import importlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))


class GradingTests(unittest.TestCase):
    def setUp(self):
        self.module = importlib.import_module("analysis_lite_grading")
        self.task = {"id": "fixture", "gold": {"declaration": {"file": "lib/a.ts", "symbol": "target", "line": 1},
            "direct_calls": [{"file": "lib/b.ts", "lines": [3, 4]}],
            "non_calls": [{"file": "lib/c.ts", "line": 2, "kind": "import via alias"}],
            "acceptance": ["semantic meaning requires review"]}}
        self.answer = {"declaration": {"file": "lib/a.ts", "symbol": "target", "line": 1},
                       "calls": [{"file": "lib/b.ts", "lines": [3, 4]}],
                       "non_calls": [{"file": "lib/c.ts", "line": 2, "kind": "import"}], "notes": "evidence"}

    def test_exact_locations_still_require_independent_semantic_review(self):
        result = self.module.grade_answer(self.task, self.answer)
        self.assertEqual(result["mechanical_status"], "pass")
        self.assertEqual(result["status"], "manual-review")
        self.assertIsNone(result["task_quality"])

    def test_explicit_missing_extra_and_wrong_declaration_fail(self):
        for change in ({"calls": []}, {"calls": [{"file": "lib/b.ts", "lines": [3, 4, 9]}]},
                       {"declaration": {"file": "lib/other.ts", "symbol": "target", "line": 1}}):
            with self.subTest(change=change):
                result = self.module.grade_answer(self.task, {**self.answer, **change})
                self.assertEqual(result["mechanical_status"], "fail")

    def test_unstructured_missing_and_null_remain_reviewable(self):
        for answer in ("plain source answer", {}, {**self.answer, "calls": None}):
            with self.subTest(answer=answer):
                result = self.module.grade_answer(self.task, answer)
                self.assertEqual(result["status"], "manual-review")
                self.assertIsNone(result["mechanical_status"])
                self.assertEqual(result["raw_answer"], answer)

    def test_impact_depth_nodes_and_multisite_edges_are_exact(self):
        task = {"gold": {"declaration": {"file": "lib/a.ts", "symbol": "target", "line": 1},
                "reverse_edges": [{"caller": "caller", "callee": "target", "file": "lib/b.ts", "lines": [3, 4], "depth": 1}],
                "acceptance": []}, "limits": {"reverse_depth": 1, "node_cap": 2, "edge_cap": 2}}
        answer = {"declaration": self.answer["declaration"], "impact": {"nodes": ["target", "caller"],
                  "edges": task["gold"]["reverse_edges"], "bounds": {"reverse_depth": 1, "node_cap": 2, "edge_cap": 2}}}
        self.assertEqual(self.module.grade_answer(task, answer)["mechanical_status"], "pass")
        answer["impact"]["nodes"].append("unrelated")
        self.assertEqual(self.module.grade_answer(task, answer)["mechanical_status"], "fail")

    def test_supplied_wrong_identifier_bytes_are_not_hidden_by_correct_line(self):
        self.task["gold"]["declaration"].update(byte_start=10, byte_end=16)
        self.answer["declaration"].update(byte_start=11, byte_end=17)
        self.assertEqual(self.module.grade_answer(self.task, self.answer)["mechanical_status"], "fail")

    def test_owned_timeout_kills_child_group_and_preserves_json_serializable_output(self):
        sentinel = subprocess.Popen([sys.executable, "-c", "import time;time.sleep(60)"], start_new_session=True)
        script = ('import subprocess,sys,time; p=subprocess.Popen([sys.executable,"-c",'
                  '"import signal,time;signal.signal(signal.SIGTERM,signal.SIG_IGN);time.sleep(60)"]);'
                  'print(p.pid,flush=True); print("diagnostic",file=sys.stderr,flush=True);time.sleep(60)')
        try:
            receipt = self.module.run_owned([sys.executable, "-c", script], timeout=0.3)
            self.assertTrue(receipt["timed_out"])
            self.assertEqual(receipt["process_group"], receipt["pid"])
            self.assertTrue(receipt["leader_reaped"])
            self.assertIn("diagnostic", receipt["stderr"])
            child = int(receipt["stdout"].strip())
            active = True
            for _ in range(20):
                result = subprocess.run(["ps", "-o", "stat=", "-p", str(child)], capture_output=True, text=True, check=False)
                active = bool(result.stdout.strip()) and not result.stdout.strip().startswith("Z")
                if not active:
                    break
                time.sleep(0.05)
            self.assertFalse(active)
            self.assertIsNone(sentinel.poll())
            json.dumps(receipt)
        finally:
            sentinel.terminate(); sentinel.wait(timeout=5)

    @unittest.skipUnless(os.environ.get("ANALYSIS_LITE_TEST_SETUP"), "requires task-local frozen npm setup")
    def test_agent_test_is_replayed_red_then_green_only_in_post_run_copies(self):
        manifest = json.loads((Path(__file__).resolve().parents[2] / "analysis-lite/tasks-v1.json").read_text())
        task = next(t for t in manifest["tasks"] if t["operation"] == "edit")
        dependencies = Path(os.environ["ANALYSIS_LITE_TEST_SETUP"])
        eval_root = dependencies.parents[1]
        with tempfile.TemporaryDirectory(dir=eval_root / "control") as tmp:
            base = Path(tmp) / "base"; final = Path(tmp) / "final"
            target, test = task["limits"]["changed_paths"]
            (base / target).parent.mkdir(parents=True)
            (base / target).write_text('export function formatCrewValue(v,m,p) { return (Math.floor(v*10**p)/10**p).toFixed(p); }')
            for name in ("package.json", "package-lock.json"):
                (base / name).write_bytes((dependencies / name).read_bytes())
            shutil.copytree(base, final)
            (final / target).write_text('export function formatCrewValue(v,m,p) { if(m==="amount")return Math.floor(v).toLocaleString("ko-KR");const x=v*10**p,n=Math.round(x);return((Math.abs(x-n)<=1e-9*Math.max(1,Math.abs(x))?n:Math.floor(x))/10**p).toFixed(p); }')
            (final / test).write_text('import {it,expect} from "vitest";import {formatCrewValue} from "./crew-value-format";it("preserves decimal grid",()=>expect(formatCrewValue(0.29,"contribution",2)).toBe("0.29"));')
            result = self.module.grade_edit(task, base, final, dependencies, Path(tmp) / "red-replay", eval_root=eval_root)
            self.assertNotEqual(result["base_red"]["returncode"], 0)
            self.assertEqual(result["final_green"]["returncode"], 0)
            self.assertTrue(result["oracle"]["result"]["passed"])
            self.assertEqual(result["status"], "manual-review")
            self.assertFalse((final / "node_modules").is_symlink())
            self.assertFalse((Path(tmp) / "red-replay/node_modules").is_symlink())
            self.assertFalse((base / test).exists())
            (final / "unexpected.ts").write_text("not allowed")
            self.assertEqual(self.module.grade_edit(task, base, final, dependencies,
                Path(tmp) / "unused", eval_root=eval_root)["status"], "fail")

    @unittest.skipUnless(os.environ.get("ANALYSIS_LITE_TEST_SETUP"), "requires task-local frozen npm setup")
    def test_real_node_oracle_separates_floor_bug_from_fixed_behavior(self):
        manifest = json.loads((Path(__file__).resolve().parents[2] / "analysis-lite/tasks-v1.json").read_text())
        task = next(t for t in manifest["tasks"] if t["operation"] == "edit")
        with tempfile.TemporaryDirectory() as tmp:
            source = Path(tmp) / "formatter.ts"
            source.write_text('export function formatCrewValue(v, m, p) { return m === "amount" ? Math.floor(v).toLocaleString("ko-KR") : (Math.floor(v * 10 ** p) / 10 ** p).toFixed(p); }')
            bad = self.module.run_oracle(task, source, Path(os.environ["ANALYSIS_LITE_TEST_SETUP"]))
            self.assertFalse(bad["result"]["passed"])
            source.write_text('export function formatCrewValue(v, m, p) { if (m === "amount") return Math.floor(v).toLocaleString("ko-KR"); const x=v*10**p, n=Math.round(x); return ((Math.abs(x-n)<=1e-9*Math.max(1,Math.abs(x))?n:Math.floor(x))/10**p).toFixed(p); }')
            good = self.module.run_oracle(task, source, Path(os.environ["ANALYSIS_LITE_TEST_SETUP"]))
            self.assertTrue(good["result"]["passed"])
            self.assertEqual(good["result"]["exhaustive_cases"], 100001)


if __name__ == "__main__":
    unittest.main()
