use crate::{
    AppState,
    error::CodeLensError,
    protocol::BackendKind,
    tools::{self, ToolResult},
};
use serde_json::json;

pub(crate) fn index_embeddings_handler(
    state: &AppState,
    arguments: &serde_json::Value,
) -> ToolResult {
    let background = tools::optional_bool(arguments, "background", true);
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
    let mut queued_arguments = arguments.clone();
    if let Some(object) = queued_arguments.as_object_mut() {
        object.insert("_job_id".to_owned(), json!(job.id));
        object.insert("_project_scope".to_owned(), json!(scope));
    }
    state.enqueue_analysis_job(
        scope,
        job.id.clone(),
        "index_embeddings".to_owned(),
        queued_arguments,
        None,
    )?;

    Ok((
        json!({
            "background": true,
            "status": "queued",
            "job": job,
            "poll": {
                "tool": "get_analysis_job",
                "arguments": { "job_id": job.id }
            }
        }),
        tools::success_meta(BackendKind::Semantic, 0.90),
    ))
}

pub(crate) fn index_embeddings_now(
    state: &AppState,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, CodeLensError> {
    let project = state.project();
    let guard = state.embedding_engine();
    let engine = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Embedding engine not available"))?;

    let count = match (
        arguments.get("_job_id").and_then(|value| value.as_str()),
        arguments
            .get("_project_scope")
            .and_then(|value| value.as_str()),
    ) {
        (Some(job_id), Some(scope)) => {
            let mut last_heartbeat_ms = 0;
            engine.index_from_project_with_checkpoint(&project, |scanned_symbols| {
                checkpoint_embedding_job(
                    state,
                    scope,
                    job_id,
                    scanned_symbols,
                    &mut last_heartbeat_ms,
                )
            })?
        }
        _ => engine.index_from_project(&project)?,
    };
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

fn checkpoint_embedding_job(
    state: &AppState,
    scope: &str,
    job_id: &str,
    scanned_symbols: usize,
    last_heartbeat_ms: &mut u64,
) -> anyhow::Result<()> {
    let job = state
        .get_analysis_job_for_scope(scope, job_id)
        .ok_or_else(|| anyhow::anyhow!("unknown semantic indexing job `{job_id}`"))?;
    match job.status {
        crate::runtime_types::JobLifecycle::Cancelled => {
            anyhow::bail!("semantic indexing job `{job_id}` was cancelled")
        }
        crate::runtime_types::JobLifecycle::Error => {
            anyhow::bail!(
                "semantic indexing job `{job_id}` stopped: {}",
                job.error.as_deref().unwrap_or("job failed")
            )
        }
        _ => {}
    }

    let now_ms = crate::util::now_ms();
    if job
        .deadline_at_ms
        .is_some_and(|deadline| now_ms >= deadline)
    {
        state.update_analysis_job(
            scope,
            job_id,
            Some(crate::runtime_types::JobLifecycle::Error),
            Some(100),
            Some(Some("deadline exceeded".to_owned())),
            None,
            None,
            Some(Some("semantic indexing deadline exceeded".to_owned())),
        )?;
        anyhow::bail!("semantic indexing job `{job_id}` deadline exceeded");
    }

    let heartbeat_interval_ms = crate::env_compat::env_var_u64("CODELENS_JOB_HEARTBEAT_SECS")
        .unwrap_or(5)
        .max(1)
        .saturating_mul(1000);
    if *last_heartbeat_ms == 0 || now_ms.saturating_sub(*last_heartbeat_ms) >= heartbeat_interval_ms
    {
        state.update_analysis_job(
            scope,
            job_id,
            Some(crate::runtime_types::JobLifecycle::Running),
            None,
            Some(Some(format!(
                "indexing embeddings ({scanned_symbols} symbols scanned)"
            ))),
            None,
            None,
            None,
        )?;
        *last_heartbeat_ms = now_ms;
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use codelens_engine::ProjectRoot;

    #[test]
    fn embedding_checkpoint_honors_job_cancellation() {
        let dir = std::env::temp_dir().join(format!(
            "codelens-embedding-cancel-{}",
            crate::util::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("sample.rs"), "fn sample() {}\n").unwrap();
        let project = ProjectRoot::new_exact(&dir).unwrap();
        let state = AppState::new_minimal(project, crate::tool_defs::ToolPreset::Full);
        let scope = state.current_project_scope();
        let job = state
            .store_analysis_job(
                &scope,
                "index_embeddings",
                None,
                Vec::new(),
                crate::runtime_types::JobLifecycle::Running,
                5,
                Some("worker started".to_owned()),
                None,
                None,
            )
            .unwrap();
        state
            .cancel_analysis_job_for_scope(&scope, &job.id)
            .unwrap();

        let error = checkpoint_embedding_job(&state, &scope, &job.id, 10, &mut 0)
            .expect_err("cancelled job must stop indexing");

        assert!(error.to_string().contains("cancelled"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
