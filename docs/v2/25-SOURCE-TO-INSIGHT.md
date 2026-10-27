# Source to insight: a Notebook-class loop over the knowledge faculty

Status: proposed design. It changes no behaviour. It rests on [`../integrations/gemini-notebook.md`](../integrations/gemini-notebook.md) (the evidence on Gemini Notebook, each claim tagged with its firmness), on reading the code at `7482737e`, and on the DLW manual (`Work/O-I/docs/dlw/DLW.md`). Where a statement rests on reading the code rather than running it, the text says so. No ask-and-cite operation exists yet, so none was run end to end.

## 1. The gap

Gemini Notebook turns a chosen set of sources into cited answers and finished outputs, and the choosing, the asking and the making are one gesture each. AIKit already holds most of what makes such a loop trustworthy: source identity with revisions, a privacy membrane, structured absence, passage-level provenance, coverage accounting and deterministic staleness. It lacks the loop that joins them, the forms that finish it, and a way to pool sources for one question without opening a new notebook for each. This document names those three, and the changes the DLW manual needs to carry them.

## 2. What is worth taking from Notebook

| Mechanism | What it does | Firmness |
|---|---|---|
| A chosen set is the hard scope of every answer | Ticked sources are used "in either the entire set or the subset you select"; the chat request carries explicit source ids | Documented; the id list is reverse-engineered. An unpinned question drifts as a notebook grows (reported) |
| Refuse when the sources are silent | "If the answer isn't in the source material, it won't provide a response" | Documented; since June 2026 mode-dependent, because chat may also search the web and run code |
| Citations are pointers into stored text | Each citation holds a source id, a verbatim fragment, a character range and a score | Shape reverse-engineered; "pointer, not model-written quote" is inference |
| Whole sources when they fit, retrieval when they do not | The team has said a full notebook cannot be put in the window; help says many-source notebooks retrieve | Inferred from statements; chunking, embedding, ranking and the switch point are undisclosed |
| A source is usable in stages | A row appears, then becomes ready or fails; failed adds can leave ghost rows | Reverse-engineered |
| An answer saved as a note becomes a citable source | "Convert to source" gives it citations and a checkbox | Documented |
| One recipe makes every output | Sources, preset, length, language, focus prompt, a background job, the prompt kept | Parameters documented; statuses reverse-engineered |

What the evidence leaves open: chunk size, the embedding model, the full-context switch point, recall in a large notebook, and any independent comparison beyond one preprint of about 15 responses per tool (13% hallucinated against 40%). The contract is worth copying. The tuning cannot be copied, so it has to be measured.

What to avoid. There is no consumer API. The Enterprise API (v1alpha, Preview) creates notebooks, loads sources and starts one audio overview, but cannot ask a question or fetch a citation. Every consumer-side client borrows a browser session against private RPCs, which their own READMEs say may breach Google's Terms. So Notebook is at most a render target for audio, video and slides, behind a removable adapter on a dedicated account, never the system of record. Glade's rule stands: its `NotebookProvider` is "breakable, never in core" (`Work/projects/Glade/docs/ARCHITECTURE.md`).

Where AIKit is already stronger than Notebook. Notebook cannot query across notebooks, documents no staleness signal for its outputs, puts no standing on claims, says nothing about what it read, and has two refusal reasons (`UNANSWERABLE`, `FILTERED`) that live probes never saw populated. AIKit has six structured absences, deterministic staleness, standing in Technē, and coverage.

## 3. What AIKit holds, what is unwired, what is absent

Read in the code and docs at `7482737e`.

