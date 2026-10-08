//! One application reading for Profile resolution + additive SkillSet routing.
//! Scope declarations and generation publication keep their existing owners:
//! format-preserving profile/overlay documents, reversible Procedures and CAS.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use aikit_core::catalog::Catalog;
use aikit_core::id::ProfileId;
use aikit_core::procedure::{BlobId, Inverse, Plan, Procedure, ProcedureKind, WorldEdit};
use aikit_core::profile::{ProjectProfileFile, SessionOverlayFile};
use aikit_core::projection::{ProjectionPlan, ResolvedContext, TargetAdapter};
use aikit_core::scope::{LayerOrigin, ScopeKind, ScopeLayer};
use aikit_core::skillset::{SetMembership, SetProvenance, SkillSet};
use aikit_core::{AikitError, Result};
use aikit_store::generation::{self, GenerationBuilder};
use aikit_store::procedure::{bind_current_preconditions, bind_read_precondition, ProcedureRunner};

use super::{resolve_or_explain, ClaudeAdapter, CodexAdapter, DshAdapter, PiAdapter, Service};

pub use aikit_core::repertoire::{
    RepertoireApplication, RepertoireApplicationObservation, RepertoireMember,
    RepertoirePlanSummary, RepertoirePreview, RepertoireReading, RepertoireRequest,
    RepertoireTarget, REPERTOIRE_SCHEMA,
};

impl Service {
    fn repertoire_scope_path(&self, scope: ScopeKind) -> Result<PathBuf> {
        match scope {
            ScopeKind::Global => Ok(self.home.global_profile()),
            ScopeKind::Session => self
                .descriptor
                .session_id
                .as_ref()
                .map(|session| self.home.session_overlay(session))
                .ok_or_else(|| {
                    AikitError::new(
                        "scope.no_session",
                        "session composition requires AIKIT_SESSION_ID",
                    )
                }),
            ScopeKind::Project | ScopeKind::ProjectLocal => self
                .project
                .as_ref()
                .map(|project| {
                    project
                        .root
                        .join(".aikit")
                        .join(if scope == ScopeKind::Project {
                            "profile.toml"
                        } else {
                            "profile.local.toml"
                        })
                })
                .ok_or_else(|| {
                    AikitError::new("scope.no_project", "project composition requires a project")
                }),
            _ => Err(AikitError::new(
                "scope.unwritable",
                format!("{} is not a writable composition scope", scope.as_str()),
            )),
        }
    }

    pub fn repertoire_source_paths(&self) -> Vec<PathBuf> {
        let mut paths = vec![self.home.global_profile()];
        if let Some(project) = &self.project {
            for layer in &project.chain {
                paths.extend([layer.profile(), layer.profile_local()]);
            }
        }
        if let Some(session) = &self.descriptor.session_id {
            paths.push(self.home.session_overlay(session));
        }
        paths
    }

    /// Union scope bindings with the existing Project Specification. This never
    /// changes Profile precedence and never grants eligibility to a set member.
    pub fn selected_skill_sets(&self) -> Result<Vec<String>> {
        self.selected_skill_sets_with(None)
    }

    fn selected_skill_sets_with(&self, replacement: Option<(&Path, &str)>) -> Result<Vec<String>> {
        let mut selected: BTreeSet<String> = self.project_skill_sets().iter().cloned().collect();
        for path in self.repertoire_source_paths() {
            let supplied = replacement
                .filter(|(candidate, _)| *candidate == path)
                .map(|(_, text)| text.to_owned());
            let text = match supplied {
                Some(text) => text,
                None => match std::fs::read_to_string(&path) {
                    Ok(text) => text,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(error) => {
                        return Err(AikitError::new(
                            "composition.source_unreadable",
                            format!("{}: {error}", path.display()),
                        ))
                    }
                },
            };
            let value: toml::Value = toml::from_str(&text).map_err(|error| {
                AikitError::new(
                    "composition.source_invalid",
                    format!("{}: {error}", path.display()),
                )
            })?;
            if let Some(raw) = value.get("skill_sets") {
                let array = raw.as_array().ok_or_else(|| {
                    AikitError::new(
                        "composition.sets_invalid",
                        "skill_sets must be an array of existing set references",
                    )
                })?;
                for reference in array {
                    selected.insert(
                        reference
                            .as_str()
                            .filter(|reference| !reference.is_empty())
                            .ok_or_else(|| {
                                AikitError::new(
                                    "composition.sets_invalid",
                                    "skill_sets must contain nonempty strings",
                                )
                            })?
                            .to_owned(),
                    );
                }
            }
        }
        Ok(selected.into_iter().collect())
    }

