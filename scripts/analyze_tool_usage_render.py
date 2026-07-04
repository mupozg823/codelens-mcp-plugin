from __future__ import annotations


def render_telemetry_report(report: dict) -> None:
    behavior = report["behavior"]
    print(f"\n{'=' * 60}")
    print("  CodeLens Agent Behavior Telemetry")
    print(f"{'=' * 60}")
    print(f"  Events             : {behavior['total_events']}")
    print(f"  Sessions           : {behavior['session_count']}")
    print(f"  Suggestion events  : {behavior['suggestion_events']}")
    print(f"  Suggestions followed: {behavior['suggestions_followed']}")
    print(f"  Suggestions missed : {behavior['suggestions_missed']}")
    print(f"  Follow rate        : {behavior['suggestion_follow_rate'] * 100:.1f}%")
    print(f"  Delegate emissions : {behavior['delegate_emissions']}")
    print(f"  Handoffs consumed  : {behavior['delegate_handoffs_consumed']}")
    print(f"  Builder tool events: {behavior['codex_builder_tool_events']}")

    if behavior["handoff_correlations"]:
        print("\n  HANDOFF CORRELATIONS:")
        for row in behavior["handoff_correlations"]:
            print(
                "      "
                f"{row['handoff_id']} "
                f"{row['emitting_session']} -> {row['consuming_session']} "
                f"via {row['consuming_tool']}"
            )

    if behavior["tool_counts"]:
        print("\n  TOP TOOLS:")
        for tool, count in behavior["tool_counts"][:10]:
            print(f"      {tool:30} {count:4}")

    if behavior["missed_label_counts"]:
        print("\n  MISSED ROUTE LABELS:")
        for label, count in behavior["missed_label_counts"]:
            print(f"      {label:30} {count:4}")

    if behavior["missed_branch_counts"]:
        print("\n  MISSED AGENT BRANCHES:")
        for branch, count in behavior["missed_branch_counts"]:
            print(f"      {branch:30} {count:4}")

    if behavior["missed_transfer_by_label"]:
        print("\n  MISSED TRANSFER COST BY LABEL:")
        for row in behavior["missed_transfer_by_label"]:
            print(
                "      "
                f"{row['route_label']:30} "
                f"avg~{row['avg_estimated_tokens']:5} tok "
                f"max~{row['max_estimated_tokens']:5} tok "
                f"tools={row['avg_tool_count']:.1f} "
                f"expensive={row['expensive_count']} "
                f"overflow={row['overflow_count']}"
            )

    if behavior["missed_transfer_by_branch"]:
        print("\n  MISSED TRANSFER COST BY BRANCH:")
        for row in behavior["missed_transfer_by_branch"]:
            print(
                "      "
                f"{row['agent_branch']:30} "
                f"avg~{row['avg_estimated_tokens']:5} tok "
                f"max~{row['max_estimated_tokens']:5} tok "
                f"tools={row['avg_tool_count']:.1f} "
                f"expensive={row['expensive_count']} "
                f"overflow={row['overflow_count']}"
            )

    if behavior["missed_suggestions"]:
        print("\n  MISSED SUGGESTIONS:")
        for row in behavior["missed_suggestions"]:
            next_external = row.get("next_external_tools") or []
            transfer = row.get("external_transfer") or {}
            branch = row.get("agent_branch") or "unknown"
            external_suffix = (
                f" | external: {', '.join(next_external)}" if next_external else ""
            )
            transfer_suffix = (
                f" | ~{transfer['estimated_tokens']} tok {transfer['efficiency_band']}"
                if transfer
                else ""
            )
            print(
                "      "
                f"{row['session_id']}:{row['tool']} "
                f"[{row['route_label']}/{branch}] -> "
                f"{', '.join(row['suggested_next_tools'])}"
                f"{external_suffix}"
                f"{transfer_suffix}"
            )

    print(f"{'=' * 60}\n")