| Need | Holder | State |
|---|---|---|
| Identity that survives moves | `SourceRef` and `SourceRevision`. Central mints and owns identities; AIKit consumes them (`docs/integrations/bkmr.md`) | Built |
| Admit any file; reach it from another Project | Central `file-map.register`, `link`, `resolve`, `locate`, `scope-register`; paths outside the World need `allow_external:true` (Central `docs/integrations/BKMR-FILE-MAP.md`) | Built, for files only |
| Search across layers | `knowledge search` over the bkmr-stores, central-bkmr and work-repos providers and the native baseline. Full text always; semantic and hybrid only where embeddings were prepared (`SourceSearchMode`; `aikit-adapters/src/bkmr.rs` gates on `enable_embeddings`) | Built |
| What may leave | `SourceBinding.visibility`, `AgentVisibility`, `ExternalEgress`, and `read_for(source, target)` in `knowledge_source_pool.rs` | Built |
| Honest non-answers | `AbsenceKind` {Open, Latent, Unknown, Irrelevant, Bound, Missing} in `context_source.rs` | A type; the context pack still carries absences as text |
| A chosen whole for a present act | `knowledge frame` returns `KnowledgeContextPack` {selected, routes, readings, absences, contradictions, open_questions, budget} and stores it | Built, per query; not a kept set |
| A kept whole with a question | `WikiFrame` and constellation; members join at an exact source revision; `Inquiry {question, purpose}` is defined beside them (`knowledge_construction.rs`). The manual's Canvas declares such wholes (§17.4) | Built; shaped by QL roles. Nothing outside `knowledge_construction.rs` writes an `Inquiry`, so a question is not yet kept with a set |
| A citation as a pointer | `WikiProvenanceRef {source_ref, source_revision, selector}` with `SourceSelector::TextSpan {start, end}` in character offsets; the revision is required and an empty span is refused (`knowledge_facets.rs`, `knowledge_construction.rs`) | Built in the wiki layer. A `SourceHit` carries only a `snippet` |
| What was read | `KnowledgeCoverageStore`: per source at a content revision, Observed extents (recorded on each full read, `app/knowledge.rs`) and Declared extents, an `unreadable` flag; a new revision restarts at zero | Built; shown nowhere beside an answer |
| An output record | `WikiReading {frame_ref, reading_type, artifact_ref, derived_by_ref, provenance, revision}` | Built; `reading_type` is free text with no recipe behind it |
| Staleness without a model | `deterministic_knowledge_impact` gives `KnowledgeFreshness` {Fresh, BasisChanged, IntegrationPending, BasisUnavailable} and invokes no agent or model (`knowledge_living.rs`) | Built; the manual shows it as Knowledge route › Affected (§19.9) |

Absent:

1. **A source's life before use.** Notebook's sources go from a row to ready or failed. AIKit has `unreadable` and nothing before it.
2. **Passages.** Retrieval returns whole sources. The bkmr adapter hands each source in as one bookmark and takes a bookmark-level hit back. Whether bkmr chunks internally was not checked. Central's file map goes further: it puts only the **first 32,768 bytes** of a file's text in the bookmark description that search runs over (`ctrl/src/file_map_catalog.rs`, `TEXT_LIMIT`, applied in `description()`). On that layer nothing past the first 32 KiB of a long source can be found. The other providers were not checked.
3. **A kept source set with ticks**, whose revision an answer can cite.
4. **An ask that returns a cited answer record**: question, set revision, route, citations as `TextSpan`s, absences by kind, contradictions and coverage. Today each harness improvises its own.
5. **Intake for what is not a file**: a URL, a transcript, pasted text, audio. None of `aikit knowledge`, `aikit wiki`, `aikit capture` or Central's documented file-map actions does it.
6. **A router** between whole sources and retrieved passages, and the measurement that sets its threshold.
7. **Recipes behind `reading_type`.**

## 4. Pooling sources ad hoc

Pooling is choosing, not copying. Four of the five steps already exist.

1. **Layers that already receive context.** The Central root map, each Project's map with its specifically linked foreign sources, Work repos, bkmr stores and the wiki. `docs/v2/02` §16 lists the kinds: Project Canon, repository tree, Project Map, wiki, external documentation, papers and research collections, prior Runs, other Projects.
2. **Admit anything.** Central `register` retains a file in place and returns its `SourceRef`; `link` makes it reachable from a Project. New identities are minted here, by Central. AIKit creates no second source authority (`docs/integrations/bkmr.md`). Material that is not a file is made one first (absent item 5), then registered.
3. **Choose.** A *source set* is a named selection of `SourceRef`s, each at the revision it was chosen at, with an `Inquiry`, drawn from any layer and allowed to cross registers. It holds references and never bodies. The nearest persisted shape is the `WikiFrame` constellation with an `Inquiry`, which the Canvas's Declare reaches for. Whether a plain set may omit the QL roles (Canvas Form: None), and whether Declare writes the `Inquiry`, were not established.
4. **Scope is explicit and stored.** An answer records the set revision and the ticked members. That removes the drift Notebook shows when a question is not pinned.
5. **Visibility never widens.** Membership grants no access. For each member, `read_for(source, target)` decides what the answer's target may receive; a member that may not be sent shows as `Bound`, not as missing.

