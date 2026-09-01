#!/usr/bin/env python3
from pathlib import Path


def patch(path: str, old: str, new: str) -> None:
    target = Path(path)
    source = target.read_text()
    if old not in source:
        raise SystemExit(f"missing provider-seam anchor in {path}: {old[:140]!r}")
    target.write_text(source.replace(old, new, 1))


module = Path("crates/aikit-core/src/resource/operative_provider.rs")
module.write_text(r'''//! Provider-owned semantic enrichment for the general O:I operative field.
//!
//! AIKit owns `AddressHorizon`, `RelationOp`, `ResolveExpression`, canonical
//! `ResourceRef`/`ActionRef` identity and `ResolvePath`. A richer semantic product
//! may read those objects through this seam while keeping its native identities
//! and return types. In particular QL-MEF can bind `VakRef`, `VakRelation`,
//! `VakActionProfile` and `VakPath` here without introducing a QL dependency or a
//! second parser/resolver into AIKit.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::Result;

use super::{
    ActionSemanticProfile, AddressHorizon, ProviderRef, RelationOp, ResolveExpression, ResolvePath,
    ResourceRef, ResourceSource,
};

pub const OPERATIVE_SEMANTIC_PROVIDER_VERSION: &str = "aikit.operative-semantic-provider/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OperativeSemanticOperation {
    HorizonBinding,
    RelationBinding,
    ResourceReading,
    ActionProfile,
    ResolvePath,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct OperativeSemanticProviderCapabilities {
    #[serde(default)]
    pub operations: BTreeSet<OperativeSemanticOperation>,
}

impl OperativeSemanticProviderCapabilities {
    pub fn supports(&self, operation: OperativeSemanticOperation) -> bool {
        self.operations.contains(&operation)
    }

    pub fn with_operations(
        operations: impl IntoIterator<Item = OperativeSemanticOperation>,
    ) -> Self {
        Self {
            operations: operations.into_iter().collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum OperativeSemanticProviderStatus {
    Available,
    Degraded { reason: String },
    Unavailable { reason: String },
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperativeSemanticProviderDescriptor {
    pub version: String,
    pub provider: ProviderRef,
    pub status: OperativeSemanticProviderStatus,
    pub capabilities: OperativeSemanticProviderCapabilities,
    /// Provider/source provenance for the semantic system itself. Individual
    /// native readings may retain richer provenance inside their associated type.
    #[serde(default)]
    pub provenance: Vec<ResourceSource>,
}

impl OperativeSemanticProviderDescriptor {
    pub fn new(
        provider: ProviderRef,
        status: OperativeSemanticProviderStatus,
        capabilities: OperativeSemanticProviderCapabilities,
    ) -> Self {
        Self {
            version: OPERATIVE_SEMANTIC_PROVIDER_VERSION.into(),
            provider,
            status,
            capabilities,
            provenance: Vec::new(),
        }
    }
}

/// Enrich the general O:I operative objects without replacing them.
///
/// Associated types are intentional. AIKit does not pretend to own or flatten a
/// provider's semantic identities. QL-MEF can therefore use exact `VakRef`,
/// `VakRelation`, `VakActionProfile` and `VakPath` types while another provider
/// can expose a different semantic reading over the same general expression.
pub trait OperativeSemanticProvider {
    /// Exact provider-native semantic identity used for horizon/operator binding
    /// (for QL-MEF this is `VakRef`).
    type SemanticRef;
    /// Provider-native reading of an ordinary O:I Resource (`VakRelation` for the
    /// full Vāk profile).
    type ResourceReading;
    /// Provider-native enrichment of the general Action semantic profile.
    type ActionProfile;
    /// Provider-native enrichment of one observed/general ResolvePath.
    type Path;

    fn descriptor(&self) -> OperativeSemanticProviderDescriptor;

    fn bind_horizon(&self, horizon: AddressHorizon) -> Result<Option<Self::SemanticRef>>;

    fn bind_relation(&self, relation: RelationOp) -> Result<Option<Self::SemanticRef>>;

    /// Read one ordinary canonical Ref through provider-native relations. The
    /// optional expression preserves the situated relation when the reading is
    /// expression-dependent; no provider relation is authored by AIKit.
    fn resource_readings(
        &self,
        resource: &ResourceRef,
        expression: Option<&ResolveExpression>,
    ) -> Result<Vec<Self::ResourceReading>>;

    fn enrich_action(
        &self,
        profile: &ActionSemanticProfile,
    ) -> Result<Option<Self::ActionProfile>>;

    fn enrich_path(&self, path: &ResolvePath) -> Result<Option<Self::Path>>;
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::resource::{
        parse_resolve_expression, resolve_expression, ActionRef, MemoryResourceIndex,
        ResourceDescriptor, ResourceKind, ResourceRecord,
    };

    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct FullProfileFixture;

    impl OperativeSemanticProvider for FullProfileFixture {
        type SemanticRef = String;
        type ResourceReading = String;
        type ActionProfile = String;
        type Path = String;

        fn descriptor(&self) -> OperativeSemanticProviderDescriptor {
            OperativeSemanticProviderDescriptor::new(
                ProviderRef::parse("provider/full-profile-fixture").unwrap(),
                OperativeSemanticProviderStatus::Available,
                OperativeSemanticProviderCapabilities::with_operations([
                    OperativeSemanticOperation::HorizonBinding,
                    OperativeSemanticOperation::RelationBinding,
                    OperativeSemanticOperation::ResourceReading,
                    OperativeSemanticOperation::ActionProfile,
                    OperativeSemanticOperation::ResolvePath,
                ]),
            )
        }

        fn bind_horizon(&self, horizon: AddressHorizon) -> Result<Option<Self::SemanticRef>> {
            Ok(Some(
                match horizon {
                    AddressHorizon::H0 => "##",
                    AddressHorizon::H1 => "O#",
                    AddressHorizon::H2 => "X#",
                    AddressHorizon::H3 => "N#",
                    AddressHorizon::H4 => "M#",
                    AddressHorizon::H5 => "R#",
                }
                .into(),
            ))
        }

        fn bind_relation(&self, relation: RelationOp) -> Result<Option<Self::SemanticRef>> {
            Ok(Some(relation.symbol().into()))
        }

        fn resource_readings(
            &self,
            resource: &ResourceRef,
            expression: Option<&ResolveExpression>,
        ) -> Result<Vec<Self::ResourceReading>> {
            Ok(vec![format!(
                "{}|{}",
                resource,
                expression.map(ResolveExpression::render).unwrap_or_default()
            )])
        }

        fn enrich_action(
            &self,
            profile: &ActionSemanticProfile,
        ) -> Result<Option<Self::ActionProfile>> {
            Ok(Some(format!("full:{}", profile.action_ref.resource())))
        }

        fn enrich_path(&self, path: &ResolvePath) -> Result<Option<Self::Path>> {
            Ok(Some(format!("full-path:{}", path.identity)))
        }
    }

    #[test]
    fn provider_can_bind_full_profile_types_without_replacing_general_identity() {
        let provider = FullProfileFixture;
        let descriptor = provider.descriptor();
        assert_eq!(descriptor.version, OPERATIVE_SEMANTIC_PROVIDER_VERSION);
        assert!(descriptor
            .capabilities
            .supports(OperativeSemanticOperation::ResolvePath));
        assert_eq!(provider.bind_horizon(AddressHorizon::H5).unwrap(), Some("R#".into()));
        assert_eq!(
            provider.bind_relation(RelationOp::Contextualise).unwrap(),
            Some("/".into())
        );

        let action_ref = ResourceRef::parse("action/verify").unwrap();
        let subject = ResourceRef::parse("project/app").unwrap();
        let profile = ActionSemanticProfile {
            action_ref: ActionRef(action_ref.clone()),
            relation_affinities: BTreeSet::from([RelationOp::Affirm]),
            horizon_affinities: BTreeSet::from([AddressHorizon::H5]),
            subject_ref_kinds: BTreeSet::from([ResourceKind::Project]),
            method_relations: Vec::new(),
            focus_relations: vec!["verify".into()],
            expected_return_forms: vec!["evidence".into()],
            native_owner: None,
            provenance: Vec::new(),
        };
        assert_eq!(
            provider.enrich_action(&profile).unwrap(),
            Some("full:action/verify".into())
        );

        let mut index = MemoryResourceIndex::default();
        index.insert(ResourceRecord::new(ResourceDescriptor::new(
            action_ref.clone(),
            ResourceKind::Action,
            "Verify",
            "verify current state",
        )));
        index.insert(ResourceRecord::new(ResourceDescriptor::new(
            subject.clone(),
            ResourceKind::Project,
            "App",
            "current project",
        )));
        let expression = parse_resolve_expression("+ @5 action/verify").unwrap();
        let path = resolve_expression(&expression, &index, 16);
        assert_eq!(path.destination(), Some(&action_ref));
        assert_eq!(
            provider.enrich_path(&path).unwrap(),
            Some(format!("full-path:{}", path.identity))
        );
        assert_eq!(
            provider
                .resource_readings(&subject, Some(&expression))
                .unwrap(),
            vec!["project/app|+ @5 action/verify".to_string()]
        );

        // Provider enrichment never changes the general ResolvePath identity.
        let after = resolve_expression(&expression, &index, 16);
        assert_eq!(path.identity, after.identity);
        assert_eq!(path.expression, after.expression);
    }
}
''')

mod_rs = "crates/aikit-core/src/resource/mod.rs"
patch(
    mod_rs,
    '''mod operative;\nmod refs;\n''',
    '''mod operative;\nmod operative_provider;\nmod refs;\n''',
)
patch(
    mod_rs,
    '''pub use refs::{OwnerRef, ProviderRef, ResourceRef, SourceRef, SourceRevision};\n''',
    '''pub use operative_provider::{\n    OperativeSemanticOperation, OperativeSemanticProvider, OperativeSemanticProviderCapabilities,\n    OperativeSemanticProviderDescriptor, OperativeSemanticProviderStatus,\n    OPERATIVE_SEMANTIC_PROVIDER_VERSION,\n};\npub use refs::{OwnerRef, ProviderRef, ResourceRef, SourceRef, SourceRevision};\n''',
)
