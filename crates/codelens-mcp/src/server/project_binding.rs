//! Project-binding provenance and precedence for HTTP sessions.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ProjectBindingSource {
    #[default]
    DaemonDefault,
    RequestHeader,
    InitializeParam,
    ExplicitTool,
}

impl ProjectBindingSource {
    pub(crate) const fn is_explicit(self) -> bool {
        !matches!(self, Self::DaemonDefault)
    }

    pub(crate) fn from_initialize(
        initialize_project: Option<String>,
        header_project: Option<String>,
    ) -> (Option<String>, Self) {
        match (initialize_project, header_project) {
            (Some(project), _) => (Some(project), Self::InitializeParam),
            (None, Some(project)) => (Some(project), Self::RequestHeader),
            (None, None) => (None, Self::DaemonDefault),
        }
    }

    const fn precedence(self) -> u8 {
        match self {
            Self::DaemonDefault => 0,
            Self::RequestHeader => 1,
            Self::InitializeParam => 2,
            Self::ExplicitTool => 3,
        }
    }

    pub(crate) const fn can_replace(self, current: Self) -> bool {
        self.precedence() >= current.precedence()
    }
}

#[cfg(test)]
mod tests {
    use super::ProjectBindingSource;

    #[test]
    fn explicit_tool_outranks_recurring_request_header() {
        // Given: an explicit tool binding and a recurring request header.
        let explicit = ProjectBindingSource::ExplicitTool;
        let header = ProjectBindingSource::RequestHeader;

        // When: each source is checked against the other.
        let explicit_can_replace_header = explicit.can_replace(header);
        let header_can_replace_explicit = header.can_replace(explicit);

        // Then: only the explicit binding can replace the lower-precedence header.
        assert!(explicit_can_replace_header);
        assert!(!header_can_replace_explicit);
    }

    #[test]
    fn same_source_request_header_can_switch_projects() {
        // Given: a live session and a new binding from the same header source.
        let current = ProjectBindingSource::RequestHeader;
        let incoming = ProjectBindingSource::RequestHeader;

        // When: replacement precedence is evaluated.
        let can_switch = incoming.can_replace(current);

        // Then: recurring header-bound hosts can still switch workspaces.
        assert!(can_switch);
    }
}