Where a set lives follows the registers: a set about one project's concern lives in that project's register; a set that crosses projects lives at the root. It is authored through the wiki's owner (Canvas › Declare), never by editing `wiki.json`.

Words. The manual already uses *pool* for the workspaces under the browser (§10.3, §16.7), and AIKit uses `SourcePool` for a provider's bindings. This document says *source set* for the chosen selection.

## 5. The loop

| Verb | What happens | State and record |
|---|---|---|
| Admit | Register the file, or make it one and then register it | Registered, then readable or `unreadable` (coverage). A `preparing` state before use is absent |
| Choose | Build the source set; tick the members for this question | Each change is a new revision of the set |
| Ask | Scope to the ticked members, retrieve, compose, answer with `TextSpan` citations | An answer record: question, set revision, route, citations, absences by kind, contradictions, coverage |
| Keep | Save an answer as a Reading whose provenance is its citations; promote it to a source through the wiki return path so later questions can cite it | Lineage kept in `derived_by_ref`; the source is never overwritten |
| Stay true | `impact` marks readings whose basis moved; `coverage` says what each considered | Knowledge route › Affected; Read again re-derives |

**The router.** If the ticked sources fit the budget, read them whole through the faculty, so that coverage is recorded for the whole extent. If not, use per-source readings as an index and retrieve passages from the ticked sources only, with hybrid search and a reranker. Published studies disagree about the crossover: long context and a RAG pipeline tie at 128k tokens in LOFT; whole sources win on comparison tasks and lose badly at saying "not in the sources" in LaRA; Anthropic advises putting a base under about 200k tokens straight in the prompt. The threshold is therefore a measurement on the owner's own corpus, not a constant.

**When the sources are silent.** The answer says which absence it is: `Unknown` when nothing supports it, `Bound` when a member could not be sent, `Missing` when expected material is absent. Notebook gives one sentence for all of them.

**No hidden spend.** Nothing in the loop calls a model unless a person or an agent explicitly asks, as in Living Knowledge (`automatic_agent_or_model_invocation: false`).

## 6. Output forms

A form is a recipe that makes a Reading of one `reading_type` from a source set. Every form shares one shape: the set and its ticks, a preset, a length, a language, a focus, a status, and the prompt kept with the result. Each form declares one grounding class and its instrument shows it:

- **Cited**: every claim points to a `TextSpan`.
- **Derived**: made from the set, carrying the set and its revision but no per-claim citations. Notebook's audio, video and slides are in this class, and its slide revisions explicitly ignore sources.
- **Proposed**: relations that wait for their owner to accept them, as Canvas lines do.

Nothing derived is shown as cited.

| Notebook output | Closest instrument in the manual | Class | State |
|---|---|---|---|
| Briefing doc, FAQ, study guide | Document: a template draws the page and entries show their sources (§17.8) | Cited | A recipe only |
| Timeline | Technē reading on the Timeline: events with standing, span and passages (§17.1, §17.5) | Cited per event | A recipe only |
| Mind map | Canvas: cards are real things, lines are proposals (§17.4); Knowledge graph (§17.3) | Proposed | A recipe only |
| Slide deck | Canvas frames, with Present (§17.4) | Derived | A recipe only |
| Audio overview | Journey with narration (§17.6) and Voice (§19.21) | Derived | A script pipeline and a voice model |
| Data table, flashcards, quiz, infographic, video | None | Cited or derived | No instrument yet |

Standing. A generated event is not a fact. One that a source states takes *Claim*; one that joins two sources takes *Inference*; the owner raises it. Notebook puts no standing on anything.

## 7. The DLW manual

### What it already carries

- **Packet** (§18.3): the passages you add, sized and shown before anything is sent; an answer shows its packet.
- **Document › Answer this** (§17.8): the answer arrives as an entry with its sources, and following a source lands on its line.
- **Knowledge route** (§19.9): sources, reach, link depth, read budget, Affected, Read again, Forget.
- **Search @** (§18.4): horizons, the kinds `source:`, `reading:` and `form:`, and `=`. `@2 reading:notes = @3 form:brief` already means "these notes, as a brief".
- **Canvas** (§17.4): a question at the centre, sources around it, Declare into the wiki.
- **Technē reading** (§17.1): "a track's material comes from its sources, each at its revision"; events carry standing and passages.

