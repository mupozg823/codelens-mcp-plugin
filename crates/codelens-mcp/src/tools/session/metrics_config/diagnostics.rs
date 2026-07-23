use serde_json::json;

#[derive(Debug, Clone, PartialEq, Eq)]
enum DiagnosticsStatus {
    Available,
    FilePathRequired,
    UnsupportedExtension,
    LspBinaryMissing,
}

impl DiagnosticsStatus {
    fn status_key(&self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::FilePathRequired => "file_path_required",
            Self::UnsupportedExtension => "unsupported_extension",
            Self::LspBinaryMissing => "lsp_binary_missing",
        }
    }

    fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DiagnosticsGuidance {
    status: DiagnosticsStatus,
    file_extension: Option<String>,
    language: Option<&'static str>,
    lsp_command: Option<&'static str>,
    server_name: Option<&'static str>,
    install_command: Option<&'static str>,
    package_manager: Option<&'static str>,
}

impl DiagnosticsGuidance {
    pub(crate) fn for_file(
        file_path: Option<&str>,
        binary_available: impl Fn(&str) -> bool,
    ) -> Self {
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
            (Some(_), Some(recipe)) if !binary_available(recipe.binary_name) => {
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

    pub(crate) fn is_available(&self) -> bool {
        self.status.is_available()
    }

    fn reason_str(&self) -> Option<&'static str> {
        match self.status {
            DiagnosticsStatus::Available => None,
            DiagnosticsStatus::FilePathRequired => Some(
                "file_path required — provide a concrete source file so CodeLens can select an LSP recipe",
            ),
            DiagnosticsStatus::UnsupportedExtension => Some(
                "unsupported extension — no default LSP recipe is registered for this file type",
            ),
            DiagnosticsStatus::LspBinaryMissing => {
                Some("LSP binary missing — install the configured server in the daemon environment")
            }
        }
    }

    fn reason_code(&self) -> Option<&'static str> {
        match self.status {
            DiagnosticsStatus::Available => None,
            DiagnosticsStatus::FilePathRequired => Some("diagnostics_file_path_required"),
            DiagnosticsStatus::UnsupportedExtension => Some("diagnostics_unsupported_extension"),
            DiagnosticsStatus::LspBinaryMissing => Some("diagnostics_lsp_binary_missing"),
        }
    }

    fn recommended_action(&self) -> Option<&'static str> {
        match self.status {
            DiagnosticsStatus::Available => None,
            DiagnosticsStatus::FilePathRequired => Some("provide_file_path"),
            DiagnosticsStatus::UnsupportedExtension => Some("pass_explicit_lsp_command"),
            DiagnosticsStatus::LspBinaryMissing => Some("install_lsp_server"),
        }
    }

    fn action_target(&self) -> Option<&'static str> {
        match self.status {
            DiagnosticsStatus::Available => None,
            DiagnosticsStatus::FilePathRequired => Some("file_path"),
            DiagnosticsStatus::UnsupportedExtension => Some("file_extension"),
            DiagnosticsStatus::LspBinaryMissing => Some("lsp_server"),
        }
    }

    pub(crate) fn guidance_payload(&self) -> serde_json::Value {
        json!({
            "status": self.status.status_key(),
            "available": self.status.is_available(),
            "reason": self.reason_str(),
            "reason_code": self.reason_code(),
            "recommended_action": self.recommended_action(),
            "action_target": self.action_target(),
            "file_extension": self.file_extension,
            "language": self.language,
            "lsp_command": self.lsp_command,
            "server_name": self.server_name,
            "install_command": self.install_command,
            "package_manager": self.package_manager,
        })
    }

    pub(crate) fn unavailable_payload(&self, feature: &str) -> serde_json::Value {
        json!({
            "feature": feature,
            "reason": self.reason_str().unwrap_or("diagnostics available"),
            "status": self.status.status_key(),
            "reason_code": self.reason_code(),
            "recommended_action": self.recommended_action(),
            "action_target": self.action_target(),
            "file_extension": self.file_extension,
            "language": self.language,
            "lsp_command": self.lsp_command,
            "server_name": self.server_name,
            "install_command": self.install_command,
            "package_manager": self.package_manager,
        })
    }
}
