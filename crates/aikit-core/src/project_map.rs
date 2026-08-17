use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::resource::{ProviderRef, ResourceKind, ResourceRef, SourceAuthority};
use crate::{AikitError, Result};

pub const PROJECT_MAP_VERSION: &str = "aikit.project-map/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectLens {
    Git,
    Code,
    SemanticWiki,
    SourcePool,
    Canon,
    Run,
    Decision,
    Verification,
    Evolution,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectMapEndpoint {
    pub resource: ResourceRef,
    pub kind: ResourceKind,
    pub lens: ProjectLens,
    pub authority: SourceAuthority,
    #[serde(default)]
    pub provider: Option<ProviderRef>,
    #[serde(default)]
    pub revision: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectMapBinding {
    pub from: ResourceRef,
    pub to: ResourceRef,
    pub relation: String,
    /// A reversible cross-lens binding can be traversed in either direction.
    /// This never upgrades the binding into a provider-native edge.
    pub reversible: bool,
    pub authority: SourceAuthority,
    #[serde(default)]
    pub provider: Option<ProviderRef>,
    #[serde(default)]
    pub provenance: Vec<ResourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectMapStep {
    pub from: ResourceRef,
    pub to: ResourceRef,
    pub relation: String,
    pub reversed: bool,
    pub authority: SourceAuthority,
    #[serde(default)]
    pub provider: Option<ProviderRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProjectMap {
    endpoints: BTreeMap<ResourceRef, ProjectMapEndpoint>,
    bindings: Vec<ProjectMapBinding>,
}

impl ProjectMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn endpoint(&self, resource: &ResourceRef) -> Option<&ProjectMapEndpoint> {
        self.endpoints.get(resource)
    }

    pub fn endpoints(&self) -> impl Iterator<Item = &ProjectMapEndpoint> {
        self.endpoints.values()
    }

    pub fn bindings(&self) -> &[ProjectMapBinding] {
        &self.bindings
    }

    pub fn add_endpoint(&mut self, endpoint: ProjectMapEndpoint) -> Result<()> {
        if let Some(existing) = self.endpoints.get(&endpoint.resource) {
            if existing != &endpoint {
                return Err(AikitError::new(
                    "project_map.endpoint_conflict",
                    format!(
                        "ProjectMap endpoint {} was declared with conflicting lens metadata",
                        endpoint.resource
                    ),
                ));
            }
            return Ok(());
        }
        self.endpoints.insert(endpoint.resource.clone(), endpoint);
        Ok(())
    }

    pub fn bind(&mut self, binding: ProjectMapBinding) -> Result<()> {
        if binding.from == binding.to {
            return Err(AikitError::new(
                "project_map.self_binding",
                "ProjectMap cross-lens bindings must connect distinct resources",
            ));
        }
        if !self.endpoints.contains_key(&binding.from) {
            return Err(AikitError::new(
                "project_map.unknown_endpoint",
                format!("ProjectMap binding source {} is not registered", binding.from),
            ));
        }
        if !self.endpoints.contains_key(&binding.to) {
            return Err(AikitError::new(
                "project_map.unknown_endpoint",
                format!("ProjectMap binding target {} is not registered", binding.to),
            ));
        }
        if binding.relation.trim().is_empty() {
            return Err(AikitError::new(
                "project_map.empty_relation",
                "ProjectMap binding relation must be non-empty",
            ));
        }
        if !self.bindings.contains(&binding) {
            self.bindings.push(binding);
            self.bindings.sort_by(|left, right| {
                (&left.from, &left.to, &left.relation)
                    .cmp(&(&right.from, &right.to, &right.relation))
            });
        }
        Ok(())
    }

    pub fn neighbours(&self, resource: &ResourceRef) -> Vec<ProjectMapStep> {
        let mut steps = Vec::new();
        for binding in &self.bindings {
            if &binding.from == resource {
                steps.push(ProjectMapStep {
                    from: binding.from.clone(),
                    to: binding.to.clone(),
                    relation: binding.relation.clone(),
                    reversed: false,
                    authority: binding.authority,
                    provider: binding.provider.clone(),
                });
            }
            if binding.reversible && &binding.to == resource {
                steps.push(ProjectMapStep {
                    from: binding.to.clone(),
                    to: binding.from.clone(),
                    relation: binding.relation.clone(),
                    reversed: true,
                    authority: binding.authority,
                    provider: binding.provider.clone(),
                });
            }
        }
        steps.sort_by(|left, right| {
            (&left.to, &left.relation, left.reversed).cmp(&(&right.to, &right.relation, right.reversed))
        });
        steps
    }

    /// Find a bounded path across explicit ProjectMap bindings only. Provider
    /// graphs are never copied into this index; callers deliberately enter those
    /// providers when a route step reaches the corresponding lens endpoint.
    pub fn route(
        &self,
        from: &ResourceRef,
        to: &ResourceRef,
        max_hops: usize,
    ) -> Option<Vec<ProjectMapStep>> {
        if from == to {
            return Some(Vec::new());
        }
        if max_hops == 0 || !self.endpoints.contains_key(from) || !self.endpoints.contains_key(to) {
            return None;
        }
        let mut seen = BTreeSet::from([from.clone()]);
        let mut queue = VecDeque::from([(from.clone(), Vec::<ProjectMapStep>::new())]);
        while let Some((current, path)) = queue.pop_front() {
            if path.len() >= max_hops {
                continue;
            }
            for step in self.neighbours(&current) {
                let mut next = path.clone();
                next.push(step.clone());
                if &step.to == to {
                    return Some(next);
                }
                if seen.insert(step.to.clone()) {
                    queue.push_back((step.to, next));
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(id: &str, lens: ProjectLens, kind: ResourceKind, authority: SourceAuthority) -> ProjectMapEndpoint {
        ProjectMapEndpoint {
            resource: ResourceRef::parse(id).unwrap(),
            kind,
            lens,
            authority,
            provider: None,
            revision: None,
            label: None,
        }
    }

    #[test]
    fn cross_lens_bindings_are_reversible_without_becoming_provider_edges() {
        let mut map = ProjectMap::new();
        map.add_endpoint(endpoint(
            "wiki:node:auth",
            ProjectLens::SemanticWiki,
            ResourceKind::KnowledgeNode,
            SourceAuthority::Authored,
        ))
        .unwrap();
        map.add_endpoint(endpoint(
            "code:login",
            ProjectLens::Code,
            ResourceKind::CodeReference,
            SourceAuthority::Derived,
        ))
        .unwrap();
        map.bind(ProjectMapBinding {
            from: ResourceRef::parse("wiki:node:auth").unwrap(),
            to: ResourceRef::parse("code:login").unwrap(),
            relation: "implemented-by".into(),
            reversible: true,
            authority: SourceAuthority::Authored,
            provider: None,
            provenance: vec![],
        })
        .unwrap();

        let outward = map.neighbours(&ResourceRef::parse("wiki:node:auth").unwrap());
        assert_eq!(outward.len(), 1);
        assert!(!outward[0].reversed);
        let inward = map.neighbours(&ResourceRef::parse("code:login").unwrap());
        assert_eq!(inward.len(), 1);
        assert!(inward[0].reversed);
        assert_eq!(inward[0].relation, "implemented-by");
    }

    #[test]
    fn bounded_route_crosses_only_explicit_federation_bindings() {
        let mut map = ProjectMap::new();
        for (id, lens, kind, authority) in [
            ("wiki:node:auth", ProjectLens::SemanticWiki, ResourceKind::KnowledgeNode, SourceAuthority::Authored),
            ("source:design", ProjectLens::SourcePool, ResourceKind::KnowledgeSource, SourceAuthority::Observed),
            ("code:login", ProjectLens::Code, ResourceKind::CodeReference, SourceAuthority::Derived),
        ] {
            map.add_endpoint(endpoint(id, lens, kind, authority)).unwrap();
        }
        map.bind(ProjectMapBinding {
            from: ResourceRef::parse("wiki:node:auth").unwrap(),
            to: ResourceRef::parse("source:design").unwrap(),
            relation: "supported-by".into(),
            reversible: true,
            authority: SourceAuthority::Authored,
            provider: None,
            provenance: vec![],
        }).unwrap();
        map.bind(ProjectMapBinding {
            from: ResourceRef::parse("source:design").unwrap(),
            to: ResourceRef::parse("code:login").unwrap(),
            relation: "constrains".into(),
            reversible: true,
            authority: SourceAuthority::Authored,
            provider: None,
            provenance: vec![],
        }).unwrap();
        let route = map.route(
            &ResourceRef::parse("code:login").unwrap(),
            &ResourceRef::parse("wiki:node:auth").unwrap(),
            2,
        ).unwrap();
        assert_eq!(route.len(), 2);
        assert!(route.iter().all(|step| step.reversed));
    }
}
