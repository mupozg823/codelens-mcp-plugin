use super::graph::{build_graph, compute_pagerank};
use super::types::FileNode;
use crate::project::ProjectRoot;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub struct GraphCache {
    inner: Mutex<GraphCacheInner>,
    generation: AtomicU64,
}

struct GraphCacheInner {
    graph: Option<Arc<HashMap<String, FileNode>>>,
    built_generation: u64,
}

impl GraphCache {
    pub fn new(_ttl_secs: u64) -> Self {
        Self {
            inner: Mutex::new(GraphCacheInner {
                graph: None,
                built_generation: 0,
            }),
            generation: AtomicU64::new(1),
        }
    }

    pub fn get_or_build(&self, project: &ProjectRoot) -> Result<Arc<HashMap<String, FileNode>>> {
        let current_gen = self.generation.load(Ordering::Acquire);
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| anyhow::anyhow!("graph cache lock poisoned"))?;
        if let Some(graph) = &inner.graph
            && inner.built_generation == current_gen
        {
            return Ok(Arc::clone(graph));
        }
        let graph = Arc::new(build_graph(project)?);
        inner.graph = Some(Arc::clone(&graph));
        inner.built_generation = current_gen;
        Ok(graph)
    }

    pub fn file_pagerank_scores(&self, project: &ProjectRoot) -> HashMap<String, f64> {
        let graph = match self.get_or_build(project) {
            Ok(graph) => graph,
            Err(_) => return HashMap::new(),
        };
        compute_pagerank(&graph)
    }

    pub fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::Release);
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }
}
