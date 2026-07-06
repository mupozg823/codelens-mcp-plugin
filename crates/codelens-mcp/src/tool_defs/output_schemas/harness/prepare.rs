mod adaptation;
mod runtime;

use adaptation::{
    coordination_output_schema, host_environment_output_schema, overlay_output_schema,
    skill_hints_output_schema,
};
use runtime::{
    config_output_schema, harness_runtime_output_schema, http_session_output_schema,
    index_recovery_output_schema, routing_output_schema, visible_tools_output_schema,
    warnings_output_schema,
};
use serde_json::json;

use super::super::jobs::activate_project_output_schema;
use super::{
    get_capabilities_output_schema, health_summary_output_schema, surface_generation_output_schema,
};

pub(crate) fn prepare_harness_session_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "activated": {"type": "boolean"},
            "project": activate_project_output_schema(),
            "active_surface": {"type": "string"},
            "token_budget": {"type": "integer"},
            "surface_generation": surface_generation_output_schema(),
            "config": config_output_schema(),
            "index_recovery": index_recovery_output_schema(),
            "capabilities": get_capabilities_output_schema(),
            "health_summary": health_summary_output_schema(),
            "warnings": warnings_output_schema(),
            "skill_hints": skill_hints_output_schema(),
            "host_environment": host_environment_output_schema(),
            "overlay": overlay_output_schema(),
            "coordination": coordination_output_schema(),
            "http_session": http_session_output_schema(),
            "visible_tools": visible_tools_output_schema(),
            "routing": routing_output_schema(),
            "harness": harness_runtime_output_schema()
        }
    })
}
