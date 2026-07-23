use crate::telemetry::ToolInvocation;

pub(super) fn has_low_level_chain(timeline: &[ToolInvocation]) -> bool {
    if timeline.len() < 3 {
        return false;
    }
    let recent = &timeline[timeline.len() - 3..];
    recent.iter().all(|entry| entry.work_class.is_primitive())
}
