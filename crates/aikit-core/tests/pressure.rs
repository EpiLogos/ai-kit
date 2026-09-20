//! W1/CASE 04 — context pressure brackets, and what they may and may not bound.

use aikit_core::pressure::{
    bound, Block, Pressure, PressureBrackets, PressureSource, Reading, DEFAULT_PROMPT_BUDGET,
};

fn table(toml: &str) -> toml::value::Table {
    toml::from_str(toml).unwrap()
}

fn ordinary_block(header: &str, lines: usize) -> Block {
    Block::ordinary(
        header,
        (0..lines).map(|n| format!("  ordinary line {n}")).collect(),
    )
}

#[test]
fn the_default_brackets_are_the_programmes_and_classify_at_their_edges() {
    let brackets = PressureBrackets::default();
    assert_eq!(brackets.classify(0.0), Pressure::Fresh);
    assert_eq!(brackets.classify(0.20), Pressure::Fresh, "≤20% is fresh");
    assert_eq!(brackets.classify(0.201), Pressure::Moderate);
    assert_eq!(brackets.classify(0.45), Pressure::Moderate, "≤45%");
    assert_eq!(brackets.classify(0.46), Pressure::Depleted);
    assert_eq!(brackets.classify(0.70), Pressure::Depleted, "≤70%");
    assert_eq!(brackets.classify(0.71), Pressure::Critical, ">70%");
}

#[test]
fn the_composition_can_move_the_brackets_and_the_values_in_effect_are_printable() {
    let (brackets, warnings) = PressureBrackets::from_config(Some(&table(
        "fresh = 0.1\nmoderate = 0.3\ndepleted = 0.5\nprompt_budget = 20\n",
    )));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(brackets.classify(0.2), Pressure::Moderate);
    let described = brackets.describe();
    assert_eq!(described["fresh_max"], 0.1);
    assert_eq!(described["moderate_max"], 0.3);
    assert_eq!(described["depleted_max"], 0.5);
    assert_eq!(described["prompt_budget"], 20);
}

#[test]
fn brackets_out_of_order_are_refused_as_a_set_rather_than_half_applied() {
    let (brackets, warnings) =
        PressureBrackets::from_config(Some(&table("fresh = 0.8\nmoderate = 0.3\n")));
    assert_eq!(brackets, PressureBrackets::default(), "defaults kept whole");
    assert!(
        warnings.iter().any(|w| w.contains("out of order")),
        "{warnings:?}"
    );
}

#[test]
fn a_value_outside_the_unit_interval_is_named_in_the_warning_and_ignored() {
    let (brackets, warnings) = PressureBrackets::from_config(Some(&table("moderate = 4\n")));
    assert_eq!(brackets.moderate, PressureBrackets::default().moderate);
    assert!(
        warnings.iter().any(|w| w.contains("`moderate = 4`")),
        "{warnings:?}"
    );
}

#[test]
fn the_prompt_count_fallback_says_it_is_a_count_not_a_measurement() {
    let brackets = PressureBrackets::default();
    let reading = Reading::from_prompt_count(&brackets, 36);
    assert_eq!(reading.pressure, Pressure::Depleted);
    assert_eq!(
        reading.source,
        PressureSource::PromptCount {
            prompts: 36,
            budget: DEFAULT_PROMPT_BUDGET
        }
    );
    // The report never lets a count pass for a measurement.
    let described = reading.describe();
    assert_eq!(described["source"]["kind"], "prompt-count");
    assert_eq!(described["source"]["prompts"], 36);
}

#[test]
fn a_reported_figure_is_preferred_and_says_who_reported_it() {
    let brackets = PressureBrackets::default();
    let reading = Reading::from_reported(&brackets, 0.9, "claude reported".into());
    assert_eq!(reading.pressure, Pressure::Critical);
    assert_eq!(reading.describe()["source"]["kind"], "reported");
}

#[test]
fn fresh_bounds_nothing() {
    let blocks = vec![ordinary_block("[a]", 40), ordinary_block("[b]", 40)];
    let bounded = bound(&blocks, Pressure::Fresh);
    assert_eq!(bounded.blocks.len(), 2);
    assert_eq!(bounded.withheld_lines, 0);
    assert!(bounded.notice.is_none());
    assert!(bounded.blocks[0].lines().count() == 41);
}

#[test]
fn ordinary_payload_becomes_increasingly_bounded_as_pressure_rises() {
    let blocks = vec![
        ordinary_block("[a]", 20),
        ordinary_block("[b]", 20),
        ordinary_block("[c]", 20),
        ordinary_block("[d]", 20),
    ];
    let kept = |pressure| {
        bound(&blocks, pressure)
            .blocks
            .iter()
            .map(|block| {
                block
                    .lines()
                    .filter(|l| l.contains("ordinary line"))
                    .count()
            })
            .sum::<usize>()
    };
    let fresh = kept(Pressure::Fresh);
    let moderate = kept(Pressure::Moderate);
    let depleted = kept(Pressure::Depleted);
    let critical = kept(Pressure::Critical);
    assert!(
        fresh > moderate && moderate > depleted && depleted > critical,
        "monotonic bounding: {fresh} {moderate} {depleted} {critical}"
    );
    assert_eq!(critical, 0, "critical admits no ordinary payload");
}

#[test]
fn standing_guidance_survives_every_bracket_including_critical() {
    let block = Block {
        header: "[continuity/domain-activation] domain/release".into(),
        standing: vec!["- never ship without the gate [standing — dedup-exempt]".into()],
        ordinary: (0..20).map(|n| format!("  ordinary {n}")).collect(),
    };
    for pressure in [
        Pressure::Fresh,
        Pressure::Moderate,
        Pressure::Depleted,
        Pressure::Critical,
    ] {
        let bounded = bound(std::slice::from_ref(&block), pressure);
        assert_eq!(
            bounded.blocks.len(),
            1,
            "{pressure} dropped a standing block"
        );
        assert!(
            bounded.blocks[0].contains("never ship without the gate"),
            "{pressure} bounded standing guidance away"
        );
    }
}

#[test]
fn withholding_is_disclosed_in_place_and_a_suppressed_block_is_disclosed_for_the_turn() {
    let blocks = vec![ordinary_block("[a]", 20), ordinary_block("[b]", 20)];
    let bounded = bound(&blocks, Pressure::Depleted);
    // One block keeps a bounded head of its payload and says what it withheld.
    assert!(
        bounded.blocks[0].contains("further line(s) withheld"),
        "{:?}",
        bounded.blocks[0]
    );
    // The other carried ordinary payload only and none of it survived, so it
    // does not arrive as a bare heading — the turn's notice accounts for it.
    assert_eq!(bounded.blocks.len(), 1);
    assert_eq!(bounded.withheld_blocks, 1);
    let notice = bounded.notice.expect("a suppressed block is disclosed");
    assert!(notice.contains("1 block(s) of ordinary"), "{notice}");
    assert!(notice.contains("standing guidance is exempt"), "{notice}");
}

#[test]
fn a_block_that_carries_only_a_header_still_arrives() {
    // The turn ledger is a single line with no body; bounding must not read
    // "no payload" as "nothing to say".
    let blocks = vec![Block::ordinary("[continuity/turn-ledger] composed", vec![])];
    let bounded = bound(&blocks, Pressure::Critical);
    assert_eq!(bounded.blocks, vec!["[continuity/turn-ledger] composed"]);
    assert!(bounded.notice.is_none());
}
