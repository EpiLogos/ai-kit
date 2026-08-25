//! Flow → authored SemanticWiki relation projection.
//!
//! Flow remains an owner-supplied low-ceremony source. This adapter consumes one
//! already-authorised `FlowStandingContext`, preserving its stable FlowRef,
//! SourceRef and exact revision while interpreting explicit Markdown links. No
//! frontmatter is required and no Flow is promoted into Wiki identity.

use aikit_core::flow::FlowStandingContext;
use aikit_core::resource::SourceAuthority;
use aikit_core::{AikitError, Result};

use crate::authored_wiki_source::{
    parse_authored_wiki_source_with_authority, AuthoredWikiSourceProjection,
};

pub const FLOW_AUTHORED_WIKI_VERSION: &str = "aikit.flow-authored-wiki/v1";

pub fn standing_flow_authored_wiki_source(
    standing: &FlowStandingContext,
    source_authority: SourceAuthority,
) -> Result<AuthoredWikiSourceProjection> {
    let body = standing.disclosed_body().ok_or_else(|| {
        AikitError::new(
            "flow.authored_wiki_undisclosed",
            "authored Wiki relation projection requires the exact Flow body to be disclosed",
        )
    })?;
    if standing.automatic_agent_or_model_invocation {
        return Err(AikitError::new(
            "flow.authored_wiki_invalid_standing",
            "Flow standing-context materialization must remain deterministic",
        ));
    }

    let locators = standing.binding.provenance.to_vec();
    parse_authored_wiki_source_with_authority(
        standing.binding.flow_ref.clone(),
        standing.binding.source_ref.clone(),
        source_authority,
        Some(standing.binding.flow_revision.clone()),
        locators,
        body,
    )
}

#[cfg(test)]
mod tests {
    use aikit_core::flow::{
        FlowBinding, FlowLifecycle, FlowStandingContext, FlowStandingDisclosure,
        FLOW_CONTEXT_VERSION,
    };
    use aikit_core::project::ProjectRef;
    use aikit_core::resource::{ResourceRef, SourceAuthority, SourceRef, SourceRevision};

    use super::*;

    fn r(raw: &str) -> ResourceRef {
        ResourceRef::parse(raw).unwrap()
    }

    #[test]
    fn ordinary_frontmatter_free_flow_projects_links_without_becoming_wiki_node() {
        let standing = FlowStandingContext {
            version: FLOW_CONTEXT_VERSION.into(),
            binding: FlowBinding {
                version: FLOW_CONTEXT_VERSION.into(),
                flow_ref: r("flow:thread:42"),
                source_ref: SourceRef::parse("source:flow:42").unwrap(),
                flow_revision: SourceRevision::parse("rev-42").unwrap(),
                provider: r("provider:central:flow"),
                project: ProjectRef::parse("epilogos/demo").unwrap(),
                context_resolution_version: "ctx/v1".into(),
                context_resolution_hash: "ctx-hash".into(),
                agent_session: r("agent-session:42"),
                agent: None,
                agency: None,
                provenance: vec!["flows/2026-08-25-thread.md".into()],
            },
            lifecycle: FlowLifecycle::Active,
            disclosure: FlowStandingDisclosure::Disclosed {
                body: "Working through [[Living Wiki]] and [[Future Concept]].\n".into(),
                digest: "digest".into(),
            },
            automatic_agent_or_model_invocation: false,
        };

        let projection =
            standing_flow_authored_wiki_source(&standing, SourceAuthority::Authored).unwrap();
        assert_eq!(projection.subject_ref.as_str(), "flow:thread:42");
        assert_eq!(projection.source_ref.as_str(), "source:flow:42");
        assert_eq!(projection.source_revision.as_ref().unwrap().as_str(), "rev-42");
        assert_eq!(projection.source_authority, SourceAuthority::Authored);
        assert_eq!(projection.relations.len(), 2);
        assert_eq!(projection.relations[0].relation, "references");
        assert_eq!(projection.locators, vec!["flows/2026-08-25-thread.md"]);
    }

    #[test]
    fn agent_maintained_flow_keeps_learned_authority() {
        let standing = FlowStandingContext {
            version: FLOW_CONTEXT_VERSION.into(),
            binding: FlowBinding {
                version: FLOW_CONTEXT_VERSION.into(),
                flow_ref: r("flow:thread:agent"),
                source_ref: SourceRef::parse("source:flow:agent").unwrap(),
                flow_revision: SourceRevision::parse("rev-agent").unwrap(),
                provider: r("provider:other-source-house"),
                project: ProjectRef::parse("epilogos/demo").unwrap(),
                context_resolution_version: "ctx/v1".into(),
                context_resolution_hash: "ctx-hash".into(),
                agent_session: r("agent-session:agent"),
                agent: Some(r("agent:epii")),
                agency: None,
                provenance: Vec::new(),
            },
            lifecycle: FlowLifecycle::Active,
            disclosure: FlowStandingDisclosure::Disclosed {
                body: "Agent-maintained thread [[Living Wiki]].".into(),
                digest: "digest".into(),
            },
            automatic_agent_or_model_invocation: false,
        };

        let projection =
            standing_flow_authored_wiki_source(&standing, SourceAuthority::Learned).unwrap();
        assert_eq!(projection.source_authority, SourceAuthority::Learned);
        assert_eq!(projection.relations[0].raw_target, "Living Wiki");
    }

    #[test]
    fn undisclosed_flow_does_not_leak_source_language_into_relation_projection() {
        let standing = FlowStandingContext {
            version: FLOW_CONTEXT_VERSION.into(),
            binding: FlowBinding {
                version: FLOW_CONTEXT_VERSION.into(),
                flow_ref: r("flow:thread:hidden"),
                source_ref: SourceRef::parse("source:flow:hidden").unwrap(),
                flow_revision: SourceRevision::parse("rev-hidden").unwrap(),
                provider: r("provider:flow"),
                project: ProjectRef::parse("epilogos/demo").unwrap(),
                context_resolution_version: "ctx/v1".into(),
                context_resolution_hash: "ctx-hash".into(),
                agent_session: r("agent-session:hidden"),
                agent: None,
                agency: None,
                provenance: Vec::new(),
            },
            lifecycle: FlowLifecycle::Active,
            disclosure: FlowStandingDisclosure::Undisclosed {
                reason: "not authorised for this act".into(),
            },
            automatic_agent_or_model_invocation: false,
        };

        let error =
            standing_flow_authored_wiki_source(&standing, SourceAuthority::Observed).unwrap_err();
        assert_eq!(error.code(), "flow.authored_wiki_undisclosed");
    }
}
