use serde_json::{Value, json};

pub(crate) fn analysis_summary_resource(analysis_id: &str) -> Value {
    json!({
        "uri": format!("codelens://analysis/{analysis_id}/summary"),
    })
}

pub(crate) fn analysis_section_handle_template(analysis_id: &str) -> String {
    format!("codelens://analysis/{analysis_id}/{{section}}")
}

pub(crate) fn analysis_section_handles(analysis_id: &str, sections: &[String]) -> Value {
    json!(
        sections
            .iter()
            .map(|section| json!({
                "section": section,
                "uri": format!("codelens://analysis/{analysis_id}/{section}"),
            }))
            .collect::<Vec<_>>()
    )
}
