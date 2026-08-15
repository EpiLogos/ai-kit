//! Focused application-service extension for the V2 Project/Context/Compose workspace.
//!
//! This stays additive to the central TUI trait while #42 is being proven. The
//! production implementation is still the same [`PaletteApplicationService`] used
//! by search, contextual Actions and composition; Project-world disclosure is not
//! reconstructed by a controller or renderer.

use aikit_core::{ProjectWorldReadModel, Result};

use crate::palette_service::PaletteApplicationService;
use crate::project_world_service;

pub trait ProjectWorldApplicationService {
    fn project_world(&self) -> Result<ProjectWorldReadModel>;
}

impl ProjectWorldApplicationService for PaletteApplicationService<'_> {
    fn project_world(&self) -> Result<ProjectWorldReadModel> {
        project_world_service::project_world(self.backend())
    }
}
