use crate::{
    AppState,
    error::CodeLensError,
    protocol::BackendKind,
    tools::{self, ToolResult},
};
use serde_json::json;

pub(in crate::dispatch) fn index_embeddings_handler(
    state: &AppState,
    arguments: &serde_json::Value,
) -> ToolResult {
    let background = tools::optional_bool(arguments, "background", false);
    if background {
        return queue_index_embeddings_job(state, arguments);
    }

    Ok((
        index_embeddings_now(state, arguments)?,
        tools::success_meta(BackendKind::Semantic, 0.95),
    ))
}

fn queue_index_embeddings_job(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let scope = state.current_project_scope();
    let job = state.store_analysis_job(
        &scope,
        "index_embeddings",
        None,
        vec!["semantic_index".to_owned(), "query_prewarm".to_owned()],
        crate::runtime_types::JobLifecycle::Queued,
        0,
        Some("queued".to_owned()),
        None,
        None,
    )?;
    let job_id = job.id.clone();
    let response_job_id = job_id.clone();
    let worker_state = state.clone_for_worker();
    let arguments = arguments.clone();
    std::thread::spawn(move || run_index_embeddings_job(worker_state, job_id, arguments));

    Ok((
        json!({
            "background": true,
            "status": "queued",
            "job": job,
            "poll": {
                "tool": "get_analysis_job",
                "arguments": { "job_id": response_job_id }
            }
        }),
        tools::success_meta(BackendKind::Semantic, 0.90),
    ))
}

fn run_index_embeddings_job(worker_state: AppState, job_id: String, arguments: serde_json::Value) {
    let scope = worker_state.current_project_scope();
    let _ = worker_state.update_analysis_job(
        &scope,
        &job_id,
        Some(crate::runtime_types::JobLifecycle::Running),
        Some(10),
        Some(Some("indexing semantic embeddings".to_owned())),
        None,
        None,
        None,
    );
    match index_embeddings_now(&worker_state, &arguments) {
        Ok(payload) => {
            let current_step = payload
                .get("indexed_symbols")
                .and_then(|value| value.as_u64())
                .map(|count| format!("indexed {count} symbols"))
                .unwrap_or_else(|| "completed".to_owned());
            let _ = worker_state.update_analysis_job(
                &scope,
                &job_id,
                Some(crate::runtime_types::JobLifecycle::Completed),
                Some(100),
                Some(Some(current_step)),
                None,
                None,
                None,
            );
        }
        Err(error) => {
            let _ = worker_state.update_analysis_job(
                &scope,
                &job_id,
                Some(crate::runtime_types::JobLifecycle::Error),
                Some(100),
                Some(Some("failed".to_owned())),
                None,
                None,
                Some(Some(error.to_string())),
            );
        }
    }
}

fn index_embeddings_now(
    state: &AppState,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, CodeLensError> {
    let project = state.project();
    let guard = state.embedding_engine();
    let engine = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Embedding engine not available"))?;

    let count = engine.index_from_project(&project)?;
    let bridges_generated = match engine.generate_bridge_candidates(&project) {
        Ok(bridges) if !bridges.is_empty() => {
            let bridges_dir = project.as_path().join(".codelens");
            let _ = std::fs::create_dir_all(&bridges_dir);
            let json_entries: Vec<serde_json::Value> = bridges
                .iter()
                .map(|(nl, code)| json!({"nl": nl, "code": code}))
                .collect();
            let _ = std::fs::write(
                bridges_dir.join("bridges.json"),
                serde_json::to_string_pretty(&json_entries).unwrap_or_default(),
            );
            bridges.len()
        }
        _ => 0,
    };

    let prewarmed = prewarm_embedding_queries(state, engine, arguments)?;
    let query_cache = engine.query_cache_stats()?;

    Ok(json!({
        "indexed_symbols": count,
        "bridges_generated": bridges_generated,
        "query_cache": {
            "enabled": query_cache.enabled,
            "entries": query_cache.entries,
            "max_entries": query_cache.max_entries,
            "prewarmed": prewarmed
        },
        "status": "ok"
    }))
}

fn prewarm_embedding_queries(
    state: &AppState,
    engine: &codelens_engine::EmbeddingEngine,
    arguments: &serde_json::Value,
) -> Result<usize, CodeLensError> {
    let limit = arguments
        .get("prewarm_limit")
        .and_then(|value| value.as_u64())
        .unwrap_or(128)
        .min(1024) as usize;
    if limit == 0 {
        return Ok(0);
    }
    let Some(queries) = arguments
        .get("prewarm_queries")
        .and_then(|value| value.as_array())
    else {
        return Ok(0);
    };
    let semantic_queries = queries
        .iter()
        .filter_map(|value| value.as_str())
        .map(str::trim)
        .filter(|query| !query.is_empty())
        .take(limit)
        .map(|query| {
            let query_analysis = crate::tools::query_analysis::analyze_retrieval_query(query);
            crate::tools::query_analysis::semantic_query_for_embedding_search(
                &query_analysis,
                Some(state.project().as_path()),
            )
        })
        .collect::<Vec<_>>();
    Ok(engine.prewarm_queries(&semantic_queries)?)
}
