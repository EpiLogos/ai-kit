//! Operational Project binding, distinct from Project meaning.
//!
//! Legacy AIKit ProjectId values may be retained as migration aliases, but only
//! a caller-supplied ProjectRef can establish the Project side of the relation.

mod binding;
mod refs;

pub use binding::{ProjectBinding, ProjectBindingLocator};
pub use refs::{ProjectConstituentRef, ProjectRef};
