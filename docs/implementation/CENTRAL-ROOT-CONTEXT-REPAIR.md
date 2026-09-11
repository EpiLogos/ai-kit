# Central root context is not a binding-free context

Central is the enclosing root meta-project. Child Project selection narrows the
working situation; a Profile selects composition. Neither creates the enclosing
World. This repair of #294 restores that distinction rather than making
`ContextResolution.project_binding` optional.

## Two existing native relations, not a synthetic child

A configured filesystem Central is recognised at its root, in Control, or at the
Work container, with the existing `control:root` protocol identity. It uses a real
local locator, without creating `.aikit`, a Profile, or ProjectCentral. An existing
child ProjectCentral manifest also participates without an AIKit profile marker.
Authored payloads are not read by location discovery. A redirected or removed root
is not silently converted to a successful binding-free resolution.

Explicit Actuation admission supplies its own World, WorldBinding, scope and
source basis. For declared root scope with no narrower local Project, the new `native-world`
locator preserves those exact references, declared revision and byte digest in
the required ProjectBinding. It makes no local-directory claim. An arbitrary
non-root World is not silently promoted into a Project; it needs its own native
Project binding. In particular,
`central:root` supplied by an admitted World is not silently renamed to Central's
`control:root` source-graph reference. The respective owners retain their refs.
When a local Central root is resolved, its local ProjectBinding remains the ground
and the independently admitted Agency/WorldBinding remains in `agency_admission`.
Root-scope admission is refused from a discovered child Project rather than
silently inheriting that child's capabilities or source scopes.

Actor bootstrap, SessionSpace evidence and Flow scope checks continue consuming
the complete required binding. Context evidence includes the native relation and
source basis; a changed basis therefore changes the SessionSpace evidence ref.
No NativeWorld locator is treated as a filesystem grant by encounter dispatch.
Native model policy, credentials, owner admission and later-turn checks remain
in the resident execution path; this repair weakens none of them.

## Verification

The existing mandatory native CAW case
`compose_cli_realisation_uses_the_same_existing_resident_target` remains profileless
and child-Project-free. It now asserts the native root identity/binding in both the
composition reading and actor bootstrap, then executes the existing selected-model
resident and verifies its Return. The new ordinary tests cover configured Central
root/Control/Work entry, markerless child identity, non-Central absence, source
privacy, changed root refusal, native binding roundtrip, bootstrap, SessionSpace
basis and Flow scope discrimination. Tests are source until an exact run reports
results; retained native campaign logs distinguish controlled providers from real
commercial-model or installed-personal-world evidence.

The root repair is in #294's existing lane. It does not close the wider CAW
programme, adopt personal Control, merge the stacked work or assert local acceptance.
