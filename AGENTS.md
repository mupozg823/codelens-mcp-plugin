# AGENTS.md

- Prefer root-cause fixes over superficial patches.
- Before editing, inspect the existing implementation, tests, and IntelliJ platform constraints.
- Run the smallest relevant test first, then broaden validation only as needed.
- Keep diffs minimal and avoid wrappers or abstractions unless they remove real duplication.
- Preserve existing plugin and MCP compatibility behavior unless the task explicitly changes it.
- For Kotlin/Gradle work, validate with `./gradlew test`; use `buildPlugin` or `verifyPlugin` only when the change reaches packaging or platform compatibility.
- For transport or bridge changes, run `python3 test-mcp-tools.py` after the smallest relevant unit test.
- Ask first before destructive commands, wide refactors, dependency upgrades, or touching files outside the current task.
