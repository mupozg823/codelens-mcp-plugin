// Call-graph accuracy fixture (Java).
// Patterns:
//   - direct call:        process(payload) inside start
//   - method_reference:   Service::onTick (covered earlier)
//   - function reference (v1.11.2+): exec.submit(onTick), bus.register("err", onError)

class Service {
    public void onTick() {}

    public void onError(String e) {}

    public void process(String payload) {}

    public void start(Executor exec, Bus bus) {
        exec.submit(onTick);
        bus.register("err", onError);
        process("seed");
    }
}
