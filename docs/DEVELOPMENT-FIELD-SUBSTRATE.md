# Development Field operative substrate

AIKit's Development Field surface is a bounded **read/composition contract** over the resources that their native owners have already supplied. It does not make AIKit the owner of Central source identity, QL form meaning, Factory developmental meaning, Workcell lifecycle, or Actuation actuality.

The core contract is `aikit.development-field-reading/v1`. A carrier remains an ordinary `ResourceRef` and therefore stays in the existing Search/Resolve field. Its optional `aikit.development-field-binding/v1` annotation adds only owner-declared carrier kind, explicit stable-ref relations, an attributable QL `ShapeBinding` carrier when supplied, and Workcell material references. The QL seam pins the accepted owner contract `ql.structural-carrier/1.0.0` (QL-MEF #122/#137): `subject_ref`, `shape_ref`, `whole_ref`, basis/member/relation bindings, optional derivation/operator refs, Return refs and opaque caller/source/standing provenance survive intact. QL relation bindings retain their evidence refs but never auto-create AIKit semantic Resource edges. Partial/developed shapes remain valid because AIKit does not re-run or replace QL shape semantics.

## Native read

```sh
aikit --json development-field \
  --ref central:self:project \
  --ref factory:plan:42 \
  --base <exact-run-or-plan-git-revision>
```

The same application operation is `Service::development_field_read(DevelopmentFieldApplicationRequest)`. It returns owner/source/revision provenance, explicit linked refs, optional QL binding and Workcell material refs, plus the current `VersionedWorld` observation. When `--base` is supplied, the packet includes the bounded tracked difference from that revision to the current index/worktree and names untracked paths separately.

Unknown and unavailable states are first-class. In particular, until Central publishes the S1 self-description/tier binding contract into AIKit's Resource field, the `central_self_description` aperture reports `unknown`; AIKit does **not** interpret `ProjectCentral/self/**` paths as a semantic API.

## Active executable identity

Every packet reports the executable path, package version, source/developer/installed modality, and the build's source revision when that evidence existed. A build from modified tracked source sets `source_dirty: true`, so its base commit cannot be mistaken for the exact executable source.

O:I suite dispatch can pin the expected AIKit source revision:

```sh
aikit --json development-field --expect-aikit-revision <sha>
```

The command refuses an absent, different, or dirty source basis with `resource.development_field_executable_revision_mismatch`. This is the parity check that prevents a stale registered binary from silently representing current AIKit behaviour.

## Deliberate boundary

This substrate preserves the already-landed `@# - + x / =` and `@0..@5` operative Search/Resolve syntax unchanged. It does not implement the QL-MEF #123 full Vāk/C′/Ta-Onta reconciliation, Context-Frame intelligence propagation, a Development Field REPL, generated QL contemplation, familiarity routes, or a new Wiki graph. Those later layers can attach to these stable `ResourceRef`, provenance, shape-binding and Git seams without another ownership migration.