    pub fn has_repertoire_routing(&self) -> Result<bool> {
        Ok(self.project_specification().is_some() || !self.selected_skill_sets()?.is_empty())
    }

    fn repertoire_plans(&self, context: &ResolvedContext) -> Result<Vec<ProjectionPlan>> {
        let context_dir = self.context_dir()?;
        let tree = self
            .descriptor
            .project_root
            .clone()
            .unwrap_or_else(|| self.invocation_cwd.clone());
        Ok(vec![
            Self::shell_plan(&context.view, self.secret_env_items(context)?)?,
            ClaudeAdapter::new(context_dir.join("projections/claude")).plan(context)?,
            PiAdapter::new(context_dir.join("projections/pi")).plan(context)?,
            CodexAdapter::new(tree).plan(context)?,
            DshAdapter::new(context_dir.join("projections/dsh")).plan(context)?,
        ])
    }

    fn repertoire_reading(
        &self,
        context: &ResolvedContext,
        selected: Vec<String>,
        plans: &[ProjectionPlan],
    ) -> Result<RepertoireReading> {
        let mut requested = BTreeSet::new();
        for reference in &selected {
            requested.extend(
                crate::skillset_package_cli::load_set(&self.home, reference)?
                    .0
                    .all_members(),
            );
        }
        if selected.is_empty() && self.project_specification().is_none() {
            requested.extend(
                context
                    .view
                    .active_of_kind(aikit_core::Kind::Skill)
                    .into_iter()
                    .map(|member| member.id.clone()),
            );
        }
        let members = requested
            .into_iter()
            .map(|id| {
                let entry = context.view.catalog_index.get(&id);
                let description = entry.map(|entry| entry.description.as_str()).unwrap_or("");
                let practice = if aikit_core::method::methodology_payload(description).is_some() {
                    "METHODOLOGY"
                } else if aikit_core::method::method_payload(description).is_some() {
                    "METHOD"
                } else {
                    "Skill"
                };
                let projected = context.view.is_active(&id);
                RepertoireMember {
                    revision: entry
                        .and_then(|entry| entry.revision.as_ref())
                        .map(ToString::to_string),
                    practice: practice.into(),
                    source_root: context.root_of(&id).map(|root| root.to_path_buf()),
                    projected,
                    withheld_reason: if projected {
                        None
                    } else {
                        Some(
                            context
                                .view
                                .unavailable_reason(&id)
                                .map(|reason| reason.describe())
                                .unwrap_or_else(|| {
                                    if entry.is_some() {
                                        "catalogued, but no scope enables it in this context".into()
                                    } else {
                                        "not in the accepted catalogue".into()
                                    }
                                }),
                        )
                    },
                    id,
                }
            })
            .collect();
        let profiles: BTreeSet<ProfileId> = self
            .layers
            .iter()
            .flat_map(|layer| {
                layer
                    .patch
                    .profiles
                    .iter()
                    .cloned()
                    .chain(layer.patch.uses.iter().map(|usage| usage.profile.clone()))
            })
            .collect();
        Ok(RepertoireReading {
            schema: REPERTOIRE_SCHEMA.into(),
            context_id: self.descriptor.context_id.to_string(),
            generation: generation::current(&self.context_dir()?)?,
            resolution_hash: context.view.hash.to_string(),
            profiles: profiles.into_iter().collect(),
            skill_sets: selected,
            members,
            targets: plans
                .iter()
                .map(|plan| RepertoireTarget {
                    target: plan.target.to_string(),
                    digest: plan.digest(),
                    items: plan.items.len(),
                    activation: plan.effect.clone(),
                    notes: plan.notes.clone(),
                })
                .collect(),
            package_commands: ["inspect", "plan", "export", "verify", "diff"]
                .into_iter()
                .map(|verb| {
                    format!(
                        "AIKIT_HOME={} AIKIT_CONTEXT_ID={} AIKIT_SESSION_ID={} AIKIT_PROJECT_ID={} AIKIT_TASK={} AIKIT_ISOLATION={} AIKIT_HOST={} AIKIT_MUX={} aikit -C {} set package {verb} . --target pi",
                        shell_word(&self.home.root().display().to_string()),
                        shell_word(self.descriptor.context_id.as_str()),
                        shell_word(self.descriptor.session_id.as_ref().map(|session| session.as_str()).unwrap_or("")),
                        shell_word(self.descriptor.project_id.as_ref().map(|project| project.as_str()).unwrap_or("")),
                        shell_word(self.descriptor.task.as_deref().unwrap_or("")),
                        shell_word(self.descriptor.isolation.as_str()),
                        shell_word(&self.descriptor.host),
                        shell_word(self.descriptor.mux.map(|mux| mux.as_str()).unwrap_or("")),
                        shell_word(&self.invocation_cwd.display().to_string())
                    )
                })
                .collect(),
        })
    }

