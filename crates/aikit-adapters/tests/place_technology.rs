//! The place-technology registry: open names resolve to detection and, where
//! this build has one, a mux adapter. An unregistered name stays a declared
//! fact — `resolve` says None, nothing refuses the name, nothing falls back.

use aikit_core::platform::{MuxKind, PlaceTechnology};
use aikit_core::Result;

use aikit_adapters::place_technology::{
    PlaceTechnologyAdapter, PlaceTechnologyReading, PlaceTechnologyRegistry, MuxAdapterHandle,
};

/// A fake technology with a scripted detection answer, so these tests pin the
/// registry contract without depending on any multiplexer being installed.
struct FakeTechnology {
    name: &'static str,
    installed: bool,
    hosts_field: bool,
}

impl PlaceTechnologyAdapter for FakeTechnology {
    fn technology(&self) -> PlaceTechnology {
        PlaceTechnology::new(self.name)
    }

    fn detect(&self) -> Result<PlaceTechnologyReading> {
        if self.installed {
            Ok(PlaceTechnologyReading {
                technology: self.technology(),
                installed: true,
                version: Some("fake 1.0".into()),
                server_running: true,
                inside: false,
                detail: None,
            })
        } else {
            Ok(PlaceTechnologyReading::absent(
                self.technology(),
                "`fake` is not on PATH",
            ))
        }
    }

    fn mux_adapter(&self) -> Option<MuxAdapterHandle> {
        // The fake names a technology this build cannot drive: the registry
        // entry is honest about detection while declaring no adapter.
        None
    }

    fn hosts_working_field(&self) -> bool {
        self.hosts_field
    }
}

#[test]
fn the_builtin_registry_detects_tmux_cmux_herdr_and_registers_plain() {
    let registry = PlaceTechnologyRegistry::builtin();
    let readings = registry.detect_all().expect("detection never fails the read");
    let names: Vec<&str> = readings
        .iter()
        .map(|reading| reading.technology.as_str())
        .collect();
    assert_eq!(names, vec!["tmux", "cmux", "herdr", "plain"]);

    // Plain is always present: it is the terminal this process is in.
    let plain = &readings[3];
    assert!(plain.installed);
    assert_eq!(plain.technology, PlaceTechnology::plain());
    assert_eq!(plain.technology.known(), Some(MuxKind::Plain));

    // herdr's reading is a real probe of this host: installed with a
    // version, or absent with the reason — never a fabricated middle.
    let herdr = &readings[2];
    assert_eq!(herdr.technology, PlaceTechnology::herdr());
    assert!(
        herdr.installed ^ herdr.detail.is_some(),
        "herdr is either installed or honestly absent, not both: {:?}",
        herdr
    );
}

#[test]
fn the_working_field_enumerates_only_technologies_that_host_a_switchable_world() {
    let registry = PlaceTechnologyRegistry::builtin();
    let field = registry.detect_field().expect("detection never fails");
    let names: Vec<&str> = field.iter().map(|r| r.technology.as_str()).collect();
    // herdr owns a switchable world, so the field now names it alongside
    // tmux and cmux; plain is never a field row.
    assert_eq!(names, vec!["tmux", "cmux", "herdr"]);

    // Detection is a real probe of this host: each reading says installed or
    // carries the reason it is not.
    for reading in &field {
        if !reading.installed {
            assert!(
                reading.detail.is_some(),
                "{} is absent with no reason given",
                reading.technology
            );
        }
    }
}

#[test]
fn a_registered_technology_resolves_and_an_unregistered_name_stays_first_class() {
    let registry = PlaceTechnologyRegistry::builtin().with_entry(Box::new(FakeTechnology {
        // Not a builtin name: the builtins already carry herdr, and
        // `with_entry` appends rather than replaces.
        name: "weave",
        installed: true,
        hosts_field: true,
    }));

    // An open name this build registers resolves — detection works even where
    // no mux adapter exists yet.
    let weave = PlaceTechnology::new("weave");
    let entry = registry
        .resolve(&weave)
        .expect("a registered technology resolves by name");
    assert_eq!(entry.technology(), weave);
    let reading = entry.detect().unwrap();
    assert!(reading.installed);
    assert_eq!(reading.version.as_deref(), Some("fake 1.0"));
    assert!(
        entry.mux_adapter().is_none(),
        "the fake declares no adapter: known-and-detected is not the same as drivable"
    );

    // An unregistered name is not an error and not a fallback — just absent.
    let unknown = PlaceTechnology::new("future-mux");
    assert!(registry.resolve(&unknown).is_none());
}

#[test]
fn a_registered_absent_technology_reports_its_reason_not_a_failure() {
    let registry = PlaceTechnologyRegistry::builtin().with_entry(Box::new(FakeTechnology {
        name: "ghost",
        installed: false,
        hosts_field: true,
    }));
    let readings = registry.detect_all().unwrap();
    let ghost = readings
        .iter()
        .find(|reading| reading.technology.as_str() == "ghost")
        .unwrap();
    assert!(!ghost.installed);
    assert_eq!(ghost.detail.as_deref(), Some("`fake` is not on PATH"));
    // It also takes its place in the field enumeration (installed filtering is
    // the field's business, not the registry's).
    assert!(registry.detect_field().unwrap().iter().any(|r| r.technology.as_str() == "ghost"));
}

#[test]
fn plain_never_hosts_a_working_field_even_when_registered() {
    let registry = PlaceTechnologyRegistry::builtin();
    let entry = registry
        .resolve(&PlaceTechnology::plain())
        .expect("plain is registered");
    assert!(!entry.hosts_working_field());
    assert!(
        entry.mux_adapter().is_some(),
        "plain still hands back its adapter for the paths that drive it"
    );
}
