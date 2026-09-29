# Agent praxis, SkillSet packages and World-facing cards

Status: architecture contract for the praxis commission of 2026-09-24.
Native owner of this document: `EpiLogos/ai-kit`. The contracts it names are
owned where each section says; this file is the joint reading that lets the
owners implement against one shape. The developmental map for landing it is
[`docs/wayfinder/praxis-architecture.md`](wayfinder/praxis-architecture.md).

## 1. The Agent as a relational whole

An Agent is read as one whole disclosed through six relations. This is a read
model assembled from existing sources — never six stores, six files or six
profile systems.

```text
0/1  AGENT        the situated whole
#0   INTENT       why am I here?            AgentProfile purpose + retained intent expression (Central)
#1   SKILL        what can I do?            unprefixed Skill descriptions (AIKit classification)
#2   METHOD       how do I act?             `METHOD:` Skill descriptions
#3   METHODOLOGY  how do I orient?          `METHODOLOGY:` Skill descriptions
#4   SKILLSET     what repertoire do I carry? AgentProfile.skill_set_refs → resolved SkillSets
#5   WORLD        where am I, with whom?    Central World + Actuation occupancy + Workcell body,
                                            composed by O:I as AgentWorldParticipation
5→0  RETURN       what changed, and which earlier determination does it pressure?
```

Human surfaces answer the short questions — *where am I? what do I carry? who
else is here? what changed? what can I do? where does this go back?* — from
this reading. They do not print `#0–#5` as decoration, and no `SKILL.md` has to
repeat the headings: the disclosure is assembled.

## 2. Praxis form: one Skill identity, three classifications

Owner: AIKit (`crates/aikit-core/src/method.rs`).

```text
PraxisForm::Skill        unprefixed description
PraxisForm::Method       description starts with `METHOD:`
PraxisForm::Methodology  description starts with `METHODOLOGY:`
```

- There is no Method or Methodology `ResourceKind`, store, trust path or
  projection lifecycle. A classified Skill is catalogued, trusted, enabled,
  projected and overlaid exactly like any other Skill.
- `METHODOLOGY:` never detects as `METHOD:` (the prefixes differ at the colon).
  A mid-description mention classifies nothing.
- A Method answers *how do these faculties, sources and operations compose for
  this class of act?* A Methodology answers *what field am I in, which kinds of
  determination exist here, which Methods apply when, and how does Return move
  through the field?* A Methodology composes and orients; it points at its
  subordinate Methods and Skills and never absorbs their bodies.
- Classification grants no authority, creates no sequence engine and orders
  nothing. The existing `SkillPraxisMetadata` situated relations apply to a
  Method- or Methodology-classified Skill alike.
- Address horizons: a Method-classified Skill also participates at `@2`; a
  Methodology-classified Skill at `@3`. Identity is unchanged.
- `aikit method list` keeps its exact contract (Methods only). `aikit praxis
  list [--form skill|method|methodology]` lists all three forms.
- `scripts/verify-native-skills.py` requires manifest and payload descriptions
  to agree on the form, not only on `METHOD:`.

## 3. SkillSet: the #4 repertoire relation

Owner: AIKit (`skillset.rs`, `aikit-store/src/skillsets.rs`,
`aikit-store/src/registry_skillsets.rs`).

The law stays `profile : resolution :: skill-set : projection`. Membership is
repertoire — never trust, authority, invocation, loading, sequencing or
precedence. A SkillSet may carry ordinary Skills, Method- and
Methodology-classified Skills, and child SkillSets.

Two kinds of child exist and both are finished:

```text
contained child   <home>/skillsets/<parent>/<child>/members      (directory nesting; existed)
referenced child  <home>/skillsets/<parent>/set.toml              children = ["documentation"]
                  <root>/skillsets/index.toml [[skillset]]        child_refs = ["central:documentation"]
```

A referenced child is resolved by name (home store) or semantic ref (registry
index) at load time; cycles and dangling refs are errors that name the path. A
referenced child is shared: two parents that carry `documentation` carry the
same set, and a revision to it reaches both. `aikit set add <set> --child
<other>` records a reference; it never copies members.