    pub fn resolved_repertoire(&self) -> Result<RepertoireReading> {
        let selected = self.selected_skill_sets()?;
        let context = self.projection_context_with_sets(&self.view, &selected)?;
        let plans = self.repertoire_plans(&context)?;
        self.repertoire_reading(&context, selected, &plans)
    }

    /// Validate an actual selected application for task/review consumers.
    /// Planned digests/current IDs do not establish the retained material;
    /// the generation owner reads the lock, metadata, all plans and content
    /// under the same current-pointer CAS without changing targets or labels.
    pub fn verify_repertoire_application(&self, selected: &RepertoireReading) -> Result<()> {
        if selected.generation.is_none() || &self.resolved_repertoire()? != selected {
            return Err(AikitError::new(
                "composition.application_changed",
                "selected application no longer matches this exact context",
            ));
        }
        let context =
            self.projection_context_with_sets(&self.view, &self.selected_skill_sets()?)?;
        let plans = self.repertoire_plans(&context)?;
        if GenerationBuilder::new()
            .verify_current(
                &self.context_dir()?,
                &self.view,
                &plans,
                selected.generation.as_ref(),
            )?
            .is_none()
        {
            return Err(AikitError::new("composition.application_drift", "selected application has retained generation or projection drift; reconcile before execution"));
        }
        Ok(())
    }

    /// Verify the selected application and its actual native Procedure basis.
    /// Task consumers must retain an observed operation, not an arbitrary ID.
    pub fn verify_repertoire_procedure(
        &self,
        selected: &RepertoireReading,
        id: &aikit_core::ProcedureId,
    ) -> Result<()> {
        self.verify_repertoire_application(selected)?;
        let procedure = ProcedureRunner::new(&self.home).verify_applied(id)?;
        let owner_matches = matches!(&procedure.kind, ProcedureKind::SkillSet { operation, set } if operation == "compose" && set == &selected.skill_sets.join("+"));
        let isolation = aikit_core::procedure::MutationIsolation::Staged {
            shadow: self
                .home
                .state()
                .join("procedures/.shadow")
                .join(procedure.digest.short()),
        };
        let session_path = self
            .descriptor
            .session_id
            .as_ref()
            .map(|session| self.home.session_overlay(session));
        let session_matches = session_path.as_ref().is_some_and(|session_path| {
            procedure
                .plan
                .touched_paths()
                .iter()
                .any(|path| path == session_path)
                || procedure
                    .plan
                    .preconditions
                    .iter()
                    .any(|basis| basis.path() == session_path)
        });
        let context_dir = self.context_dir()?;
        let context_matches = procedure.plan.edits.iter().any(|edit| matches!(edit, WorldEdit::CreateLink { target, .. } if target.starts_with(&context_dir)));
        if !owner_matches
            || procedure.isolation != isolation
            || (!session_matches && !context_matches)
        {
            return Err(AikitError::new("composition.application_unbound", "Procedure is not an applied native composition for this exact Session/context basis"));
        }
        Ok(())
    }

