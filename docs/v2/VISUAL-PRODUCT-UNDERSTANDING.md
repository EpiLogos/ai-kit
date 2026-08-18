# AIKit V2 Visual Product Understanding

**Status:** canonical V2 product-understanding surface  
**Architecture status:** accepted `main`, including restored composition application semantics, common Explain/History, Knowledge navigation, SessionSpace, and persistent Surface ↔ Workcell material observation  
**Sources:** `01-PRODUCT-AND-OWNERSHIP.md` through `10-PERSISTENT-AGENCY-AND-MATERIAL-HOSTING.md`, current Rust crates, and accepted V2 conformance evidence.

AIKit is not primarily a catalogue. It is the system by which an actor can inhabit an operative horizon whose powers, information, body, boundaries, provenance and surfaces are discoverable and explainable without AIKit taking ownership of the things it resolves.

## 1. Experience — the actor has a resolved working world

```mermaid
flowchart TB
    ACT["Actor here and now<br/>Agent / Agency / human operator"]

    P["What can I do?<br/>Capabilities + Actions"]
    K["What can I know or ask about?<br/>Context Sources + Knowledge routes"]
    B["What body am I operating through?<br/>Model + Harness + Components"]
    F["What matters now?<br/>Project + Focus + session/task relation"]
    E["Why this world?<br/>provenance + Explain + History + boundaries"]

    ACT -->|"acts through"| P
    ACT -->|"orients through"| K
    ACT -->|"is embodied through"| B
    ACT -->|"attends within"| F
    ACT -->|"can inspect"| E

    P --> H["Resolved operative horizon"]
    K --> H
    B --> H
    F --> H
    E --> H

    H -->|"can appear through"| S["CLI · TUI · model tools · conversation · editor · API · messaging · webhook · other Surfaces"]
```

The experience is therefore not “choose something from a registry”. The actor can ask what world is available, progressively disclose what matters, and understand why a capability, source, body component or Surface is present or absent.

## 2. Product / conceptual relation — heterogeneous worlds become one explainable horizon

```mermaid
flowchart TB
    subgraph Native["Heterogeneous native authorities"]
      PS["Project and authored sources"]
      CAP["Skills · tools · Actions"]
      KNOW["SemanticWiki · SourcePool · code/source providers"]
      MOD["Models · Harnesses · Components"]
      MAT["Hosts · Workcell offers · material observations"]
    end

    RES["AIKit resolution<br/>eligibility · precedence · retrieval · ranking · composition"]
    EXP["Explanation<br/>source · reason · history · absence · revision"]
    H["Actor's current capability + information + runtime horizon"]

    PS -->|"declares or exposes"| RES
    CAP -->|"offers powers without transferring ownership"| RES
    KNOW -->|"makes a navigable horizon addressable"| RES
    MOD -->|"makes embodiment composable"| RES
    MAT -->|"reports availability and material truth"| RES

    RES -->|"produces effective bindings"| H
    RES -->|"retains reasons for"| EXP
    EXP -->|"makes the horizon intelligible"| H

    H -->|"projected without semantic retyping"| S1["Surface / Harness A"]
    H -->|"projected without semantic retyping"| S2["Surface / Harness B"]
    H -->|"projected without semantic retyping"| S3["Human TUI / CLI"]
```

The central invariant is visible in the direction of the arrows: AIKit **resolves relations among externally and internally described resources without becoming the canonical owner of either side**. One Action can appear on several Surfaces; one source can be askable without being loaded; one Agent can change body without changing identity.

## 3. Architecture — accepted V2 implementation seams

```mermaid
flowchart TB
    EXT["Native providers and source descriptors<br/>Projects · Skills · knowledge · models · harnesses · Components · Workcell observations"]

    CORE["aikit-core<br/>typed refs/resources · ContextResolution · composition · application services"]
    STORE["aikit-store<br/>durable resource state · Generations · history / observations"]
    ADAPT["aikit-adapters<br/>source · harness · provider · Workcell / target integration"]

    APP["Common application layer<br/>Search · Context · Explain · History · KnowledgeRoute · Profile/SkillSet · SessionSpace"]
    GEN["Generation / HarnessComposition / ContextDisclosure read models"]
    MAT["aikit.surface-material/v1<br/>Surface identity related to Workcell material observations"]

    CLI["aikit-cli"]
    TUI["aikit-tui"]
    TARGET["target-native harness and Surface projections"]

    EXT -->|"discovered through adapters"| ADAPT
    ADAPT -->|"normalises descriptors, not ownership"| CORE
    CORE <-->|"persists resolvable operational state"| STORE
    CORE -->|"serves shared semantics"| APP
    APP -->|"derives inspectable current world"| GEN
    ADAPT -->|"relates material observation to Surface without minting identity"| MAT
    MAT --> GEN

    GEN -->|"human machine-readable operation"| CLI
    GEN -->|"human navigation and composition"| TUI
    GEN -->|"projected through supported target seams"| TARGET
```

This is an ownership diagram as much as a component diagram. Workcell remains owner of process/service/endpoint/network/lifecycle truth; application and Project owners retain Action/source meaning; target runtimes retain their native plugin/service semantics. AIKit owns the resolution, explanation, projection and observed activation relation.

## 4. Diagram audit

| Existing visual | Class | Disposition |
|---|---|---|
| `01-PRODUCT-AND-OWNERSHIP.md` Human/Project Sources → AIKit → Harness/Factory/Workcell → Execution | cross-product conceptual | **Preserve, but not as the first product picture.** It locates AIKit among neighbours rather than showing the actor's experienced horizon. |
| `02-RESOLUTION-AND-CONTEXT-COGNITION.md` authored preference → resolution → effective binding | specialist conceptual | **Preserve.** It explains a critical resolution law. |
| disclosure ladder EXISTS → KNOWN → ASKABLE → RETRIEVED → FOCUSED | experiential/epistemic specialist | **Preserve.** It is a valuable detailed view inside the information-horizon relation. |
| operational-state ladders catalogued → eligible → enabled → projected → loaded → invoked | architectural specialist | **Preserve.** They explain state semantics, not whole-product meaning. |
| `03-ACTOR-RUNTIME-AND-PROJECTION.md` Agent/Agency/Session/Execution and HarnessComposition diagrams | conceptual/architecture | **Preserve.** They remain the correct identity/body deepening after the resolved-world diagram. |
| older `docs/ARCHITECTURE.md` / prior-art diagrams | historical / implementation lineage | **Do not promote over V2.** Retain for provenance where still useful. |

## 5. Verification

**Semantic:** the first diagram is an operative horizon, not a catalogue. The conceptual diagram shows why resolution exists and why multiple Surfaces do not imply multiple semantic owners.

**Implementation:** the architecture reflects accepted current V2 seams, including common Explain/History and Surface-material observation. It does not claim Workcell material lifecycle ownership, application Action ownership, or a universal target plugin protocol.

**Cross-product:** AIKit is not Central: it resolves authored sources rather than owning durable human truth. It is not Actuation: it resolves the body/world of situated agency rather than defining agency's determination/Return grammar. It is not Workcell: it asks what operative world should be available; Workcell makes material execution true.

## 6. Public-site projection

Project the **resolved operative horizon** as the primary public/design image. The heterogeneous-input conceptual relation is suitable for a deeper “how AIKit works” explainer. The crate/application architecture should remain technical documentation, with richer UI designs derived from the same semantic relations rather than turning resource cards into the product metaphor.