No device holds a chosen set you can tick. No view shows what an answer rested on at set level. The outputs have instruments, and nothing makes them from a set.

### Change 1: Knowledge route gains a kept source set

Knowledge route already holds Sources, Reach, Read budget, Affected, Read again and Forget, which is most of what a notebook needs, and one job is one device (§16.1). So the set extends it instead of adding a device.

*You need to choose the exact sources an answer rests on, and to see what each is and how much of it was read.* The route gains these rows. A row whose operation is absent stays dark until the operation exists (§16.13, §16.16).

| Parameter | Values | Default | What it does | Operation behind it |
|---|---|---|---|---|
| Sources | adds **Source set** to Wiki, Project knowledge, Code | as now | Lets the agent answer from the kept set | Kept set: absent (the `WikiFrame` and `Inquiry` exist) |
| Set | A named source set | None | The set the answer rests on | Absent |
| Ticks | Each member on or off | All on | The members this answer may use | Member list: `knowledge sources` (not run on a set); set revision absent |
| Depth | Whole · Passages · Auto | Auto | Whole sources when they fit, retrieved passages when not | Router absent |
| Considered (reading) | Per source, the share of its extent read at this revision, Observed or Declared | Read from AIKit | What the agent actually read | `knowledge coverage show` exists |
| Answered from (reading) | The set revision and ticked members of the last answer | Read from AIKit | What the last answer rested on | Answer record absent |

A new page, **Considered**, draws each member as a bar: read extents lit, declared extents hatched, an unreadable source dashed.

Rules:

- Ticking copies nothing.
- A set widens no one's visibility. A member the target may not receive stays in the set and shows as withheld.
- An answer states the set revision it rests on.
- A source that changes after an answer marks that answer's reading Affected.
- The set never reaches farther than Scope allows.

### Change 2: forms are Methods in the Skillset rack

Each form is a Method that makes a Reading of one type from a set. The manual already has a device for a Method, with its path and Run (§19.8), and the Skillset rack already carries Methods with a request, evidence and preview-then-apply (§19.7). So no new device kind is needed, and the name *Studio* is not used because §20.3 has it. In AIKit a Method is a skill document an agent follows; `method.rs` holds run barriers, not typed parameters. The missing piece is a small recipe contract: parameters, grounding class, status and kept prompt. Without it a form's preset, length, language and focus can only be words in the Method's text.

### Change 3: an answer shows its ground

A citation in a Document entry opens the source at its span, and the entry names the set revision it rests on. §17.8 already promises "land on the line it came from". The span layer in §3 (absent item 2) makes it true for search hits.

### What not to add

No Notebook mode, no new pool, no Studio. The browser, the pool and the Canvas keep their jobs.

## 8. Order of work

Each step ends in a demonstration on the running app, not a test alone.

1. **Measure first.** A labelled question set over one real source set, scoring whether the answering passage reaches the prompt and whether each citation supports its claim. The Return of Zero corpus is the natural first set: `knowledge_ingest.rs` records it as the design input and first client. A clone's retrieval eval raised the hit rate from 50% to 98% on its own corpus, and the gain came from measuring.
2. **Passages.** Lift the 32 KiB bound for the layers that matter, add a chunk index that keeps character offsets, and make a hit return a `TextSpan`.
3. **The kept set and the ask.** The set with ticks and revisions, and an `ask` that returns the answer record, with coverage shown beside it.
4. **Intake for non-files.** Fetch or transcribe to a file, then `register`.
5. **Forms**, in order: brief, FAQ and timeline (their instruments exist), then map and deck on the Canvas, and audio last.
6. **The manual.** Change 1 and Change 2 once steps 2 and 3 give their rows an operation. Dark rows show what is still missing.
7. **A Notebook adapter**, only if wanted: breakable, on a dedicated account, for audio, video and slides.

## 9. Not established

- Whether bkmr chunks internally, and the text bound in the providers other than Central's file map.
- Whether Canvas › Declare accepts a plain set with Form: None, and whether anything writes the `Inquiry`.
- Whether a Method can carry typed inputs. In the code read, it cannot.
- Where the full-context threshold lies. It is a measurement.
- Whether to keep the word *source set*.