    /// Preview one existing Profile + unioned sets without changing declarations
    /// or generation pointers. The exact Procedure/diff is the apply authority.
    pub fn preview_repertoire(&self, request: RepertoireRequest) -> Result<RepertoirePreview> {
        let path = self.repertoire_scope_path(request.scope)?;
        let mut document = aikit_store::edit::ProfileDocument::open(&path)?;
        if let Some(profile) = &request.profile {
            if self.catalog.profile(profile).is_none() {
                return Err(AikitError::new(
                    "resolution.unknown_profile",
                    profile.to_string(),
                ));
            }
            document.use_profile(profile);
        }
        let mut edit: toml_edit::DocumentMut = document
            .to_string()
            .parse()
            .map_err(|error| AikitError::new("composition.source_invalid", format!("{error}")))?;
        if request.scope == ScopeKind::Session {
            let session = self
                .descriptor
                .session_id
                .as_ref()
                .ok_or_else(|| AikitError::new("scope.no_session", "no session"))?;
            if let Some(existing) = edit.get("session_id").and_then(toml_edit::Item::as_str) {
                if existing != session.as_str() {
                    return Err(AikitError::new(
                        "edit.session_mismatch",
                        "scope belongs to another session",
                    ));
                }
            }
            edit["session_id"] = toml_edit::value(session.as_str());
        }
        if !request.skill_sets.is_empty() {
            if edit.get("skill_sets").is_none() {
                edit["skill_sets"] = toml_edit::value(toml_edit::Array::new());
            }
            let array = edit["skill_sets"].as_array_mut().ok_or_else(|| {
                AikitError::new("composition.sets_invalid", "skill_sets must be an array")
            })?;
            for reference in &request.skill_sets {
                crate::skillset_package_cli::load_set(&self.home, reference)?;
                if !array
                    .iter()
                    .any(|value| value.as_str() == Some(reference.as_str()))
                {
                    array.push(reference.as_str());
                }
            }
        }
        let text = edit.to_string();
        let patch = if request.scope == ScopeKind::Session {
            toml::from_str::<SessionOverlayFile>(&text).map(|file| file.patch)
        } else {
            toml::from_str::<ProjectProfileFile>(&text).map(|file| file.patch)
        }
        .map_err(|error| AikitError::new("composition.source_invalid", error.to_string()))?;
        let mut layers = self.layers.clone();
        layers.retain(|layer| layer.origin.to_string() != path.display().to_string());
        layers.push(ScopeLayer::new(
            request.scope,
            LayerOrigin::new(path.display().to_string()),
            patch,
        ));
        let view = resolve_or_explain(
            &self.catalog,
            &self.trust,
            &self.descriptor,
            &layers,
            &self.policy,
        )?;
        let selected = self.selected_skill_sets_with(Some((&path, &text)))?;
        let context = self.projection_context_with_sets(&view, &selected)?;
        let plans = self.repertoire_plans(&context)?;
        let current_context =
            self.projection_context_with_sets(&self.view, &self.selected_skill_sets()?)?;
        let current_plans = self.repertoire_plans(&current_context)?;
        let target_plans = plans
            .iter()
            .map(|plan| {
                RepertoirePlanSummary::from_plan(
                    plan,
                    current_plans
                        .iter()
                        .find(|current| current.target == plan.target),
                )
            })
            .collect();
        let mut reading = self.repertoire_reading(&context, selected, &plans)?;
        reading.profiles = layers
            .iter()
            .flat_map(|layer| {
                layer
                    .patch
                    .profiles
                    .iter()
                    .cloned()
                    .chain(layer.patch.uses.iter().map(|usage| usage.profile.clone()))
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let mut plan = Plan::new().with_note("compose Profile resolution and additive SkillSet routing; apply retains accepted source revisions");
        if std::fs::read(&path).ok().as_deref() != Some(text.as_bytes()) {
            plan = plan.with_edit(WorldEdit::WriteFile {
                path: path.clone(),
                contents: text.into_bytes(),
                inverse: if path.exists() {
                    Inverse::Restore {
                        blob: BlobId::deferred(),
                    }
                } else {
                    Inverse::Remove
                },
            });
        }
        let mut basis = BTreeSet::new();
        basis.extend(
            self.repertoire_source_paths()
                .into_iter()
                .filter(|path| path.exists()),
        );
        for member in &reading.members {
            if let Some(root) = &member.source_root {
                basis.insert(root.clone());
            }
        }
        for profile in &reading.profiles {
            if let Some(source) = self.profile_source(profile) {
                basis.insert(source);
            }
        }
        for reference in &reading.skill_sets {
            basis.extend(crate::skillset_package_cli::load_set(&self.home, reference)?.2);
        }
        for source in basis {
            plan = bind_read_precondition(plan, &source)?;
        }
        if self.project_specification().is_some() || !reading.skill_sets.is_empty() {
            plan = self.repertoire_discovery_plan(plan)?;
        }
        let plan = bind_current_preconditions(plan)?;
        // Composition owns reversible scope/discovery edits, never the task's
        // source branch. The runner still stages and journals every inverse.
        let shadow = self
            .home
            .state()
            .join("procedures/.shadow")
            .join(plan.digest().short());
        let procedure = Procedure::new(
            ProcedureKind::SkillSet {
                operation: "compose".into(),
                set: reading.skill_sets.join("+"),
            },
            plan,
            aikit_core::procedure::MutationIsolation::Staged { shadow },
        )?;
        let diff = ProcedureRunner::new(&self.home).diff(&procedure)?;
        Ok(RepertoirePreview {
            request,
            reading,
            procedure,
            diff,
            target_plans,
            package_command_basis: "current-scope-declarations".into(),
        })
    }

    pub fn apply_repertoire(
        &mut self,
        preview: RepertoirePreview,
    ) -> Result<RepertoireApplication> {
        let clock = std::time::Instant::now();
        let observer = generation::GenerationObserver::default();
        let result = self.apply_repertoire_observed(preview, &observer);
        let observation = RepertoireApplicationObservation {
            generation: observer.snapshot(),
            elapsed_apply_ms: u64::try_from(clock.elapsed().as_millis()).unwrap_or(u64::MAX),
        };
        match result {
            Ok(mut application) => {
                application.observation = Some(observation);
                Ok(application)
            }
            Err(error) => Err(error.with(
                "composition.application_observation",
                serde_json::to_string(&observation)
                    .expect("bounded numeric observation serializes"),
            )),
        }
    }

    fn apply_repertoire_observed(
        &mut self,
        preview: RepertoirePreview,
        observer: &generation::GenerationObserver,
    ) -> Result<RepertoireApplication> {
        crate::skill_sources::validate_central_generations(&self.home)?;
        self.refresh()?;
        let fresh = self.preview_repertoire(preview.request.clone())?;
        // Discovery created by the retained Procedure can change an advisory
        // root-discovery note. Compare accepted selection and material digests,
        // rather than that incidental observation or the current pointer.
        if preview.target_plans.len() != fresh.target_plans.len()
            || !preview
                .target_plans
                .iter()
                .zip(&fresh.target_plans)
                .all(|(reviewed, actual)| reviewed.same_material(actual))
            || preview.package_command_basis != fresh.package_command_basis
        {
            return Err(AikitError::new(
                "composition.preview_invalid",
                "proposed native target metadata changed or is absent; inspect a fresh preview before apply",
            ));
        }
        if !same_repertoire_material(&preview.reading, &fresh.reading) {
            return Err(AikitError::new(
                "composition.preview_stale",
                "selected repertoire or target plan changed after preview; inspect again",
            ));
        }
        if preview.procedure.digest != preview.procedure.plan.digest() {
            return Err(AikitError::new(
                "composition.preview_invalid",
                "preview Procedure digest does not match its reviewed plan",
            ));
        }
        if preview.procedure.plan.digest() == fresh.procedure.plan.digest() {
            // The reviewed isolation and owner attribution are part of the
            // operation too; a saved plan digest alone cannot authorise an
            // alternative shadow tree or a Git branch effect.
            if preview.procedure.kind != fresh.procedure.kind
                || preview.procedure.isolation != fresh.procedure.isolation
            {
                return Err(AikitError::new(
                    "composition.preview_invalid",
                    "saved preview changed the native operation's isolation or owner attribution",
                ));
            }
        } else {
            // A stopped operation can already have applied its declarations.
            // Only its own exact retained native Procedure authorises replay;
            // a changed supplied plan cannot smuggle unrelated edits into apply.
            let retained = ProcedureRunner::new(&self.home).load(&preview.procedure.id);
            if !retained.is_ok_and(|retained| retained == preview.procedure) {
                return Err(AikitError::new(
                    "composition.preview_stale",
                    "reviewed declaration/source preconditions changed; inspect again",
                ));
            }
        }
        let base = generation::current(&self.context_dir()?)?;
        let recovered_publication = base != preview.reading.generation;
        if recovered_publication
            && !same_repertoire_material(&self.resolved_repertoire()?, &preview.reading)
        {
            return Err(AikitError::new(
                "generation.stale_base",
                "current generation moved after repertoire preview",
            ));
        }
        // A successful earlier publication is still reconciled through the
        // generation owner's locked CAS/readback. Planned digests alone cannot
        // establish that every retained pickup remains present and correct.
        // Build from the reviewed hypothetical view before any declaration edit.
        let path = self.repertoire_scope_path(preview.request.scope)?;
        let text = preview
            .procedure
            .plan
            .edits
            .iter()
            .find_map(|edit| match edit {
                WorldEdit::WriteFile {
                    path: candidate,
                    contents,
                    ..
                } if candidate == &path => std::str::from_utf8(contents).ok(),
                _ => None,
            });
        let (view, selected) = if let Some(text) = text {
            let patch = if preview.request.scope == ScopeKind::Session {
                toml::from_str::<SessionOverlayFile>(text).map(|file| file.patch)
            } else {
                toml::from_str::<ProjectProfileFile>(text).map(|file| file.patch)
            }
            .map_err(|error| AikitError::new("composition.source_invalid", error.to_string()))?;
            let mut layers = self.layers.clone();
            layers.retain(|layer| layer.origin.to_string() != path.display().to_string());
            layers.push(ScopeLayer::new(
                preview.request.scope,
                LayerOrigin::new(path.display().to_string()),
                patch,
            ));
            (
                resolve_or_explain(
                    &self.catalog,
                    &self.trust,
                    &self.descriptor,
                    &layers,
                    &self.policy,
                )?,
                self.selected_skill_sets_with(Some((&path, text)))?,
            )
        } else {
            (self.view.clone(), self.selected_skill_sets()?)
        };
        let context = self.projection_context_with_sets(&view, &selected)?;
        let plans = self.repertoire_plans(&context)?;
        let builder = GenerationBuilder::new()
            .with_secret_resolver(std::sync::Arc::new(
                aikit_adapters::secret_resolver::SuiteSecretResolver::default(),
            ))
            .with_observer(observer.clone());
        let context_dir = self.context_dir()?;
        let retained = builder.reuse_current(&context_dir, &view, &plans, base.as_ref())?;
        let reused_generation = retained.is_some();
        let staged = if reused_generation {
            None
        } else {
            Some(builder.build(&context_dir, &view, &plans)?)
        };
        let runner = ProcedureRunner::new(&self.home);
        let outcome = runner.run(&preview.procedure)?;
        let committed = match staged {
            Some(staged) => match staged.commit(base.as_ref()) {
                Ok(committed) => committed,
                Err(error) => {
                    if !outcome.already_satisfied {
                        runner.undo(&preview.procedure.id)?;
                    }
                    self.refresh()?;
                    return Err(error);
                }
            },
            None => retained.expect("retained generation was checked above"),
        };
        self.refresh()?;
        let reading = self.resolved_repertoire()?;
        if reading.generation.as_ref() != Some(&committed.id) {
            return Err(AikitError::new(
                "generation.stale_base",
                "generation moved before composition readback; reconcile the retained operation",
            ));
        }
        Ok(RepertoireApplication {
            reading,
            procedure: preview.procedure.id.clone(),
            applied_edits: outcome.applied,
            reused_generation,
            recovered: outcome.already_satisfied || recovered_publication,
            undo: format!(
                "AIKIT_HOME={} aikit procedure undo {}",
                shell_word(&self.home.root().display().to_string()),
                preview.procedure.id
            ),
            observation: None,
        })
    }

    /// Preflight native discovery ownership before a normal apply can publish.
    pub fn validate_repertoire_discovery(&self) -> Result<()> {
        if self.has_repertoire_routing()? {
            self.repertoire_discovery_plan(Plan::new())?;
        }
        Ok(())
    }

    fn repertoire_discovery_plan(&self, mut plan: Plan) -> Result<Plan> {
        let Some(project) = &self.descriptor.project_root else {
            return Ok(plan);
        };
        let context_dir = self.context_dir()?;
        for (native, projection, label) in [
            (
                ".agents/skills",
                "current/projections/codex/.agents/skills",
                "codex",
            ),
            (".pi/skills", "current/projections/pi/.pi/skills", "pi"),
        ] {
            let path = project.join(native);
            let target = context_dir.join(projection);
            let inverse = match std::fs::symlink_metadata(&path) {
                Ok(metadata) => {
                    if !metadata.file_type().is_symlink() {
                        return Err(AikitError::new(
                            if label == "pi" {
                                "projection.pi_tree_owned"
                            } else {
                                "projection.codex_tree_owned"
                            },
                            format!("refusing to replace user-owned {}", path.display()),
                        ));
                    }
                    let existing = std::fs::read_link(&path).map_err(|error| {
                        AikitError::new("composition.discovery_unreadable", error.to_string())
                    })?;
                    if existing == target {
                        continue;
                    }
                    if !existing.starts_with(self.home.contexts()) {
                        return Err(AikitError::new(
                            if label == "pi" {
                                "projection.pi_tree_owned"
                            } else {
                                "projection.codex_tree_owned"
                            },
                            format!("{} is a foreign skill link", path.display()),
                        ));
                    }
                    Inverse::Restore {
                        blob: BlobId::deferred(),
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Inverse::Remove,
                Err(error) => {
                    return Err(AikitError::new(
                        "composition.discovery_unreadable",
                        error.to_string(),
                    ))
                }
            };
            plan = plan.with_edit(WorldEdit::CreateLink {
                path,
                target,
                inverse,
            });
        }
        Ok(plan)
    }

    /// Pi's native discovery link uses the same managed-generation ownership
    /// checks as Codex; foreign directories/links are never replaced.
    pub fn prepare_pi_project_link(&self, context_dir: &Path) -> Result<()> {
        if !self.has_repertoire_routing()? {
            return Ok(());
        }
        let Some(project) = &self.descriptor.project_root else {
            return Ok(());
        };
        let parent = project.join(".pi");
        let link = parent.join("skills");
        let target = context_dir.join("current/projections/pi/.pi/skills");
        if let Ok(metadata) = std::fs::symlink_metadata(&link) {
            if !metadata.file_type().is_symlink() {
                return Err(AikitError::new(
                    "projection.pi_tree_owned",
                    format!("refusing to replace user-owned {}", link.display()),
                ));
            }
            let existing = std::fs::read_link(&link)
                .map_err(|error| AikitError::new("projection.pi_link_failed", error.to_string()))?;
            if existing == target {
                return Ok(());
            }
            if !existing.starts_with(self.home.contexts()) {
                return Err(AikitError::new(
                    "projection.pi_tree_owned",
                    "Pi skill link is owned outside AIKit",
                ));
            }
        }
        std::fs::create_dir_all(&parent)
            .map_err(|error| AikitError::new("projection.pi_link_failed", error.to_string()))?;
        let temporary = parent.join(format!("skills.aikit-{}", ulid::Ulid::generate()));
        super::create_directory_link(&target, &temporary)?;
        if let Err(error) = std::fs::rename(&temporary, &link) {
            let _ = std::fs::remove_file(&temporary);
            return Err(AikitError::new(
                "projection.pi_link_failed",
                error.to_string(),
            ));
        }
        Ok(())
    }

    /// Transient union consumed by the existing package route, never a new set
    /// source. Withheld members remain represented by the package loader.
    pub fn repertoire_skillset(&self, reading: &RepertoireReading) -> SkillSet {
        let mut set = SkillSet::new(
            format!("repertoire-{}", self.descriptor.context_id),
            SetProvenance::Composed,
        );
        for member in &reading.members {
            set.members
                .insert(member.id.clone(), SetMembership::Explicit);
        }
        set
    }
}

#[derive(Debug, clap::Args)]
pub struct RepertoireArgs {
    /// Existing Profile to add at the selected scope.
    #[arg(long)]
    pub profile: Option<String>,
    /// Existing SkillSet to union at this scope (repeatable).
    #[arg(long = "set")]
    pub skill_sets: Vec<String>,
    #[arg(long, default_value = "project")]
    pub scope: String,
    /// Apply the exact preview through the existing reversible Procedure.
    #[arg(long)]
    pub apply: bool,
    /// Saved preview JSON (plain preview or ordinary aikit success envelope).
    #[arg(long, requires = "apply", conflicts_with_all = ["profile", "skill_sets"])]
    pub from_preview: Option<PathBuf>,
}

pub fn run(service: &mut Service, args: RepertoireArgs) -> Result<serde_json::Value> {
    if !args.apply && args.profile.is_none() && args.skill_sets.is_empty() {
        let reading = service.resolved_repertoire()?;
        let human = reading.render();
        return Ok(serde_json::json!({"reading": reading, "human": human}));
    }
    let preview = if let Some(path) = &args.from_preview {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|error| AikitError::new("composition.preview_unreadable", error.to_string()))?
            .take(1_048_577)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                AikitError::new("composition.preview_unreadable", error.to_string())
            })?;
        if bytes.len() > 1_048_576 {
            return Err(AikitError::new(
                "composition.preview_invalid",
                "preview must fit the 1MiB input bound",
            ));
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|error| AikitError::new("composition.preview_invalid", error.to_string()))?;
        let value = value.get("data").unwrap_or(&value);
        let value = value.get("preview").unwrap_or(value);
        serde_json::from_value(value.clone())
            .map_err(|error| AikitError::new("composition.preview_invalid", error.to_string()))?
    } else {
        service.preview_repertoire(RepertoireRequest {
            scope: args.scope.parse()?,
            profile: args.profile.as_deref().map(ProfileId::parse).transpose()?,
            skill_sets: args.skill_sets,
        })?
    };
    if args.apply {
        let result = service.apply_repertoire(preview)?;
        let human = result.render();
        Ok(serde_json::json!({"application": result, "reading": result.reading, "human": human}))
    } else {
        let human = format!("{}\nadd --apply to publish", preview.render());
        Ok(serde_json::json!({"reading": preview.reading, "preview": preview, "human": human}))
    }
}

/// Notes describe currently observed discovery; they carry no apply authority.
/// Every selected source revision, eligibility reading and target digest does.
fn same_repertoire_material(left: &RepertoireReading, right: &RepertoireReading) -> bool {
    let comparable = |reading: &RepertoireReading| {
        let mut value = reading.clone();
        value.generation = None;
        for target in &mut value.targets {
            target.notes.clear();
        }
        value
    };
    comparable(left) == comparable(right)
}

/// One literal shell word for copyable exact-context package commands.
pub(crate) fn shell_word(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
