//! Package-backed Capability state helper.
//!
//! Canonical staging lives in [`crate::application::StagedChanges`] and follows
//! the one staging -> preview/explain -> confirm -> apply route. This helper is
//! intentionally limited to asking whether a proven package-backed Capability is
//! effectively on; it owns no staged set, preview, resolver or mutation path.

use aikit_core::id::CapsuleId;
use aikit_core::resolve::ResolvedView;

pub fn is_on(view: &ResolvedView, id: &CapsuleId) -> bool {
    view.is_active(id) || view.is_declared_enabled(id)
}
