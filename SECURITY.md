# Security policy

AIKit is currently a production-oriented alpha and has not yet published a
stable release line. Security fixes are applied to the latest `main` branch.

## Reporting a vulnerability

Do not open a public issue for a vulnerability involving secret exposure,
configuration takeover, path traversal, trust bypass, or destructive session
control.

Use GitHub's private vulnerability reporting for this repository:

1. Open the repository's **Security** tab.
2. Choose **Report a vulnerability**.
3. Include the affected commit, platform, reproduction, impact, and any
   evidence that the issue crosses an authority boundary.

If private vulnerability reporting is not available to you, contact the
repository owner through the private channel already used to grant repository
access.

You should receive an acknowledgement within seven days. A fix timeline depends
on severity and whether coordinated disclosure is needed.

## Security model

AIKit's important boundaries are:

- discovery must not silently become adoption;
- unreviewed or quarantined behaviour must not become active;
- secrets must not enter registries, event logs, previews, or generated state;
- external configuration changes must be previewable and reversible;
- multiplexer teardown must remain bounded by durable AIKit ownership;
- project, session, and task state must not leak across contexts;
- client isolation must never be claimed when the working tree is shared.

Reports that demonstrate a violation of one of these boundaries are treated as
security issues even if the individual command appears to work as documented.
