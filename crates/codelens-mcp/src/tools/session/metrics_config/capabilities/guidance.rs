use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Copy)]
struct GuidanceDescriptor {
    status_key: &'static str,
    available: bool,
    reason: Option<&'static str>,
    reason_code: Option<&'static str>,
    recommended_action: Option<&'static str>,
    action_target: Option<&'static str>,
}

impl GuidanceDescriptor {
    fn guidance_payload(self) -> Value {
        json!({
            "status": self.status_key,
            "available": self.available,
            "reason": self.reason,
            "reason_code": self.reason_code,
            "recommended_action": self.recommended_action,
            "action_target": self.action_target,
        })
    }

    fn unavailable_payload(self, feature: &str) -> Value {
        json!({
            "feature": feature,
            "reason": self.reason.unwrap_or("available"),
            "status": self.status_key,
            "reason_code": self.reason_code,
            "recommended_action": self.recommended_action,
            "action_target": self.action_target,
        })
    }
}

fn append_field<T>(payload: &mut Map<String, Value>, key: &str, value: Option<T>)
where
    T: Into<Value>,
{
    payload.insert(key.to_owned(), value.map(Into::into).unwrap_or(Value::Null));
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DiagnosticsStatus {
    Available,
    FilePathRequired,
    UnsupportedExtension,
    LspBinaryMissing,
}

impl DiagnosticsStatus {
    pub(crate) fn is_available(&self) -> bool {
        self.guidance().available
    }

    fn guidance(&self) -> GuidanceDescriptor {
        match self {
            Self::Available => GuidanceDescriptor {
                status_key: "available",
                available: true,
                reason: None,
                reason_code: None,
                recommended_action: None,
                action_target: None,
            },
            Self::FilePathRequired => GuidanceDescriptor {
                status_key: "file_path_required",
                available: false,
                reason: Some(
                    "file_path required — provide a concrete source file so CodeLens can select an LSP recipe",
                ),
                reason_code: Some("diagnostics_file_path_required"),
                recommended_action: Some("provide_file_path"),
                action_target: Some("file_path"),
            },
            Self::UnsupportedExtension => GuidanceDescriptor {
                status_key: "unsupported_extension",
                available: false,
                reason: Some(
                    "unsupported extension — no default LSP recipe is registered for this file type",
                ),
                reason_code: Some("diagnostics_unsupported_extension"),
                recommended_action: Some("pass_explicit_lsp_command"),
                action_target: Some("file_extension"),
            },
            Self::LspBinaryMissing => GuidanceDescriptor {
                status_key: "lsp_binary_missing",
                available: false,
                reason: Some(
                    "LSP binary missing — install the configured server or provide an explicit command",
                ),
                reason_code: Some("diagnostics_lsp_binary_missing"),
                recommended_action: Some("install_lsp_server"),
                action_target: Some("lsp_server"),
            },
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DiagnosticsGuidance {
    pub(crate) status: DiagnosticsStatus,
    file_extension: Option<String>,
    language: Option<&'static str>,
    lsp_command: Option<&'static str>,
    server_name: Option<&'static str>,
    install_command: Option<&'static str>,
    package_manager: Option<&'static str>,
}

impl DiagnosticsGuidance {
    pub(crate) fn for_file(file_path: Option<&str>) -> Self {
        let extension = file_path.and_then(|path| {
            std::path::Path::new(path)
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.to_ascii_lowercase())
        });
        let recipe = extension
            .as_deref()
            .and_then(codelens_engine::get_lsp_recipe);

        let status = match (file_path, recipe) {
            (None, _) => DiagnosticsStatus::FilePathRequired,
            (Some(_), None) => DiagnosticsStatus::UnsupportedExtension,
            (Some(_), Some(recipe)) if !codelens_engine::lsp_binary_exists(recipe.binary_name) => {
                DiagnosticsStatus::LspBinaryMissing
            }
            (Some(_), Some(_)) => DiagnosticsStatus::Available,
        };

        Self {
            status,
            file_extension: extension,
            language: recipe.map(|recipe| recipe.language),
            lsp_command: recipe.map(|recipe| recipe.binary_name),
            server_name: recipe.map(|recipe| recipe.server_name),
            install_command: recipe.map(|recipe| recipe.install_command),
            package_manager: recipe.map(|recipe| recipe.package_manager),
        }
    }

    fn guidance(&self) -> GuidanceDescriptor {
        self.status.guidance()
    }

    fn append_details(&self, payload: &mut Map<String, Value>) {
        append_field(payload, "file_extension", self.file_extension.clone());
        append_field(payload, "language", self.language);
        append_field(payload, "lsp_command", self.lsp_command);
        append_field(payload, "server_name", self.server_name);
        append_field(payload, "install_command", self.install_command);
        append_field(payload, "package_manager", self.package_manager);
    }

    pub(crate) fn guidance_payload(&self) -> serde_json::Value {
        let mut payload = self
            .guidance()
            .guidance_payload()
            .as_object()
            .cloned()
            .unwrap_or_default();
        self.append_details(&mut payload);
        Value::Object(payload)
    }

    pub(crate) fn unavailable_payload(&self, feature: &str) -> serde_json::Value {
        let mut payload = self
            .guidance()
            .unavailable_payload(feature)
            .as_object()
            .cloned()
            .unwrap_or_default();
        self.append_details(&mut payload);
        Value::Object(payload)
    }
}

/// Four-way decomposition of why `semantic_search` might not be
/// currently runnable. Phase 4a, §capability-reporting: the previous
/// single reason string "embeddings not loaded — call
/// index_embeddings first" conflated four distinct root causes, the
/// only one of which the user could actually act on was
/// `index_missing`. This enum keeps them separate so the caller can
/// suggest the right remediation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SemanticSearchStatus {
    /// The `semantic_search` handler is reachable, either because the
    /// engine is already loaded in memory or because an on-disk index
    /// exists and the engine will be lazy-initialized on first call.
    #[cfg(feature = "semantic")]
    Available,
    /// The bundled CodeSearchNet ONNX model file is missing or
    /// corrupt. User remediation: reinstall with a binary that ships
    /// the model, or set `SYMBIOTE_MODEL_DIR` / `CODELENS_MODEL_DIR`.
    #[cfg(feature = "semantic")]
    ModelAssetsUnavailable,
    /// The active tool surface (preset or profile) does not include
    /// `semantic_search`. User remediation: switch profile via
    /// `set_profile` / `set_preset`, or use a client that activates a
    /// richer surface.
    #[cfg(feature = "semantic")]
    NotInActiveSurface,
    /// The on-disk symbol index has no embedding rows yet. User
    /// remediation: call `index_embeddings` to build the index.
    #[cfg(feature = "semantic")]
    IndexMissing,
    /// The binary was built without the `semantic` cargo feature.
    /// User remediation: rebuild with `cargo build --features semantic`.
    ///
    /// Only constructed in the `#[cfg(not(feature = "semantic"))]`
    /// branch of `determine_semantic_search_status`. The default
    /// feature set for this crate enables `semantic`, so under a
    /// normal build this variant is unreachable — `#[allow(dead_code)]`
    /// silences the warning without dropping the variant, which we
    /// still want available for no-feature builds and for
    /// `semantic_search_status_reason_strings_are_distinct` to pin
    /// its reason text.
    #[allow(dead_code)]
    FeatureDisabled,
}

impl SemanticSearchStatus {
    pub(crate) fn status_key(&self) -> &'static str {
        self.guidance().status_key
    }

    pub(crate) fn reason_str(&self) -> Option<&'static str> {
        self.guidance().reason
    }

    pub(crate) fn reason_code(&self) -> Option<&'static str> {
        self.guidance().reason_code
    }

    pub(crate) fn recommended_action(&self) -> Option<&'static str> {
        self.guidance().recommended_action
    }

    pub(crate) fn action_target(&self) -> Option<&'static str> {
        self.guidance().action_target
    }

    pub(crate) fn guidance_payload(&self) -> serde_json::Value {
        self.guidance().guidance_payload()
    }

    pub(crate) fn unavailable_payload(&self, feature: &str) -> serde_json::Value {
        self.guidance().unavailable_payload(feature)
    }

    pub(crate) fn is_available(&self) -> bool {
        self.guidance().available
    }

    fn guidance(&self) -> GuidanceDescriptor {
        match self {
            #[cfg(feature = "semantic")]
            Self::Available => GuidanceDescriptor {
                status_key: "available",
                available: true,
                reason: None,
                reason_code: None,
                recommended_action: None,
                action_target: None,
            },
            #[cfg(feature = "semantic")]
            Self::ModelAssetsUnavailable => GuidanceDescriptor {
                status_key: "model_assets_unavailable",
                available: false,
                reason: Some(
                    "model assets unavailable — reinstall with bundled model or set SYMBIOTE_MODEL_DIR or CODELENS_MODEL_DIR",
                ),
                reason_code: Some("semantic_model_assets_unavailable"),
                recommended_action: Some("configure_model_assets"),
                action_target: Some("model_assets"),
            },
            #[cfg(feature = "semantic")]
            Self::NotInActiveSurface => GuidanceDescriptor {
                status_key: "not_in_active_surface",
                available: false,
                reason: Some(
                    "not in active surface — call set_profile/set_preset to include semantic_search",
                ),
                reason_code: Some("semantic_not_in_active_surface"),
                recommended_action: Some("switch_tool_surface"),
                action_target: Some("tool_surface"),
            },
            #[cfg(feature = "semantic")]
            Self::IndexMissing => GuidanceDescriptor {
                status_key: "index_missing",
                available: false,
                reason: Some("index missing — call index_embeddings to build the embedding index"),
                reason_code: Some("semantic_index_missing"),
                recommended_action: Some("run_index_embeddings"),
                action_target: Some("embedding_index"),
            },
            Self::FeatureDisabled => GuidanceDescriptor {
                status_key: "feature_disabled",
                available: false,
                reason: Some("feature disabled — rebuild with `cargo build --features semantic`"),
                reason_code: Some("semantic_feature_disabled"),
                recommended_action: Some("rebuild_with_semantic_feature"),
                action_target: Some("binary"),
            },
        }
    }
}
