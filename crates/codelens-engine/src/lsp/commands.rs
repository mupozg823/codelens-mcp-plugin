use super::registry::{LSP_RECIPES, resolve_lsp_binary};
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// Compatibility inventory for callers that display supported server names.
/// Enforcement uses `LSP_RECIPES`; a regression test keeps this view in sync.
pub(super) const ALLOWED_COMMANDS: &[&str] = &[
    "pyright-langserver",
    "typescript-language-server",
    "rust-analyzer",
    "gopls",
    "jdtls",
    "kotlin-language-server",
    "clangd",
    "solargraph",
    "intelephense",
    "metals",
    "sourcekit-lsp",
    "csharp-ls",
    "dart",
    "lua-language-server",
    "zls",
    "nextls",
    "haskell-language-server-wrapper",
    "ocamllsp",
    "erlang_ls",
    "R",
    "bash-language-server",
    "julia",
    "terraform-ls",
    "yaml-language-server",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ValidatedLspInvocation {
    recipe_binary: &'static str,
    executable: PathBuf,
    args: Vec<String>,
}

impl ValidatedLspInvocation {
    pub(super) fn recipe_binary(&self) -> &'static str {
        self.recipe_binary
    }

    pub(super) fn executable(&self) -> &Path {
        &self.executable
    }

    pub(super) fn args(&self) -> &[String] {
        &self.args
    }
}

#[derive(Debug, Default)]
pub(super) struct LspLaunchPolicy {
    trusted_binaries: RwLock<HashMap<&'static str, PathBuf>>,
}

impl Clone for LspLaunchPolicy {
    fn clone(&self) -> Self {
        let trusted_binaries = self
            .trusted_binaries
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        Self {
            trusted_binaries: RwLock::new(trusted_binaries),
        }
    }
}

impl LspLaunchPolicy {
    pub(super) fn from_environment() -> Self {
        let trusted_binaries = LSP_RECIPES
            .iter()
            .filter_map(|recipe| {
                resolve_lsp_binary(recipe.binary_name)
                    .and_then(|path| path.canonicalize().ok())
                    .map(|path| (recipe.binary_name, path))
            })
            .collect();
        Self {
            trusted_binaries: RwLock::new(trusted_binaries),
        }
    }

    pub(super) fn register_trusted_binary(
        &self,
        command: &str,
        executable: &Path,
    ) -> Result<PathBuf> {
        let recipe = recipe_for_binary(command)
            .ok_or_else(|| anyhow::anyhow!("'{command}' is not a registered LSP server recipe"))?;
        let canonical = executable
            .canonicalize()
            .with_context(|| format!("trusted LSP binary not found: {}", executable.display()))?;
        if !canonical.is_file() {
            bail!("trusted LSP binary is not a file: {}", canonical.display());
        }
        self.trusted_binaries
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(recipe.binary_name, canonical.clone());
        Ok(canonical)
    }

    pub(super) fn trusted_binary(&self, command: &str) -> Option<PathBuf> {
        let recipe = recipe_for_binary(command)?;
        self.trusted_binaries
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(recipe.binary_name)
            .cloned()
    }

    fn trusted_recipe_for_path(
        &self,
        executable: &Path,
    ) -> Option<(&'static super::registry::LspRecipe, PathBuf)> {
        let trusted = self
            .trusted_binaries
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        LSP_RECIPES.iter().find_map(|recipe| {
            trusted
                .get(recipe.binary_name)
                .filter(|path| path.as_path() == executable)
                .cloned()
                .map(|path| (recipe, path))
        })
    }
}

/// Validate an LSP launch without spawning a process.
///
/// The recipe table is the command-and-argument authority. Executable paths
/// come only from the daemon environment captured by [`LspLaunchPolicy`] or
/// from an explicit host registration; repository-local shims and
/// caller-selected paths are not trusted implicitly.
pub(super) fn validate_lsp_invocation(
    policy: &LspLaunchPolicy,
    command: &str,
    args: &[String],
) -> Result<ValidatedLspInvocation> {
    let command_path = Path::new(command);
    let (recipe, trusted) = if command_path.components().count() > 1 {
        let supplied = command_path
            .canonicalize()
            .with_context(|| format!("caller-supplied LSP path does not exist: {command}"))?;
        policy.trusted_recipe_for_path(&supplied).ok_or_else(|| {
            anyhow::anyhow!(
                "Blocked: caller-supplied LSP path '{}' is not a trusted registered recipe",
                supplied.display()
            )
        })?
    } else {
        let recipe = recipe_for_binary(command)
            .ok_or_else(|| anyhow::anyhow!("'{command}' is not a registered LSP server recipe"))?;
        let trusted = policy.trusted_binary(recipe.binary_name).ok_or_else(|| {
            anyhow::anyhow!(
                "Blocked: '{}' has no trusted executable configured in the daemon environment",
                recipe.binary_name
            )
        })?;
        (recipe, trusted)
    };
    if !recipe
        .args
        .iter()
        .copied()
        .eq(args.iter().map(String::as_str))
    {
        bail!(
            "Blocked: arguments for '{}' must exactly match its registered recipe: {:?}",
            recipe.binary_name,
            recipe.args
        );
    }

    Ok(ValidatedLspInvocation {
        recipe_binary: recipe.binary_name,
        executable: trusted,
        args: recipe.args.iter().map(|arg| (*arg).to_owned()).collect(),
    })
}

fn recipe_for_binary(command: &str) -> Option<&'static super::registry::LspRecipe> {
    LSP_RECIPES
        .iter()
        .find(|recipe| recipe.binary_name == command)
}

/// Return whether `command` is a bare, registered LSP server recipe.
pub(super) fn is_allowed_lsp_command(command: &str) -> bool {
    recipe_for_binary(command).is_some()
}
