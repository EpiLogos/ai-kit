//! Transitional name-only export while #59 migrates old callers.
//!
//! Product semantics now live in [`crate::application_service::ApplicationService`].
//! This module owns no search, resolver, relation, history, mutation or Capsule
//! behavior and can be deleted once compatibility imports are gone.

pub use crate::application_service::ApplicationService as PaletteApplicationService;
