"""Synthetic protocol fixtures only: these are not measured provider runs."""

import importlib
import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))


def jsonl(*events):
    return "\n".join(json.dumps(event) for event in events) + "\n"


def codex_final(**usage):
    return {"type": "turn.completed", "usage": {"input_tokens": 100,
            "cached_input_tokens": 70, "output_tokens": 20, **usage}}


def claude_final(**changes):
    event = {"type": "result", "subtype": "success", "is_error": False,
             "session_id": "session-1", "uuid": "result-1", "result": "done",
             "usage": {"input_tokens": 10, "cache_read_input_tokens": 70,
                       "cache_creation_input_tokens": 20, "output_tokens": 5},
             "modelUsage": {"claude-test": {"inputTokens": 10, "cacheReadInputTokens": 70,
                            "cacheCreationInputTokens": 20, "outputTokens": 5}}}
    event.update(changes)
    return event


class EventAccountingTests(unittest.TestCase):
    def collect(self, host, text, **kwargs):
        module = importlib.import_module("analysis_lite_events")
        return module.collect_cli_events(host, text, **kwargs)

    def test_codex_single_turn_keeps_cache_inclusive_and_child_scope_unconfirmed(self):
        text = jsonl({"type": "thread.started", "thread_id": "thread-1"},
                     {"type": "turn.started"}, codex_final())
        result = self.collect("codex", text, returncode=0, fresh_session=True)
        self.assertEqual(result["observed_usage"]["input_tokens"], 100)
        self.assertEqual(result["observed_usage"]["total_tokens"], 120)
        self.assertIsNone(result["provider_usage"]["total_tokens"])
        self.assertEqual(result["usage_completeness"], "unknown")
        self.assertEqual(result["raw_stdout"], text)

    def test_duplicate_codex_terminal_is_not_added(self):
        result = self.collect("codex", jsonl({"type": "turn.started"}, codex_final(), codex_final()), returncode=0)
        self.assertEqual(result["observed_usage"]["total_tokens"], 120)
        self.assertEqual(result["duplicate_usage_events"], 1)

    def test_multiple_codex_turns_are_not_assumed_delta_or_cumulative(self):
        result = self.collect("codex", jsonl({"type": "turn.started"}, codex_final(),
            {"type": "turn.started"}, codex_final(input_tokens=300)), returncode=0)
        self.assertEqual(len(result["usage_records"]), 2)
        self.assertIsNone(result["provider_usage"]["input_tokens"])
        self.assertIsNone(result["observed_usage"]["total_tokens"])
        self.assertIn("codex-multi-turn-aggregation-unverified", result["unresolved"])

    def test_conflicting_codex_terminals_in_one_turn_are_not_silently_replaced(self):
        result = self.collect("codex", jsonl({"type": "turn.started"}, codex_final(),
            codex_final(input_tokens=200)), returncode=0)
        self.assertIsNone(result["observed_usage"]["total_tokens"])
        self.assertIn("conflicting-terminal-usage", result["unresolved"])

    def test_codex_cache_write_receipt_is_mapped_without_changing_raw_usage(self):
        event = codex_final(cache_write_input_tokens=20)
        result = self.collect("codex", jsonl(event), returncode=0)
        self.assertEqual(result["observed_usage"]["cache_creation_input_tokens"], 20)
        self.assertEqual(result["observed_usage"]["uncached_input_tokens"], 10)
        self.assertEqual(result["usage_records"][0]["raw"], event)

    def test_decreasing_cumulative_claude_snapshot_cannot_be_a_cheaper_run(self):
        earlier = claude_final()
        later = claude_final(uuid="result-2", modelUsage={"claude-test": {
            "inputTokens": 1, "cacheReadInputTokens": 2,
            "cacheCreationInputTokens": 3, "outputTokens": 4}})
        result = self.collect("claude", jsonl(earlier, later), returncode=0, fresh_session=True)
        self.assertIsNone(result["provider_usage"]["total_tokens"])
        self.assertIn("nonmonotonic-cumulative-modelUsage", result["unresolved"])

    def test_claude_uses_last_cumulative_model_usage_not_assistant_or_result_usage(self):
        first = claude_final()
        last = claude_final(uuid="result-2", modelUsage={"claude-test": {
            "inputTokens": 20, "cacheReadInputTokens": 140,
            "cacheCreationInputTokens": 40, "outputTokens": 10}})
        assistant = {"type": "assistant", "message": {"id": "m1", "model": "claude-test",
            "usage": {"input_tokens": 999, "output_tokens": 999}, "content": []}}
        result = self.collect("claude", jsonl(assistant, assistant, first, last, last),
                              returncode=0, fresh_session=True)
        self.assertEqual(result["provider_usage"]["input_tokens"], 200)
        self.assertEqual(result["provider_usage"]["total_tokens"], 210)
        self.assertEqual(result["usage_completeness"], "complete")
        self.assertEqual(result["duplicate_usage_events"], 1)
        self.assertEqual(result["usage_records"][-1]["raw"], last)

    def test_claude_model_usage_includes_all_models_without_double_counting_children(self):
        final = claude_final()
        final["modelUsage"]["child-test"] = {"inputTokens": 2, "cacheReadInputTokens": 3,
                                            "cacheCreationInputTokens": 4, "outputTokens": 1}
        child = {"type": "assistant", "parent_tool_use_id": "agent-1", "message": {
            "id": "child", "model": "child-test", "usage": {"input_tokens": 2}, "content": []}}
        result = self.collect("claude", jsonl(child, final), returncode=0, fresh_session=True)
        self.assertEqual(result["provider_usage"]["total_tokens"], 115)
        self.assertEqual(result["usage_scope"], "claude-modelUsage-whole-tree")
        self.assertIsNone(result["settings"]["model"]["value"])

    def test_claude_main_loop_usage_is_not_a_whole_tree_measurement(self):
        event = claude_final()
        del event["modelUsage"]
        result = self.collect("claude", jsonl(event), returncode=0, fresh_session=True)
        self.assertEqual(result["observed_usage"]["total_tokens"], 105)
        self.assertIsNone(result["provider_usage"]["total_tokens"])
        self.assertIn("claude-main-loop-only", result["unresolved"])

    def test_resumed_or_unspecified_session_cannot_claim_current_run_totals(self):
        for fresh in (False, None):
            with self.subTest(fresh=fresh):
                result = self.collect("claude", jsonl(claude_final()), returncode=0, fresh_session=fresh)
                self.assertEqual(result["observed_usage"]["total_tokens"], 105)
                self.assertIsNone(result["provider_usage"]["total_tokens"])

    def test_conversation_reset_or_session_change_is_not_summed(self):
        result = self.collect("claude", jsonl(claude_final(),
            {"type": "system", "subtype": "conversation_reset"},
            claude_final(uuid="r2", session_id="session-2")), returncode=0, fresh_session=True)
        self.assertEqual(result["usage_completeness"], "unknown")
        self.assertIn("session-reset-or-multiple-sessions", result["unresolved"])

    def test_failed_result_preserves_usage_but_cannot_be_a_complete_cheap_run(self):
        event = claude_final(subtype="error_during_execution", is_error=True,
                             errors=["disconnected"])
        result = self.collect("claude", jsonl(event), returncode=1, fresh_session=True,
                              stderr="exact diagnostic\n")
        self.assertEqual(result["run_status"], "failed")
        self.assertEqual(result["raw_stderr"], "exact diagnostic\n")
        self.assertEqual(result["observed_usage"]["total_tokens"], 105)
        self.assertIsNone(result["provider_usage"]["total_tokens"])

    def test_truncation_malformed_lines_and_missing_usage_remain_unknown(self):
        for text in ["", "warning\n{\"type\":", jsonl({"type": "turn.failed", "error": {"message": "quota"}})]:
            with self.subTest(text=text):
                result = self.collect("codex", text, returncode=1)
                self.assertIsNone(result["provider_usage"]["total_tokens"])
                self.assertNotEqual(result["usage_completeness"], "complete")
                self.assertEqual(result["raw_stdout"], text)

    def test_codex_error_message_string_is_a_valid_error_receipt(self):
        event = {"type": "error", "message": "quota exhausted"}
        result = self.collect("codex", jsonl(event), returncode=1, fresh_session=True)
        self.assertEqual(result["malformed_lines"], [])
        self.assertEqual(result["errors"], [{"line": 1, "event": event}])
        self.assertEqual(result["run_status"], "failed")
        self.assertIsNone(result["provider_usage"]["total_tokens"])

    def test_invalid_provider_counts_and_missing_cache_components_do_not_become_zero(self):
        for values in [{"inputTokens": -1}, {"inputTokens": True}, {"inputTokens": 3, "outputTokens": 4}]:
            with self.subTest(values=values):
                result = self.collect("claude", jsonl(claude_final(modelUsage={"m": values})),
                                      returncode=0, fresh_session=True)
                self.assertIsNone(result["provider_usage"]["total_tokens"])

    def test_semantically_malformed_event_fields_cannot_crash_or_validate_usage(self):
        for event in [
            {"type": "item.started", "item": {"type": None, "id": []}},
            {"type": "item.completed", "item": {"type": "mcp_tool_call", "id": []}},
            {"type": "assistant", "parent_tool_use_id": [], "message": {}},
            {"type": "system", "subtype": "api_retry", "uuid": []},
            {"type": "assistant", "message": {"content": [{"type": "tool_use", "id": []}]}},
        ]:
            with self.subTest(event=event):
                host = "codex" if event["type"].startswith("item.") else "claude"
                result = self.collect(host, jsonl(event, claude_final() if host == "claude" else codex_final()),
                                      returncode=0, fresh_session=True)
                self.assertTrue(result["malformed_lines"])
                self.assertEqual(result["usage_completeness"], "unknown")

    def test_claude_init_selection_is_distinct_from_observed_provider_settings(self):
        result = self.collect("claude", jsonl({"type": "system", "subtype": "init",
            "model": "selected-alias", "effort": "high"}, claude_final()),
            returncode=0, fresh_session=True)
        self.assertIsNone(result["settings"]["model"]["value"])
        self.assertIsNone(result["settings"]["reasoning_effort"]["value"])
        self.assertEqual(result["settings"]["configured_model_receipts"][0]["model"], "selected-alias")

    def test_tools_retries_and_final_text_have_receipts_and_deduplication(self):
        tool = {"type": "tool_use", "id": "tool1", "name": "Read", "input": {"file_path": "source.ts"}}
        assistant = {"type": "assistant", "message": {"id": "m1", "model": "observed-model", "content": [tool]}}
        tool_result = {"type": "user", "message": {"content": [
            {"type": "tool_result", "tool_use_id": "tool1", "content": "source"}]}}
        retry = {"type": "system", "subtype": "api_retry", "uuid": "retry1", "attempt": 3}
        result = self.collect("claude", jsonl(assistant, assistant, tool_result, retry, retry, claude_final()),
                              returncode=0, fresh_session=True)
        self.assertEqual(result["tool_roundtrips"], 1)
        self.assertEqual(result["visible_retry_events"], 1)
        self.assertIsNone(result["actual_retry_attempts"])
        self.assertEqual(result["final_text"], "done")
        self.assertEqual(result["settings"]["model"]["value"], "observed-model")
        self.assertEqual(result["settings"]["model"]["source"], "assistant.message.model")
        self.assertIsNone(result["settings"]["reasoning_effort"]["value"])

    def test_codex_completed_item_counts_one_roundtrip_and_final_answer(self):
        item = {"id": "item1", "type": "mcp_tool_call", "server": "codelens", "tool": "search"}
        result = self.collect("codex", jsonl({"type": "item.started", "item": item},
            {"type": "item.completed", "item": {**item, "status": "completed"}},
            {"type": "item.completed", "item": {"id": "msg", "type": "agent_message", "text": "answer"}},
            codex_final()), returncode=0)
        self.assertEqual(result["tool_roundtrips"], 1)
        self.assertEqual(result["final_text"], "answer")
        self.assertIsNone(result["settings"]["model"]["value"])

    def test_unknown_host_is_rejected_without_io(self):
        with self.assertRaises(ValueError):
            self.collect("unsupported", "")


if __name__ == "__main__":
    unittest.main()