Registry SkillSets (`<root>/skillsets/index.toml`) are portable source that
travels with its repository. AIKit loads them from its own registry and from
any registered directory source whose root has a sibling `skillsets/index.toml`
(Central's `skills/` source ⇒ `Work/Central/skillsets/index.toml`). They are
addressed by semantic ref, e.g. `aikit:project-author`,
`central:documentation`, `central:core-development`.

An Agent ordinarily carries `Intent + SkillSet refs + World relation`. It does
not enumerate every Skill.

## 4. Involvement: what a disclosure may claim

Owner: AIKit (`crates/aikit-core/src/agent_praxis.rs`).

The self-disclosure distinguishes constitutive involvement from external
attribution:

```text
SOURCE        what the Skill / Agent / document says of itself (authored)
INVOLVEMENT   how that source actually participates in this act
RETURN        what happened
OBSERVATION   what another participant can establish externally
ATTRIBUTION   what another participant infers about competence or causation
RECOGNITION   what durable source is actually revised
```

Each Skill in a disclosure carries an involvement ladder. Every rung is
`true`, `false`, or `null` (not observed). A rung is only `true` with the
evidence that establishes it; nothing is inferred upward.

```text
carried        reachable through the Agent's authored SkillSets or direct refs
catalogued     present in the resolved catalogue (else: withheld, reason named)
available      passes its own trust/policy/standing gates in this context
selected       chosen for the current act (a selected Method, a Focus binding)
projected      materialised into the harness tree for this context
loaded         body entered the model context (activation receipt / hook evidence)
invoked        the Skill was actually called in this session (hook evidence)
relied_upon    the Return names it as material to the result
succeeded      the act it served produced its expected Return
verified       an independent verification obligation passed
```

`generally fit` is deliberately not a rung: fitness is attribution over many
Returns and belongs to evidence, not to self-disclosure. The same ladder
applies to knowledge sources and documents (`considered` → `retrieved` →
`relied_upon`).

Selected repertoire never implies a loaded body. Withheld members stay
withheld, with the reason. Activity and Return never rewrite source: the
disclosure reports `source_rewritten: false` and routes pressure to a Return
destination.

### Contract `aikit.agent-praxis-disclosure/v1`

`aikit praxis disclose --profile-json <file> [--activity-json <file>] [--select <skill>...]`

```json
{
  "schema": "aikit.agent-praxis-disclosure/v1",
  "agent": {"agent_ref": "", "profile_ref": "", "profile_revision": "", "name": null},
  "expression": {"purpose": null, "role": null, "intent_expression": null,
                 "authorship": null, "recognition": null},
  "repertoire": {
    "authored_skill_sets": [{"ref": "", "resolved": true, "origin": "", "description": "",
                             "revision": "", "members": [], "children": [], "withheld": []}],
    "effective_skill_sets": [""],
    "direct_skill_refs": [""],
    "method_refs": [""],
    "unresolved": [{"ref": "", "reason": ""}]
  },
  "praxis": [{"id": "", "name": "", "form": "skill|method|methodology", "position": 1,
              "payload": "", "revision": null, "via": ["set:<ref>", "direct", "method-ref"],
              "involvement": {"carried": true, "catalogued": true, "available": null,
                              "selected": false, "projected": null, "loaded": null,
                              "invoked": null, "relied_upon": null, "succeeded": null,
                              "verified": null},
              "withheld_reason": null}],
  "world": {"world_ref": "", "ratified_world_refs": [""], "scope": "personal|project"},
  "operative": {"context_id": null, "selected": [""], "loaded": [""], "invoked": [""],
                "evidence_refs": [""]},
  "return": {"destinations": [""], "activity_refs": [""], "source_rewritten": false},
  "answers": {"why_am_i_here": "", "what_can_i_do": "", "how_do_i_work": "",
              "how_do_i_orient": "", "what_do_i_carry": "", "where_am_i": "",
              "what_is_operative_now": "", "what_changed": "", "where_does_it_go_back": ""}
}
```

The profile input is Central's `central.agent-profile/v1` JSON exactly as
`ctrl --json action run agent-profile.read` returns it. `--activity-json`
accepts `aikit.praxis-activity/v1`:
`{"schema","loaded":[id],"invoked":[id],"relied_upon":[id],"succeeded":[id],"verified":[id],"evidence_refs":[ref],"return_destinations":[ref]}`.
Without it the upper rungs stay `null`.

`method_refs` on the profile remain a valid explicit-assignment convenience
over Method-classified Skill identity; they are disclosed with `via:
method-ref` and are not normalised away. New authoring prefers
`skill_set_refs`. There is no `methodology_refs`: a Methodology reached
through a carried SkillSet is classified from its ordinary Skill identity.

## 5. Portable SkillSet packages

Owner: AIKit (`crates/aikit-core/src/skillset_package/`,
`crates/aikit-cli/src/skillset_package_cli.rs`).

The native SkillSet is the authoritative source. A provider package is a
target projection of it and is never a new source SkillSet.

```text
native SkillSet (+ neutral [package] metadata)
      ↓  resolve exact member capsules + content revisions
PortableSkillPackage          aikit.portable-skill-package/v1
      ↓  PackageTarget adapter
target plan (portable | translated | target-addition | unsupported)
      ↓  render
native package tree + receipt  aikit.skillset-package-receipt/v1
      ↓  native validation / disposable discovery
```

`PortableSkillPackage`: identity (name, semantic ref), version, description,
skillset_ref, source_revision (digest over member revisions), members (id,
form, name, description, revision, payload files), references/assets/scripts
(inside each member payload), tool_dependencies, mcp_dependencies,
hook_requirements, environment requirements (names only — **no secret
values**), presentation metadata, license/attribution, and `target_overlays`.

Neutral package metadata lives beside the set: `[package]` in `set.toml` for a
home set, `[skillset.package]` in a registry index. Target-specific material
lives only under `target_overlays.<target>` and never mutates the SkillSet.

`PackageTarget` answers, per target: native package identity; where Skills
live; how MCP/tool relations are expressed; which hooks/extensions exist; the
UI contribution model; install/discovery; the validation that proves the
package usable; and which relations have no analogue. A relation the target
cannot express is returned as `unsupported` with a reason in the plan and
receipt. Nothing is dropped silently.

Targets implemented:

| target | tree | skills | MCP | hooks | validation |
| --- | --- | --- | --- | --- | --- |
| `openai` (Agent Plugins v1) | `plugin.json` (`$schema` agent-plugins 1.0.0) | `skills/<name>/` | `mcp.json` (own `$schema`; every server typed `stdio` / `streamable-http` / `sse`) | `extensions["com.openai"]` | schema-shape check; Codex marketplace load in a disposable `CODEX_HOME` when requested |
| `codex` (compat overlay) | `.codex-plugin/plugin.json` beside the portable root; interface and hooks carried in both places because Codex ignores the overlay when the root manifest has `extensions["com.openai"]` | same | overlay points at `./mcp.json` | `./hooks/hooks.json` | as above |
| `claude` | `.claude-plugin/plugin.json` (strict validation requires `author`; declare `[package.author]`) | `skills/<name>/` | `.mcp.json` | `hooks/hooks.json` | `claude plugin validate --strict --json` |
| `pi` | `package.json` with `pi` key + `pi-package` keyword | `skills/<name>/` | unsupported (pi has no MCP) | TypeScript extension under `extensions/` only when a hook/tool/command is required | `pi --mode rpc --offline get_commands` lists `skill:<name>` |

Every exported tree carries `aikit-package.json` (source identity and member revisions) and the receipt records `source_unchanged` from before/after hashes of the SkillSet and its capsules. A Skill description containing an unquoted `: ` (e.g. `METHOD: …`) is refused by `verify`: Pi silently drops such a Skill. Pi and Agent Plugins have no field for required tools or environment names; those relations are recorded as unsupported and kept in the provenance file.

Claude commands/agents and Pi extensions are generated only when the SkillSet
or the explicit export request warrants them. A documentation SkillSet exports
as Skills.

CLI: `aikit set package {inspect|plan|export|verify|diff} <set> --target <t>
[--out <dir>] [--native]`. The authoring praxis is
`skill/aikit/skillset-package-authoring` (Skill) and
`skill/aikit/skillset-package-export` (`METHOD: Export a SkillSet to a native
agent package`).

Receipt `aikit.skillset-package-receipt/v1`: source SkillSet ref, source
revision, member ids + revisions, target, target format/version, exported
files (path + sha256), translated relations, target additions, unsupported
relations, validation (command, exit, summary) and discovery evidence where
exercised.

## 6. World participation and citizenship

Owners: Central (World, AgentProfile, Positions), Actuation (Agency, authority,
occupancy/tenure), AIKit (praxis/capability disclosure), Workcell (material
presence), Factory (work custody), O:I (the composed relation and its
presentation). O:I composes; it does not copy underlying state.

`oi agent participation --agent <agent_ref> [--world <world_ref>]` returns
`oi.agent-world-participation/v1`: `agent_ref`, `world_ref`, `profile`
(ref+revision), `expression`, `roles`, `residence` (profile scope, ratified
worlds, occupied Positions), `repertoire` (the AIKit disclosure's sets and
praxis summary), `public_capabilities`, `interaction_surfaces`, `authority`
(Actuation grants / allowed-denied action summary), `material_presence`
(Workcell / tenure body facets), `relations` (other occupants, Agencies,
Projects), `availability` (tenure presence), `contribution`, `returns`,
`evidence`, `recognition` (acceptance record), `disclosure` (public / private
boundary) and `citizenship`.

`citizenship` is a vector, never a scalar authority:

```text
residence  role  repertoire  reach  authority  relation
contribution  reciprocity  reliability  recognition  continuity
```

Each dimension carries `state` (`established | partial | absent |
unavailable`), the native `basis` refs it was derived from, and a one-line
`reading`. A dimension whose owner could not be read is `unavailable` with the
failing command — never guessed. Card prose never feeds a dimension. If a
scalar is ever wanted it is derived visibly from these components and the
vector stays beneath it.

An Agent in two Worlds has two participation readings and one identity.

## 7. Two cards, one Agent

```text
                     AGENT 0/1
                        │
              AgentWorldParticipation   (O:I composes; owners hold state)
                        │
          ┌─────────────┴─────────────┐
          ▼                           ▼
HUMAN AGENT CARD               A2A AGENT CARD
oi.human-agent-card/v1         A2A v1.0.1 AgentCard (AIKit a2a.rs builder)
Cradle Agent creator/roster    /.well-known/agent-card.json when served
```

- Neither card is identity authority and neither is edited. A change to
  intent, SkillSets, participation or public disclosure re-derives both.
- The human card (`oi agent card --agent <ref> [--world <ref>]`) reads: name,
  why I'm here, what I can do, how I work, how I orient (when useful), what I
  carry, where I live/participate, citizenship summary, currently. Each field
  keeps the native refs it expands to.
- The A2A card (`aikit a2a card --participation-json <file> --interface-url
  <url>`) maps only the participation's `public_capabilities` into A2A
  `skills`. Internal repertoire is not public disclosure: carried SkillSet
  members that are not publicly disclosed never reach the card. The builder
  validates its own output with the same `assert_a2a_card` the consumer side
  uses, and records the extended-card relation explicitly
  (`capabilities.extendedAgentCard`) only when an authenticated extended card
  is actually served.

## 8. Documentation and developmental Methodologies

Owners: Central (Documentation Methodology and its Skills/Methods,
`central:documentation`); Control personal ground (adopted Wayfinder,
grilling, prototype); upstream `mattpocock/skills` (research,
domain-modeling, writing-great-skills).

```text
central:core-development        developmental repertoire
  #3 skill/personal/wayfinder          METHODOLOGY  (adopted, Central-modified)
  #2 skill/personal/grilling           METHOD       (adopted)
  #2 skill/personal/prototype          METHOD       (adopted)
  #1 skill/mattpocock/engineering/research
  #1 skill/mattpocock/engineering/domain-modeling
  #1 skill/mattpocock/productivity/writing-great-skills

central:documentation           documentation field repertoire
  #3 docs-methodology                  METHODOLOGY  (Documentation Field)
  #1 vision / design / ui-mockup / architecture / diagram authoring
  #1 documentation-standing, capability-matrices
  #2 product-, ui-, architecture-development; evidence-led repair;
     documentation reconciliation; reverse recovery; experimental development
```

Wayfinder and the Documentation Methodology compose in parallel over different
aspects of one developmental act; neither sits above the other. Wayfinder
determines the developmental field (destination, frontier, ownership,
evidence, closure); the Documentation Methodology determines the
representation/source field (Vision, Design, Mockup, Architecture, diagrams,
capability relations). Knowledge navigation and Jev narrow the context;
Methods and Skills carry the work; Factory and native products develop it;
verification returns evidence; Return revises the map and/or the
documentation field.

Progressive disclosure is the default everywhere: an Agent carries compact
descriptions; a Methodology identifies the family; a Method is selected; only
then are the required Skill bodies disclosed; Jev narrows a compact document
inventory before any large retrieval. A small local repair loads neither
Methodology and no Vision. Package membership likewise never means every body
belongs in every prompt.

Capability relations stay in `ql-capability-matrix/1`; documentation
relations ride its `extensions.documentation` object, validated by Central's
matrix tools. There is no second matrix protocol and no documentation graph.
