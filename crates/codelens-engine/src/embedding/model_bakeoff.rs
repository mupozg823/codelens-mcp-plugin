use anyhow::{Context, Result};
use fastembed::TextEmbedding;

use super::EmbeddingRuntimeInfo;
use super::runtime_info::cpu_runtime_info;
use super::runtime_settings::configured_embedding_max_length;

pub fn load_fastembed_builtin(
    model_id: &str,
) -> Result<(TextEmbedding, usize, String, EmbeddingRuntimeInfo)> {
    use fastembed::EmbeddingModel;

    let (model_enum, expected_dim) = match model_id {
        "all-MiniLM-L6-v2" | "sentence-transformers/all-MiniLM-L6-v2" => {
            (EmbeddingModel::AllMiniLML6V2, 384)
        }
        "all-MiniLM-L12-v2" | "sentence-transformers/all-MiniLM-L12-v2" => {
            (EmbeddingModel::AllMiniLML12V2, 384)
        }
        "bge-small-en-v1.5" | "BAAI/bge-small-en-v1.5" => (EmbeddingModel::BGESmallENV15, 384),
        "bge-base-en-v1.5" | "BAAI/bge-base-en-v1.5" => (EmbeddingModel::BGEBaseENV15, 768),
        "nomic-embed-text-v1.5" | "nomic-ai/nomic-embed-text-v1.5" => {
            (EmbeddingModel::NomicEmbedTextV15, 768)
        }
        "jina-embeddings-v2-base-code" | "jinaai/jina-embeddings-v2-base-code" => {
            (EmbeddingModel::JinaEmbeddingsV2BaseCode, 768)
        }
        other => {
            anyhow::bail!(
                "Unknown fastembed model: {other}. \
                 Supported: all-MiniLM-L6-v2, all-MiniLM-L12-v2, bge-small-en-v1.5, \
                 bge-base-en-v1.5, nomic-embed-text-v1.5, jina-embeddings-v2-base-code"
            );
        }
    };

    let init = fastembed::InitOptionsWithLength::new(model_enum)
        .with_max_length(configured_embedding_max_length())
        .with_cache_dir(std::env::temp_dir().join("codelens-fastembed-cache"))
        .with_show_download_progress(true);
    let model =
        TextEmbedding::try_new(init).with_context(|| format!("failed to load {model_id}"))?;

    let runtime_info = cpu_runtime_info("cpu".to_string(), None);

    tracing::info!(
        model = model_id,
        dimension = expected_dim,
        "loaded fastembed built-in model for A/B comparison"
    );

    Ok((model, expected_dim, model_id.to_string(), runtime_info))
}
