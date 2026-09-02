from pathlib import Path

lib = Path("crates/aikit-core/src/lib.rs")
text = lib.read_text()

module_anchor = "pub mod context;\npub mod context_resolution;"
module_replacement = "pub mod context;\npub mod context_activation;\npub mod context_resolution;"
if text.count(module_anchor) != 1:
    raise SystemExit(f"context module anchor drifted: {text.count(module_anchor)}")
text = text.replace(module_anchor, module_replacement, 1)

export_anchor = "pub use context::{ContextBinding, ContextDescriptor, Isolation};\npub use context_resolution::{"
export_replacement = """pub use context::{ContextBinding, ContextDescriptor, Isolation};
pub use context_activation::{
    attach_context_activations, explain_context_activation, ContextActivationEvidenceBasis,
    ContextActivationExplanation, ContextActivationMode, ContextActivationReceipt,
    CONTEXT_ACTIVATION_VERSION,
};
pub use context_resolution::{"""
if text.count(export_anchor) != 1:
    raise SystemExit(f"context export anchor drifted: {text.count(export_anchor)}")
text = text.replace(export_anchor, export_replacement, 1)

for retained in ["OperativePathEvidence", "resolve_registered_credential", "SecretProvider", "SecretValue"]:
    if retained not in text:
        raise SystemExit(f"current-main export lost while composing context activation: {retained}")

lib.write_text(text)
