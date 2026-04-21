use crate::AppState;
use codelens_engine::{LSP_RECIPES, LspRecipe, LspWorkspaceSymbolRequest, lsp_binary_exists};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;

const LSP_AUTO_ATTACH_SAMPLE_LIMIT: usize = 800;
const LSP_AUTO_ATTACH_MAX_DEPTH: usize = 4;

const LSP_AUTO_ATTACH_SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    "venv",
    ".venv",
    "__pycache__",
    ".idea",
    ".vscode",
    ".codelens",
    "external-repos",
];

fn shallow_sample_files(root: &Path, limit: usize, max_depth: usize) -> Vec<std::path::PathBuf> {
    let mut out = Vec::with_capacity(limit);
    let mut stack: Vec<(std::path::PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        if out.len() >= limit || depth > max_depth {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if out.len() >= limit {
                break;
            }
            let path = entry.path();
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                if depth + 1 > max_depth {
                    continue;
                }
                let name_str = entry.file_name().to_string_lossy().to_string();
                if name_str.starts_with('.') && name_str != "." {
                    continue;
                }
                if LSP_AUTO_ATTACH_SKIP_DIRS.iter().any(|s| *s == name_str) {
                    continue;
                }
                stack.push((path, depth + 1));
            } else if ft.is_file() && path.extension().is_some() {
                out.push(path);
            }
        }
    }
    out
}

pub(super) fn auto_attach_lsp_prewarm(state: &AppState) -> Value {
    let opt_env = std::env::var("CODELENS_LSP_AUTO").ok();
    let opt_out = opt_env.as_deref() == Some("false");
    let opt_in = opt_env.as_deref() == Some("true");
    if opt_out {
        return json!({
            "enabled": false,
            "disabled_reason": "user_opt_out",
            "detected_languages": [],
            "prewarm_fired": [],
        });
    }

    let transport_is_http = matches!(
        state.transport_mode(),
        crate::runtime_types::RuntimeTransportMode::Http
    );
    if !opt_in && !transport_is_http {
        return json!({
            "enabled": false,
            "disabled_reason": "non_persistent_transport",
            "detected_languages": [],
            "prewarm_fired": [],
        });
    }

    let project = state.project();
    let sample = shallow_sample_files(
        project.as_path(),
        LSP_AUTO_ATTACH_SAMPLE_LIMIT,
        LSP_AUTO_ATTACH_MAX_DEPTH,
    );

    let mut lang_present: HashMap<&'static str, &'static LspRecipe> = HashMap::new();
    for file in &sample {
        let Some(ext) = file.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        for recipe in LSP_RECIPES {
            if recipe.extensions.contains(&ext) {
                lang_present.entry(recipe.language).or_insert(recipe);
                break;
            }
        }
    }

    let mut detected: Vec<String> = lang_present.keys().map(|s| (*s).to_owned()).collect();
    detected.sort();

    let mut prewarm_fired: Vec<String> = Vec::new();
    for (lang, recipe) in &lang_present {
        if !lsp_binary_exists(recipe.binary_name) {
            continue;
        }
        let pool = state.lsp_pool();
        let command = recipe.binary_name.to_owned();
        let args: Vec<String> = recipe.args.iter().map(|s| (*s).to_owned()).collect();
        std::thread::spawn(move || {
            let _ = pool.search_workspace_symbols(&LspWorkspaceSymbolRequest {
                command,
                args,
                query: String::new(),
                max_results: 1,
            });
        });
        prewarm_fired.push((*lang).to_owned());
    }
    prewarm_fired.sort();

    json!({
        "enabled": true,
        "disabled_reason": Value::Null,
        "detected_languages": detected,
        "prewarm_fired": prewarm_fired,
    })
}
