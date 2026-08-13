# Part XV — QL/MEF interoperability

## 51. Integration levels

AIKit should support four increasing depths without requiring the deepest level.

### A. Architectural grounding

The product can embody QL-aligned distinctions such as whole/context, 4+2 structural sensitivity, source/projection difference, return, and retained difference without exposing QL vocabulary or requiring a service.

### B. Passive interoperability

AIKit can carry shared references such as:

```text
QLFormRef
QLAddress
LensRef
QLTarget
QLAnnotation
```

where relevant.

### C. Explicit QL operation

An optional provider exposes operations such as:

```text
capabilities
locate
refract
relate
synthesise
```

AIKit can request a reading of existing resources without handing their ownership to the QL service.

### D. Experimental QL cognition

Separate QL-native loop providers can be selected through the common runtime/harness seam.

AIKit resolves the world required to instantiate them. It does not absorb their loop semantics.

---

## 52. QL dependency firewall

Ordinary AIKit correctness must survive total absence of a QL provider.

No live QL service may be required for:

- trust;
- availability;
- scope resolution;
- Capability dependency resolution;
- Action identity;
- ProjectBinding;
- Agent identity;
- Generation/Projection correctness;
- Procedure safety;
- Workcell materialisation.

QL-derived readings are optional, attributable, and provenance-bearing.

If a Profile or workflow explicitly requires a QL capability, its absence is handled like any other required unavailable capability rather than as a hidden system dependency.

---

## 53. MEF in AIKit

MEF is not twelve mandatory workflow stages and not twelve AIKit object types.

AIKit may use MEF through the external QL/MEF module for tasks such as:

- refracting Context or Agency;
- interpreting capability fit;
- analysing Claims/Evidence;
- model/Agency attunement;
- growth/reflection;
- context-source inquiry;
- Skill authoring/discovery research.

Any lens-related influence on resolution must be explicit and explainable. Lens semantics do not silently override trust, policy, or authored preference.

---

# Part XVI — Relation to QL-native loop experiments

## 54. Common runtime seam

The QL agent experiment programme separates a harness/host from a selectable Loop Runtime:

```text
AGENT HOST / HARNESS
        │
        ▼
   LOOP RUNTIME
    ├── classic
    └── QL
```

AIKit should resolve the inputs needed by either runtime:

```text
Agent
Agency
Project/Focus
Model
HarnessProvider
Capabilities
Actions
Context Sources
material execution requirements
optional QL provider/operator profile
```

The runtime owns recurrence semantics.

---

## 55. Experimental convergence

The Pi, Pydantic, and native experiments, plus the Deep QL branch, should converge on shared product contracts rather than merged implementation internals.

Successful experimental findings may later land as:

- a new HarnessProvider capability;
- an Agency disposition;
- a QL operator;
- a ContextResolution field;
- a Skill;
- a trace/event relation;
- a model-attunement observation.

Promotion is evidence-driven. AIKit does not predeclare where every QL experiment must end up.

---
