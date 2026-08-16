//! Project-world extension of the canonical V2 application service.
//!
//! Project/Context/Compose disclosure is produced from the same live backend used
//! by Search, Actions and composition. Controllers and renderers consume this read
//! model; they do not reconstruct context or own another resolver.

use aikit_core::{ProjectWorldReadModel, Result};

use crate::application_service::ApplicationService;
use crate::project_world_service;

pub trait ProjectWorldApplicationService {
    fn project_world(&self) -> Result<ProjectWorldReadModel>;
}

impl ProjectWorldApplicationService for ApplicationService<'_> {
    fn project_world(&self) -> Result<ProjectWorldReadModel> {
        project_world_service::project_world(self.backend())
    }
}
