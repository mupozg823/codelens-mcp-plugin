// Call-graph accuracy fixture (Rust).
// Patterns:
//   - direct call: handler() inside run()
//   - method chain: builder.with_x().with_y()
//   - macro:        my_log!()
//   - function reference (v1.11.0+): LazyLock::new(build_tools), iter.map(parse_line)

fn build_tools() -> Vec<u32> {
    vec![1, 2, 3]
}

fn parse_line(s: &str) -> u32 {
    s.len() as u32
}

fn handler() {
    let _ = build_tools();
}

macro_rules! my_log {
    ($($t:tt)*) => {};
}

static TOOLS: std::sync::LazyLock<Vec<u32>> = std::sync::LazyLock::new(build_tools);

fn run() {
    handler();
    my_log!("hi");
    let lines = ["a", "bb"];
    let _: Vec<_> = lines.iter().map(parse_line).collect();
}
