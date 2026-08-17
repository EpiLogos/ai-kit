//! Durable AIKit-owned Knowledge operation history.
//!
//! Provider-native Wiki/Source/Code truth remains in its provider. This store
//! records only the operational evidence AIKit itself owns: routes an actor
//! actually traversed and context frames it actually materialised. The receipts
//! retain provider/lens/revision/authority evidence already present on those
//! operation results; they never become a second semantic graph.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use aikit_core::knowledge::{KnowledgeContextPack, KnowledgeRoute};
use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, FamiliarityContext, KnowledgeAddress, KnowledgeSearchHit, Result};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::AikitHome;

pub const KNOWLEDGE_APPLICATION_STORE_VERSION: &str = "aikit.knowledge-application-history/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KnowledgeHistoryOperation {
    Route,
    Frame,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeApplicationReceipt {
    pub schema: String,
    pub receipt_id: String,
    pub sequence: u64,
    pub recorded_at_ms: u64,
    pub operation: KnowledgeHistoryOperation,
    pub context: FamiliarityContext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<KnowledgeRoute>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<KnowledgeContextPack>,
}

impl KnowledgeApplicationReceipt {
    pub fn touches(&self, resource: &ResourceRef) -> bool {
        self.route.as_ref().is_some_and(|route| {
            route.route == *resource || route.steps.iter().any(|step| step.resource == *resource)
        }) || self.frame.as_ref().is_some_and(|frame| {
            frame.selected.iter().any(|candidate| candidate == resource)
                || frame.routes.iter().any(|route| {
                    route.route == *resource
                        || route.steps.iter().any(|step| step.resource == *resource)
                })
                || frame
                    .readings
                    .iter()
                    .any(|reading| reading.resource == *resource)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct KnowledgeApplicationState {
    schema: String,
    next_sequence: u64,
    #[serde(default)]
    receipts: Vec<KnowledgeApplicationReceipt>,
    /// Rebuildable provider-address bindings observed from live Knowledge search.
    /// Canonical ResourceRef/provider truth remains outside this cache.
    #[serde(default)]
    addresses: BTreeMap<ResourceRef, KnowledgeAddress>,
}

impl Default for KnowledgeApplicationState {
    fn default() -> Self {
        Self {
            schema: KNOWLEDGE_APPLICATION_STORE_VERSION.into(),
            next_sequence: 1,
            receipts: Vec::new(),
            addresses: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct KnowledgeApplicationStore {
    home: AikitHome,
}

impl KnowledgeApplicationStore {
    pub fn new(home: AikitHome) -> Self {
        Self { home }
    }

    pub fn remember_search_hits(&self, hits: &[KnowledgeSearchHit]) -> Result<()> {
        let mut state = self.load()?;
        for hit in hits {
            state
                .addresses
                .insert(hit.resource.clone(), hit.address.clone());
        }
        self.save(&state)
    }

    pub fn address(&self, resource: &ResourceRef) -> Result<Option<KnowledgeAddress>> {
        Ok(self.load()?.addresses.get(resource).cloned())
    }

    pub fn append_route(&self, route: KnowledgeRoute) -> Result<KnowledgeApplicationReceipt> {
        let context = route.context.clone();
        self.append(KnowledgeHistoryOperation::Route, context, Some(route), None)
    }

    pub fn append_frame(&self, frame: KnowledgeContextPack) -> Result<KnowledgeApplicationReceipt> {
        let context = frame.context.clone();
        self.append(KnowledgeHistoryOperation::Frame, context, None, Some(frame))
    }

    pub fn history(
        &self,
        context: Option<&FamiliarityContext>,
        resource: Option<&ResourceRef>,
    ) -> Result<Vec<KnowledgeApplicationReceipt>> {
        let state = self.load()?;
        let mut receipts = state
            .receipts
            .into_iter()
            .filter(|receipt| context.is_none_or(|wanted| &receipt.context == wanted))
            .filter(|receipt| resource.is_none_or(|wanted| receipt.touches(wanted)))
            .collect::<Vec<_>>();
        receipts.sort_by(|left, right| right.sequence.cmp(&left.sequence));
        Ok(receipts)
    }

    fn append(
        &self,
        operation: KnowledgeHistoryOperation,
        context: FamiliarityContext,
        route: Option<KnowledgeRoute>,
        frame: Option<KnowledgeContextPack>,
    ) -> Result<KnowledgeApplicationReceipt> {
        let mut state = self.load()?;
        let sequence = state.next_sequence;
        state.next_sequence = state.next_sequence.saturating_add(1);
        let receipt = KnowledgeApplicationReceipt {
            schema: KNOWLEDGE_APPLICATION_STORE_VERSION.into(),
            receipt_id: format!("knowledge-receipt/{}", Ulid::generate()),
            sequence,
            recorded_at_ms: now_ms(),
            operation,
            context,
            route,
            frame,
        };
        state.receipts.push(receipt.clone());
        self.save(&state)?;
        Ok(receipt)
    }

    fn path(&self) -> PathBuf {
        self.home.state().join("knowledge/application-history.json")
    }

    fn load(&self) -> Result<KnowledgeApplicationState> {
        let path = self.path();
        if !path.exists() {
            return Ok(KnowledgeApplicationState::default());
        }
        let bytes = fs::read(&path)
            .map_err(|error| io_error("knowledge.history_read_failed", &path, error))?;
        let state: KnowledgeApplicationState = serde_json::from_slice(&bytes).map_err(|error| {
            AikitError::new(
                "knowledge.history_decode_failed",
                format!("could not decode {}: {error}", path.display()),
            )
            .with("path", path.display().to_string())
        })?;
        if state.schema != KNOWLEDGE_APPLICATION_STORE_VERSION {
            return Err(AikitError::new(
                "knowledge.history_schema_mismatch",
                format!(
                    "Knowledge history schema {} is not supported; expected {}",
                    state.schema, KNOWLEDGE_APPLICATION_STORE_VERSION
                ),
            )
            .with("path", path.display().to_string()));
        }
        Ok(state)
    }

    fn save(&self, state: &KnowledgeApplicationState) -> Result<()> {
        let path = self.path();
        let parent = path.parent().expect("Knowledge history path has a parent");
        fs::create_dir_all(parent)
            .map_err(|error| io_error("knowledge.history_prepare_failed", parent, error))?;
        let bytes = serde_json::to_vec_pretty(state).map_err(|error| {
            AikitError::new(
                "knowledge.history_encode_failed",
                format!("could not encode Knowledge history: {error}"),
            )
        })?;
        let temp = parent.join(format!(".application-history-{}.tmp", Ulid::generate()));
        {
            let mut file = fs::File::create(&temp)
                .map_err(|error| io_error("knowledge.history_write_failed", &temp, error))?;
            file.write_all(&bytes)
                .map_err(|error| io_error("knowledge.history_write_failed", &temp, error))?;
            file.sync_all()
                .map_err(|error| io_error("knowledge.history_write_failed", &temp, error))?;
        }
        fs::rename(&temp, &path)
            .map_err(|error| io_error("knowledge.history_commit_failed", &path, error))?;
        Ok(())
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or_default()
}

fn io_error(code: &'static str, path: &std::path::Path, error: std::io::Error) -> AikitError {
    AikitError::new(code, format!("{}: {error}", path.display()))
        .with("path", path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::knowledge::{KnowledgeReading, KnowledgeRouteStep};
    use aikit_core::resource::SourceAuthority;
    use tempfile::TempDir;

    fn r(raw: &str) -> ResourceRef {
        ResourceRef::parse(raw).unwrap()
    }

    #[test]
    fn route_and_frame_receipts_survive_reopen_and_filter_by_resource() {
        let temp = TempDir::new().unwrap();
        let store = KnowledgeApplicationStore::new(AikitHome::at(temp.path()));
        let context = FamiliarityContext {
            project: Some(r("project/app")),
            actor: None,
            agency: None,
            focus: Some("auth".into()),
        };
        let mut route = KnowledgeRoute::new(r("knowledge-route/test"), context.clone());
        route.steps.push(KnowledgeRouteStep {
            resource: r("wiki:node:auth"),
            provider: None,
            lens: Some("semantic-wiki".into()),
            transition: None,
            revision: Some("1".into()),
            authority: SourceAuthority::Authored,
        });
        store.append_route(route.clone()).unwrap();

        let mut frame = KnowledgeContextPack::new(context.clone());
        frame.selected.push(r("wiki:node:auth"));
        frame.routes.push(route);
        frame.readings.push(KnowledgeReading {
            resource: r("wiki:node:auth"),
            provider: None,
            lens: Some("semantic-wiki".into()),
            revision: Some("1".into()),
            freshness: None,
            authority: SourceAuthority::Authored,
            content: Some("auth".into()),
            evidence: Vec::new(),
            why_selected: "test".into(),
        });
        store.append_frame(frame).unwrap();

        let reopened = KnowledgeApplicationStore::new(AikitHome::at(temp.path()));
        let history = reopened
            .history(Some(&context), Some(&r("wiki:node:auth")))
            .unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].operation, KnowledgeHistoryOperation::Frame);
        assert_eq!(history[1].operation, KnowledgeHistoryOperation::Route);
    }
}
