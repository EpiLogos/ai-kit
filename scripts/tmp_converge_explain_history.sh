#!/usr/bin/env bash
set -euo pipefail

branch="${GITHUB_HEAD_REF:-${GITHUB_REF_NAME:-agent/explain-history-final-convergence}}"
source_branch="origin/agent/v2-explain-history-evidence-convergence"

git config user.name github-actions[bot]
git config user.email 41898282+github-actions[bot]@users.noreply.github.com
git fetch origin agent/v2-explain-history-evidence-convergence

# Copy only the accepted PR #86 implementation modules/tests and its native Skill
# projection. Current application/lib ownership stays on this branch and is merged
# explicitly below.
git checkout "$source_branch" -- \
  crates/aikit-cli/src/bin/aikit-explain-history.rs \
  crates/aikit-core/src/composition_explain_history.rs \
  crates/aikit-core/src/explain_history.rs \
  crates/aikit-core/src/explain_history_actions.rs \
  crates/aikit-core/src/live_activation_history.rs \
  crates/aikit-store/src/generation_history.rs \
  crates/aikit-store/src/history_evidence.rs \
  crates/aikit-store/src/procedure_history.rs \
  crates/aikit-store/tests/generation_history_v2.rs \
  crates/aikit-tui/src/explain_history_service.rs \
  crates/aikit-tui/tests/explain_history_action_parity_v2.rs \
  skills/registry/capsules/skill/aikit/operation/payload/SKILL.md

python3 - <<'PY'
from pathlib import Path

def insert_after(path, needle, addition):
    p = Path(path)
    text = p.read_text()
    if addition.strip() in text:
        return
    if needle not in text:
        raise SystemExit(f"insertion point missing in {path}: {needle!r}")
    p.write_text(text.replace(needle, needle + addition, 1))

core = 'crates/aikit-core/src/lib.rs'
insert_after(core, 'pub mod composition;\n', 'pub mod composition_explain_history;\n')
insert_after(core, 'pub mod error;\n', 'pub mod explain_history;\npub mod explain_history_actions;\n')
insert_after(core, 'pub mod lifecycle;\n', 'pub mod live_activation_history;\n')
insert_after(
    core,
    'pub use composition::{\n    resolve_harness_composition, ActivationScope, ActivationScopeKind, ComponentBinding,\n    ComponentContribution, ComponentDescriptor, ComponentRequirement, ComponentSelection,\n    CompositionAbsence, CompositionActivationMode, CompositionCatalog, CompositionRelationKind,\n    CompositionState, ContractBinding, ContractProvider, ContributionKind, HarnessComposition,\n    HarnessCompositionRequest, LifetimeOwner, LifetimeOwnerKind, ProjectionBinding,\n    RequirementStrength, ResolutionScope, RetractionMode, SurfaceDescriptor, SurfaceKind,\n    TargetNativeComponentBinding, HARNESS_COMPOSITION_VERSION,\n};\n',
    'pub use composition_explain_history::{\n    explain_harness_component, explain_harness_composition_preview,\n};\n'
)
insert_after(
    core,
    'pub use effects::{EffectClass, Effects};\n',
    'pub use explain_history::{\n    explain_resource_evidence, familiarity_history_evidence,\n    harness_composition_history_evidence, EvidenceProvenance, ExplainEvidence, ExplainFact,\n    HistoryEvidence, HistoryKind, HistoryReadModel, HistoryRecoverability,\n    EXPLAIN_HISTORY_VERSION,\n};\npub use explain_history_actions::{\n    explain_history_action_resources, explain_history_actions_for,\n    install_explain_history_actions, EXPLAIN_ACTION_REF, HISTORY_ACTION_REF,\n};\n'
)
insert_after(core, 'pub use lifecycle::{CapabilityLifecycle, LifecycleThresholds};\n', 'pub use live_activation_history::live_activation_history_evidence;\n')

store = 'crates/aikit-store/src/lib.rs'
insert_after(store, 'pub mod generation;\n', 'pub mod generation_history;\npub mod history_evidence;\n')
insert_after(store, 'pub mod procedure;\n', 'pub mod procedure_history;\n')
insert_after(store, 'pub use generation::{CommittedGeneration, GenerationBuilder, GenerationMetadata, StagedGeneration};\n', 'pub use generation_history::{compare_generation_worlds, GenerationWorldComparison};\npub use history_evidence::{\n    familiarity_history_evidence_model, generation_history_evidence,\n    session_space_history_evidence, session_space_receipt_evidence,\n};\n')
insert_after(store, 'pub use procedure::{\n    plan_procedure, EditDiff, ProcedureDiff, ProcedureOutcome, ProcedureRunner,\n};\n', 'pub use procedure_history::procedure_history_evidence;\n')

tui = 'crates/aikit-tui/src/lib.rs'
insert_after(tui, 'pub mod event;\n', 'pub mod explain_history_service;\n')
insert_after(tui, 'pub use event::{EventSource, PaletteEvent, ScriptedEvents};\n', 'pub use explain_history_service::ExplainHistoryApplicationService;\n')
PY

rustfmt \
  crates/aikit-core/src/composition_explain_history.rs \
  crates/aikit-core/src/explain_history.rs \
  crates/aikit-core/src/explain_history_actions.rs \
  crates/aikit-core/src/live_activation_history.rs \
  crates/aikit-store/src/generation_history.rs \
  crates/aikit-store/src/history_evidence.rs \
  crates/aikit-store/src/procedure_history.rs \
  crates/aikit-tui/src/explain_history_service.rs

cargo test -p aikit-core composition_explain_history
cargo test -p aikit-core explain_history
cargo test -p aikit-store --test generation_history_v2
cargo test -p aikit-tui --test explain_history_action_parity_v2
cargo test -p aikit-cli --bin aikit-explain-history
cargo clippy --workspace --all-targets -- -D warnings

rm -f .github/workflows/tmp-converge-explain-history.yml scripts/tmp_converge_explain_history.sh
git add -A
git commit -m "chore: remove temporary Explain History convergence machinery"
git push origin HEAD:"$branch"
