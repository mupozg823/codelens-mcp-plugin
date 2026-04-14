use super::*;
use serde_json::json;
use std::time::Instant;

fn write_scip_fixture(project: &ProjectRoot) {
    fs::create_dir_all(project.as_path().join("src")).unwrap();
    fs::write(
        project.as_path().join("src/main.rs"),
        "pub struct MyStruct;\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("src/lib.rs"),
        "use crate::MyStruct;\n",
    )
    .unwrap();
    write_test_scip_index(project);
}

fn measure_ms<F>(mut f: F) -> f64
where
    F: FnMut(),
{
    let start = Instant::now();
    f();
    start.elapsed().as_secs_f64() * 1000.0
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

#[test]
#[ignore = "benchmark"]
fn benchmark_scip_tool_paths() {
    let project_a = project_root();
    let project_b = project_root();
    write_scip_fixture(&project_a);
    write_scip_fixture(&project_b);

    let state = make_state(&project_a);

    let cold_find_symbol_ms = measure_ms(|| {
        let payload = call_tool(
            &state,
            "find_symbol",
            json!({ "name": "MyStruct", "file_path": "src/main.rs", "max_matches": 5 }),
        );
        assert_eq!(payload["success"], json!(true));
        assert_eq!(payload["data"]["backend"], json!("scip"));
    });

    let warm_find_symbol_runs = (0..25)
        .map(|_| {
            measure_ms(|| {
                let payload = call_tool(
                    &state,
                    "find_symbol",
                    json!({ "name": "MyStruct", "file_path": "src/main.rs", "max_matches": 5 }),
                );
                assert_eq!(payload["success"], json!(true));
                assert_eq!(payload["data"]["backend"], json!("scip"));
            })
        })
        .collect::<Vec<_>>();

    let warm_references_runs = (0..25)
        .map(|_| {
            measure_ms(|| {
                let payload = call_tool(
                    &state,
                    "find_referencing_symbols",
                    json!({ "file_path": "src/main.rs", "symbol_name": "MyStruct", "max_results": 10 }),
                );
                assert_eq!(payload["success"], json!(true));
                assert_eq!(payload["data"]["backend"], json!("scip"));
            })
        })
        .collect::<Vec<_>>();

    let warm_diagnostics_runs = (0..25)
        .map(|_| {
            measure_ms(|| {
                let payload = call_tool(
                    &state,
                    "get_file_diagnostics",
                    json!({ "file_path": "src/main.rs", "max_results": 10 }),
                );
                assert_eq!(payload["success"], json!(true));
                assert_eq!(payload["data"]["backend"], json!("scip"));
            })
        })
        .collect::<Vec<_>>();

    state
        .switch_project(project_b.as_path().to_str().unwrap())
        .unwrap();

    let post_switch_find_symbol_ms = measure_ms(|| {
        let payload = call_tool(
            &state,
            "find_symbol",
            json!({ "name": "MyStruct", "file_path": "src/main.rs", "max_matches": 5 }),
        );
        assert_eq!(payload["success"], json!(true));
        assert_eq!(payload["data"]["backend"], json!("scip"));
    });

    let report = json!({
        "find_symbol": {
            "cold_ms": cold_find_symbol_ms,
            "warm_avg_ms": mean(&warm_find_symbol_runs),
            "warm_min_ms": warm_find_symbol_runs.iter().copied().fold(f64::INFINITY, f64::min),
            "warm_max_ms": warm_find_symbol_runs.iter().copied().fold(0.0, f64::max),
            "post_switch_cold_ms": post_switch_find_symbol_ms,
        },
        "find_referencing_symbols": {
            "warm_avg_ms": mean(&warm_references_runs),
            "warm_min_ms": warm_references_runs.iter().copied().fold(f64::INFINITY, f64::min),
            "warm_max_ms": warm_references_runs.iter().copied().fold(0.0, f64::max),
        },
        "get_file_diagnostics": {
            "warm_avg_ms": mean(&warm_diagnostics_runs),
            "warm_min_ms": warm_diagnostics_runs.iter().copied().fold(f64::INFINITY, f64::min),
            "warm_max_ms": warm_diagnostics_runs.iter().copied().fold(0.0, f64::max),
        }
    });

    eprintln!("SCIP_BENCHMARK {}", report);
}
