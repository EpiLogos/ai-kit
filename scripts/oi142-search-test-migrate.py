#!/usr/bin/env python3
from pathlib import Path

path = Path("crates/aikit-core/tests/search.rs")
source = path.read_text()

old = '''#[test]\nfn each_fast_prefix_maps_to_its_documented_lane() {\n    assert_eq!(parse_query(\">cargo\").prefix, Some(FastPrefix::Run));\n    assert_eq!(parse_query(\"+rust\").prefix, Some(FastPrefix::Capabilities));\n    assert_eq!(parse_query(\"@payments\").prefix, Some(FastPrefix::Sessions));\n    assert_eq!(parse_query(\":apply\").prefix, Some(FastPrefix::Manage));\n}\n'''
new = '''#[test]\nfn retained_fast_prefixes_map_to_their_documented_lanes_and_operative_tokens_do_not() {\n    assert_eq!(parse_query(\">cargo\").prefix, Some(FastPrefix::Run));\n    assert_eq!(parse_query(\":apply\").prefix, Some(FastPrefix::Manage));\n    assert_eq!(parse_query(\"+rust\").prefix, None);\n    assert_eq!(parse_query(\"@payments\").prefix, None);\n    assert!(parse_query(\"+rust\").expression.is_some());\n    assert!(parse_query(\"@payments\").expression.is_some());\n}\n'''
if old not in source:
    raise SystemExit("fast-prefix acceptance anchor changed; re-inspect before editing")
source = source.replace(old, new, 1)

old = '''#[test]\nfn the_session_prefix_only_matches_session_capsules() {\n    assert!(parse_query(\"@dev\").matches_filters(&doc(\"session/work/dev\")));\n    assert!(!parse_query(\"@dev\").matches_filters(&doc(\"script/test/dev\")));\n}\n'''
new = '''#[test]\nfn universal_address_is_not_a_session_only_filter() {\n    let query = parse_query(\"@dev\");\n    assert!(query.prefix.is_none());\n    assert!(query.expression.is_some());\n    assert!(query.matches_filters(&doc(\"session/work/dev\")));\n    assert!(query.matches_filters(&doc(\"script/test/dev\")));\n}\n'''
if old not in source:
    raise SystemExit("session-prefix acceptance anchor changed; re-inspect before editing")
source = source.replace(old, new, 1)

old = '''#[test]\nfn the_capability_and_management_prefixes_do_not_narrow_the_capsule_list() {\n    // They select a different palette source, which is the TUI's job. Core records\n    // the intent without inventing a capsule filter for it.\n    let capability = doc(\"skill/rust/review\");\n    assert!(parse_query(\"+review\").matches_filters(&capability));\n    assert!(parse_query(\":review\").matches_filters(&capability));\n}\n'''
new = '''#[test]\nfn affirm_and_management_do_not_invent_capsule_filters() {\n    // `+` is now the general Affirm relation while `:` remains the management\n    // convenience lane. Neither changes the legacy capsule filter predicate.\n    let capability = doc(\"skill/rust/review\");\n    assert!(parse_query(\"+review\").matches_filters(&capability));\n    assert!(parse_query(\":review\").matches_filters(&capability));\n}\n'''
if old not in source:
    raise SystemExit("capability-prefix acceptance anchor changed; re-inspect before editing")
source = source.replace(old, new, 1)

path.write_text(source)
