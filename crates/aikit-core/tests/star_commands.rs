//! Star prompt-command recognition and protocol rendering (W2, CASE 07/08).

use aikit_core::star::{
    armed, protocol, recognise, unknown_packs, Invocation, RoutingContext, StarCommand,
    CLOSEOUT_PACK,
};
use std::path::PathBuf;

fn closeout() -> Vec<StarCommand> {
    armed(&[CLOSEOUT_PACK.to_owned()])
}

fn context() -> RoutingContext {
    RoutingContext {
        project: Some("Central".to_owned()),
        central_root: Some(PathBuf::from("/Users/example/Central")),
        ctrl_bin: "ctrl".to_owned(),
        actor: Some("claude".to_owned()),
        factory_bin: "factory".to_owned(),
        factory_ledger_root: Some("/tmp/ledger".to_owned()),
        factory_run_ref: Some("run:01ARZ3NDEKTSV4RRFFQ69G5FCB".to_owned()),
    }
}

#[test]
fn the_default_composition_arms_no_star_command() {
    assert!(armed(&[]).is_empty());
    // The tokens are ordinary prose when nothing is armed — this is the
    // descope law at the recognition layer, not a separate switch.
    assert!(recognise("*end this argument now", &armed(&[])).is_empty());
}

#[test]
fn a_declared_pack_arms_its_commands_and_an_unknown_pack_is_reported_not_ignored() {
    let commands = closeout();
    assert_eq!(
        commands,
        vec![StarCommand::End, StarCommand::Fork, StarCommand::Handoff]
    );
    assert!(armed(&["not-a-pack".to_owned()]).is_empty());
    assert_eq!(
        unknown_packs(&["not-a-pack".to_owned(), CLOSEOUT_PACK.to_owned()]),
        vec!["not-a-pack".to_owned()]
    );
}

#[test]
fn an_armed_command_is_recognised_with_the_rest_of_its_line_as_its_argument() {
    let found = recognise(
        "*fork extract the pressure brackets\nand carry on",
        &closeout(),
    );
    assert_eq!(
        found,
        vec![Invocation {
            command: StarCommand::Fork,
            argument: Some("extract the pressure brackets".to_owned()),
        }]
    );
}

#[test]
fn commands_stack_in_the_order_they_appear_and_a_repeat_is_one_invocation() {
    let found = recognise(
        "*fork check the ledger *end wrap up *end again",
        &closeout(),
    );
    let commands: Vec<StarCommand> = found.iter().map(|item| item.command).collect();
    assert_eq!(commands, vec![StarCommand::Fork, StarCommand::End]);
    // A stacked argument stops at the next star token rather than swallowing it.
    assert_eq!(found[0].argument.as_deref(), Some("check the ledger"));
    assert_eq!(found[1].argument.as_deref(), Some("wrap up"));
}

#[test]
fn arithmetic_and_prose_are_not_commands() {
    for prompt in ["2*end", "rate*fork", "a**end"] {
        assert!(
            recognise(prompt, &closeout()).is_empty(),
            "{prompt} must not read as a command"
        );
    }
    assert_eq!(recognise("(*end)", &closeout()).len(), 1);
}

#[test]
fn the_end_protocol_names_every_route_its_carrier_and_the_verification() {
    let text = protocol(
        &Invocation {
            command: StarCommand::End,
            argument: None,
        },
        &context(),
    );
    // Central carries learned material and the continuation.
    assert!(text.contains("projectcentral.now.return"));
    assert!(text.contains("\"kind\":\"learning\""));
    assert!(text.contains("\"kind\":\"handoff\""));
    // The previous open handoff is superseded rather than left standing.
    assert!(text.contains("projectcentral.now.update"));
    assert!(text.contains("preserve_refs"));
    // Deferred work goes to Factory's own carrier.
    assert!(text.contains("factory development observe /tmp/ledger run:01ARZ3NDEKTSV4RRFFQ69G5FCB"));
    assert!(text.contains("recognition_required"));
    // And the close-out is checkable afterwards, against objects.
    assert!(text.contains("aikit continuity closeout verify --project Central"));
    assert!(text.contains("A close-out that starts new work has not closed anything."));
    // It never claims to have written anything itself.
    assert!(!text.to_lowercase().contains("recorded for you"));
}

#[test]
fn an_unbound_factory_ledger_is_disclosed_instead_of_printing_a_command_that_would_fail() {
    let mut context = context();
    context.factory_ledger_root = None;
    context.factory_run_ref = None;
    assert!(!context.factory_bound());
    let text = protocol(
        &Invocation {
            command: StarCommand::Fork,
            argument: Some("split the pressure work out".to_owned()),
        },
        &context,
    );
    assert!(text.contains("no Factory Run ledger is bound to this session"));
    assert!(text.contains("factory_ledger_root"));
    assert!(!text.contains("factory development observe /"));
}

#[test]
fn a_session_outside_a_project_is_told_so_rather_than_handed_a_placeholder_route() {
    let mut context = context();
    context.project = None;
    let text = protocol(
        &Invocation {
            command: StarCommand::Handoff,
            argument: Some("carry the star spine".to_owned()),
        },
        &context,
    );
    assert!(text.contains("not standing in a project of the Central world"));
}
