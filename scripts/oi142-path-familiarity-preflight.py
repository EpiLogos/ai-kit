#!/usr/bin/env python3
from pathlib import Path

path = Path("scripts/oi142-path-familiarity-migrate.py")
source = path.read_text()

old = '''patch(\n    test,\n    ''' + "'''" + '''    FamiliarityObservation, FamiliaritySnapshotLoad, FamiliarityStore, FitnessEvidence,\\n    ForgetScope, RouteStepEvidence, DEFAULT_FAMILIARITY_HALF_LIFE_MS, FAMILIARITY_SCHEMA_VERSION,\\n''' + "'''" + ''',\n    ''' + "'''" + '''    FamiliarityObservation, FamiliaritySnapshotLoad, FamiliarityStore, FitnessEvidence,\\n    ForgetScope, OperativePathEvidence, RouteStepEvidence, DEFAULT_FAMILIARITY_HALF_LIFE_MS,\\n    FAMILIARITY_SCHEMA_VERSION,\\n''' + "'''" + ''',\n)\n# The actual import is line-wrapped differently on current branch; repair alternate anchor if needed.\np = Path(test)\ns = p.read_text()\nif "OperativePathEvidence" not in s.split(";", 2)[1]:\n    s = s.replace(\n        "    FamiliarityUse, FitnessEvidence, ForgetScope, RouteStepEvidence,\\n",\n        "    FamiliarityUse, FitnessEvidence, ForgetScope, OperativePathEvidence, RouteStepEvidence,\\n",\n        1,\n    )\n    p.write_text(s)\n'''
new = '''patch(\n    test,\n    ''' + "'''" + '''    AccessibilitySignalClass, FamiliarityContext, FamiliarityObservation, FamiliaritySnapshot,\\n    FamiliaritySnapshotLoad, FamiliarityStore, FitnessEvidence, ForgetScope, RouteStepEvidence,\\n    DEFAULT_FAMILIARITY_HALF_LIFE_MS, FAMILIARITY_SCHEMA_VERSION,\\n''' + "'''" + ''',\n    ''' + "'''" + '''    AccessibilitySignalClass, FamiliarityContext, FamiliarityObservation, FamiliaritySnapshot,\\n    FamiliaritySnapshotLoad, FamiliarityStore, FitnessEvidence, ForgetScope, OperativePathEvidence,\\n    RouteStepEvidence, DEFAULT_FAMILIARITY_HALF_LIFE_MS, FAMILIARITY_SCHEMA_VERSION,\\n''' + "'''" + ''',\n)\n'''
if old not in source:
    raise SystemExit("familiarity import patch block changed; re-inspect")
source = source.replace(old, new, 1)

old = "    '''    fn learned_resolve_path_breaks_an_otherwise_equal_resolution_tie() {\\n"
new = "    '''    #[test]\\n    fn learned_resolve_path_breaks_an_otherwise_equal_resolution_tie() {\\n"
if old not in source:
    raise SystemExit("ranking test patch marker changed; re-inspect")
source = source.replace(old, new, 1)

path.write_text(source)
