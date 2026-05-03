// Call-graph accuracy fixture (Kotlin).
// v1.12.3: dedicated KOTLIN_CALL_QUERY targets tree-sitter-kotlin-ng's
// (call_expression / navigation_expression / value_arguments) shape — the
// previously-shared JAVA_CALL_QUERY produced 0 edges because
// tree-sitter-java uses completely different node names (method_invocation,
// argument_list, ...). See docs/eval/v1.12.1-post-release-eval.md.
//
// Patterns covered:
//   - direct call:        prepare()
//   - method invocation:  exec.submit(...)
//   - function reference: exec.submit(onTick), bus.register("err", onError)
//   - callable reference (v1.12.4): exec.submit(::onTick),
//                                   bus.register("err", ::onError),
//                                   exec.submit(this::onTick)

class Service {
    fun onTick() {}

    fun onError(msg: String) {}

    fun prepare() {}

    // v1.12.4: regression guards — these symbols are reachable ONLY via
    // callable references; if KOTLIN_CALL_QUERY's callable_reference /
    // qualified-navigation patterns are removed, the bench loses these
    // edges and F1 drops below threshold.
    fun handleAll() {}

    fun shutdown() {}

    fun start(exec: Executor, bus: Bus) {
        exec.submit(onTick)
        bus.register("err", onError)
        prepare()

        // v1.12.4 callable-reference forms (Codex P1 follow-up)
        exec.submit(::handleAll)
        bus.register("err", ::onError)
        exec.submit(this::shutdown)
    }
}
