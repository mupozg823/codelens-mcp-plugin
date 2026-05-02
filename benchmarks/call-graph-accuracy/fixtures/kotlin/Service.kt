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

class Service {
    fun onTick() {}

    fun onError(msg: String) {}

    fun prepare() {}

    fun start(exec: Executor, bus: Bus) {
        exec.submit(onTick)
        bus.register("err", onError)
        prepare()
    }
}
