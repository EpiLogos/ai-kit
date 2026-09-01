#!/usr/bin/env python3
from pathlib import Path

path = Path("crates/aikit-adapters/src/gateway_service.rs")
source = path.read_text()
old = '''        let gateway = gateway();\n        persist_gateway_state(&gateway, Some(&state)).unwrap();\n        let encoded = fs::read_to_string(&state).unwrap();\n        assert!(!encoded.contains("pid"));\n        assert!(!encoded.contains("socket"));\n        assert!(!encoded.contains("workcell"));\n        let restored = restore_gateway_state(gateway(), Some(&state)).unwrap();\n        assert_eq!(restored.status(), gateway.status());\n'''
new = '''        let original_gateway = gateway();\n        persist_gateway_state(&original_gateway, Some(&state)).unwrap();\n        let encoded = fs::read_to_string(&state).unwrap();\n        assert!(!encoded.contains("pid"));\n        assert!(!encoded.contains("socket"));\n        assert!(!encoded.contains("workcell"));\n        let restored = restore_gateway_state(gateway(), Some(&state)).unwrap();\n        assert_eq!(restored.status(), original_gateway.status());\n'''
if old not in source:
    raise SystemExit("current-main gateway test baseline anchor changed; re-inspect before editing")
path.write_text(source.replace(old, new, 1))
