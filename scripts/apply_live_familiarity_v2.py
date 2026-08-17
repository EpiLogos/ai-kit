from pathlib import Path

app = Path("crates/aikit-cli/src/app/mod.rs")
text = app.read_text()
needle = '''    fn scope_layers(&self) -> Option<&[ScopeLayer]> {
        Some(&self.layers)
    }

    fn documents(&self) -> Vec<SearchDoc> {
'''
replacement = '''    fn scope_layers(&self) -> Option<&[ScopeLayer]> {
        Some(&self.layers)
    }

    fn familiarity(&self) -> Result<Option<aikit_core::FamiliarityStore>> {
        match aikit_store::replay_familiarity(&self.index)? {
            aikit_store::FamiliarityReplay::Loaded { store, .. } => Ok(Some(store)),
            aikit_store::FamiliarityReplay::Invalidated { .. } => Ok(None),
        }
    }

    fn record_familiarity(
        &mut self,
        observation: aikit_core::FamiliarityObservation,
    ) -> Result<()> {
        aikit_store::append_familiarity_observation(&self.index, observation)
    }

    fn documents(&self) -> Vec<SearchDoc> {
'''
if needle not in text:
    raise SystemExit("PaletteBackend Service insertion point not found")
app.write_text(text.replace(needle, replacement, 1))

Path("crates/aikit-cli/tests/live_familiarity_v2.rs").write_text(r'''use aikit_cli::app::Service;
use aikit_core::resource::ResourceRef;
use aikit_core::{FamiliarityContext, FamiliarityObservation};
use aikit_store::AikitHome;
use aikit_tui::backend::PaletteBackend;
use tempfile::TempDir;

fn open_service(temp: &TempDir) -> Service {
    let home = AikitHome::at(temp.path().join("aikit-home"));
    Service::open(home, temp.path(), |_| None).expect("open production application service")
}

#[test]
fn production_service_persists_and_replays_actual_navigation_use() {
    let temp = TempDir::new().unwrap();
    let destination = ResourceRef::parse("wiki:node:auth").unwrap();
    let context = FamiliarityContext {
        project: None,
        actor: None,
        agency: None,
        focus: Some("knowledge-navigation".into()),
    };

    {
        let mut service = open_service(&temp);
        PaletteBackend::record_familiarity(
            &mut service,
            FamiliarityObservation::destination(
                "obs-live-1",
                destination.clone(),
                context.clone(),
                1_000,
            )
            .from_surface(ResourceRef::parse("surface/aikit/tui").unwrap()),
        )
        .unwrap();
    }

    let service = open_service(&temp);
    let learned = PaletteBackend::familiarity(&service)
        .unwrap()
        .expect("production service should replay learned accessibility");
    let assessment = learned.assess_destination(
        &destination,
        &context,
        1_000,
        aikit_core::DEFAULT_FAMILIARITY_HALF_LIFE_MS,
    );
    assert_eq!(assessment.observations, 1);
    assert_eq!(assessment.contextual_observations, 1);
}
''')
