use super::{Value, analysis_section_handles, analysis_summary_resource, json};

pub(super) fn job_handle_fields(analysis_id: Option<&str>, sections: &[String]) -> Value {
    match analysis_id {
        Some(analysis_id) => json!({
            "summary_resource": analysis_summary_resource(analysis_id),
            "section_handles": analysis_section_handles(analysis_id, sections),
        }),
        None => json!({
            "summary_resource": Value::Null,
            "section_handles": Vec::<Value>::new(),
        }),
    }
}
