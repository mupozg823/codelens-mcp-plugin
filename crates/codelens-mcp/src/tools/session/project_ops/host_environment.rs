use crate::client_profile::ClientProfile;
use crate::host_capabilities::HostCapabilities;
use crate::session_context::SessionRequestContext;
use crate::tool_defs::HostContext;
use serde_json::{Value, json};
use std::path::PathBuf;

mod field_extract;
mod memory_entrypoints;

use field_extract::{first_string_array, first_string_field};
use memory_entrypoints::memory_entrypoints;

#[derive(Debug, Clone)]
pub(super) struct HostEnvironmentSnapshot {
    pub client_profile: ClientProfile,
    pub client_name: Option<String>,
    pub client_version: Option<String>,
    pub requested_profile: Option<String>,
    pub deferred_tool_loading: bool,
    pub loaded_namespaces: Vec<String>,
    pub loaded_tiers: Vec<String>,
    pub full_tool_exposure: bool,
    pub available_mcp_servers: Vec<String>,
    pub available_mcp_tools: Vec<String>,
    pub skill_roots: Vec<String>,
    pub skill_root_source: HostRootSource,
    pub memory_roots: Vec<String>,
    pub host_setting_keys: Vec<String>,
    pub harness_profile: Option<String>,
    pub host_context: Option<HostContext>,
    pub host_capabilities: Option<HostCapabilities>,
    pub explicit_snapshot: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HostRootSource {
    HostSnapshot,
    CodexDefaultRoots,
    None,
}

impl HostRootSource {
    const fn as_str(self) -> &'static str {
        match self {
            Self::HostSnapshot => "host_snapshot",
            Self::CodexDefaultRoots => "codex_default_roots",
            Self::None => "none",
        }
    }
}

impl HostEnvironmentSnapshot {
    pub(super) fn from_arguments(
        arguments: &Value,
        session: &SessionRequestContext,
        client_profile: ClientProfile,
        host_capabilities: Option<HostCapabilities>,
    ) -> Self {
        let host_context = arguments
            .get("host_context")
            .or_else(|| arguments.get("_session_host_context"))
            .and_then(|value| value.as_str())
            .and_then(HostContext::from_str);
        let available_mcp_servers = first_string_array(
            arguments,
            "available_mcp_servers",
            "_session_available_mcp_servers",
        );
        let available_mcp_tools = first_string_array(
            arguments,
            "available_mcp_tools",
            "_session_available_mcp_tools",
        );
        let mut skill_roots = first_string_array(arguments, "skill_roots", "_session_skill_roots");
        let memory_roots = first_string_array(arguments, "memory_roots", "_session_memory_roots");
        let host_setting_keys =
            first_string_array(arguments, "host_setting_keys", "_session_host_setting_keys");
        let harness_profile =
            first_string_field(arguments, "harness_profile", "_session_harness_profile")
                .or_else(|| session.requested_profile.clone());
        let explicit_snapshot = !available_mcp_servers.is_empty()
            || !available_mcp_tools.is_empty()
            || !skill_roots.is_empty()
            || !memory_roots.is_empty()
            || !host_setting_keys.is_empty()
            || harness_profile.is_some()
            || host_context.is_some()
            || host_capabilities.is_some();

        let effective_client_profile = host_context
            .and_then(|context| ClientProfile::from_host_context(context.as_str()))
            .unwrap_or(client_profile);
        let skill_root_source = if !skill_roots.is_empty() {
            HostRootSource::HostSnapshot
        } else if effective_client_profile == ClientProfile::Codex {
            skill_roots = crate::skill_catalog::codex_default_skill_roots()
                .into_iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect();
            if skill_roots.is_empty() {
                HostRootSource::None
            } else {
                HostRootSource::CodexDefaultRoots
            }
        } else {
            HostRootSource::None
        };

        Self {
            client_profile: effective_client_profile,
            client_name: session.client_name.clone(),
            client_version: session.client_version.clone(),
            requested_profile: session.requested_profile.clone(),
            deferred_tool_loading: session.deferred_loading,
            loaded_namespaces: session.loaded_namespaces.clone(),
            loaded_tiers: session.loaded_tiers.clone(),
            full_tool_exposure: session.full_tool_exposure,
            available_mcp_servers,
            available_mcp_tools,
            skill_roots,
            skill_root_source,
            memory_roots,
            host_setting_keys,
            harness_profile,
            host_context,
            host_capabilities,
            explicit_snapshot,
        }
    }

    pub(super) fn skill_root_paths(&self) -> Vec<PathBuf> {
        self.skill_roots.iter().map(PathBuf::from).collect()
    }

    pub(super) fn payload(&self) -> Value {
        let memory_entrypoints = memory_entrypoints(&self.memory_roots);
        let memory_entrypoint_count = memory_entrypoints.len();
        json!({
            "client_profile": self.client_profile.as_str(),
            "client_name": self.client_name,
            "client_version": self.client_version,
            "host_context": self.host_context.map(|value| value.as_str()),
            "host_capabilities": HostCapabilities::negotiated_payload(self.host_capabilities),
            "snapshot_source": if self.explicit_snapshot { "explicit_host_snapshot" } else { "session_defaults" },
            "requested_profile": self.requested_profile,
            "harness_profile": self.harness_profile,
            "deferred_tool_loading": self.deferred_tool_loading,
            "loaded_namespaces": self.loaded_namespaces,
            "loaded_tiers": self.loaded_tiers,
            "full_tool_exposure": self.full_tool_exposure,
            "available_mcp_servers": self.available_mcp_servers,
            "available_mcp_tools": self.available_mcp_tools,
            "skill_roots": self.skill_roots,
            "skill_root_source": self.skill_root_source.as_str(),
            "memory_roots": self.memory_roots,
            "memory_entrypoints": memory_entrypoints,
            "memory_entrypoint_count": memory_entrypoint_count,
            "host_setting_keys": self.host_setting_keys,
            "counts": {
                "available_mcp_servers": self.available_mcp_servers.len(),
                "available_mcp_tools": self.available_mcp_tools.len(),
                "skill_roots": self.skill_roots.len(),
                "memory_roots": self.memory_roots.len(),
                "memory_entrypoints": memory_entrypoint_count,
                "host_setting_keys": self.host_setting_keys.len(),
                "loaded_namespaces": self.loaded_namespaces.len(),
                "loaded_tiers": self.loaded_tiers.len(),
            },
            "adaptation_notes": self.adaptation_notes(),
        })
    }

    pub(super) fn compact_payload(&self) -> Value {
        let memory_entrypoints = memory_entrypoints(&self.memory_roots);
        let memory_entrypoint_count = memory_entrypoints.len();
        json!({
            "client_profile": self.client_profile.as_str(),
            "host_context": self.host_context.map(|value| value.as_str()),
            "host_capabilities": HostCapabilities::negotiated_payload(self.host_capabilities),
            "snapshot_source": if self.explicit_snapshot { "explicit_host_snapshot" } else { "session_defaults" },
            "available_mcp_server_count": self.available_mcp_servers.len(),
            "available_mcp_tool_count": self.available_mcp_tools.len(),
            "skill_root_count": self.skill_roots.len(),
            "skill_root_source": self.skill_root_source.as_str(),
            "memory_root_count": self.memory_roots.len(),
            "memory_entrypoint_count": memory_entrypoint_count,
            "memory_entrypoints": memory_entrypoints,
            "host_setting_key_count": self.host_setting_keys.len(),
            "deferred_tool_loading": self.deferred_tool_loading,
            "full_tool_exposure": self.full_tool_exposure,
            "adaptation_notes": self.adaptation_notes(),
        })
    }

    fn adaptation_notes(&self) -> Vec<String> {
        let mut notes = Vec::new();
        if self.explicit_snapshot {
            notes.push(
                "Using host-observed settings instead of assuming every Claude/Codex install has the same MCP, skill, memory, or harness layout.".to_owned(),
            );
        } else {
            notes.push(
                "No explicit host settings snapshot was supplied; routing falls back to client/profile defaults.".to_owned(),
            );
        }
        match self.host_context {
            Some(HostContext::Codex) => notes.push(
                "Codex host_context selected; prefer AGENTS.md routing plus compact skill metadata hints before loading SKILL.md bodies.".to_owned(),
            ),
            Some(HostContext::ClaudeCode) => notes.push(
                "Claude Code host_context selected; respect managed settings and memory boundaries supplied by the host snapshot.".to_owned(),
            ),
            Some(_) => notes.push(
                "A host_context hint was supplied; adapt routing to that client's tool and instruction surface instead of assuming Claude/Codex defaults.".to_owned(),
            ),
            None => {}
        }
        match self.skill_root_source {
            HostRootSource::HostSnapshot => notes.push(
                "Skill hints are bound to the supplied skill_roots and only metadata is scanned during bootstrap.".to_owned(),
            ),
            HostRootSource::CodexDefaultRoots => notes.push(
                "Codex default skill roots were detected; skill hints scan only metadata from those folders during bootstrap.".to_owned(),
            ),
            HostRootSource::None => {}
        }
        if !self.memory_roots.is_empty() {
            notes.push(
                "Memory roots were observed; prefer root-aware memory lookup before broad project scans.".to_owned(),
            );
        }
        if !self.available_mcp_tools.is_empty() {
            notes.push(
                "Host-observed MCP tool inventory is available; prefer advertised tool names before assuming a capability is missing.".to_owned(),
            );
        }
        if self
            .host_capabilities
            .is_some_and(|capabilities| capabilities.native_tool_search)
        {
            notes.push(
                "Native tool search is declared; the host owns next-action selection and server-side suggestions are suppressed.".to_owned(),
            );
        }
        if self
            .host_setting_keys
            .iter()
            .any(|key| host_policy_key(key))
        {
            notes.push(
                "managed or locked host settings were observed; treat repo and user instructions as bounded by host policy.".to_owned(),
            );
        }
        if self.available_mcp_servers.len() >= 6 && self.full_tool_exposure {
            notes.push(
                "Many MCP servers are attached while full tool exposure is enabled; prefer deferred namespace/tier loading to reduce token pressure.".to_owned(),
            );
        }
        notes
    }
}

fn host_policy_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    normalized.contains("managed")
        || normalized.contains("locked")
        || normalized.contains("permission")
        || normalized.contains("policy")
}
