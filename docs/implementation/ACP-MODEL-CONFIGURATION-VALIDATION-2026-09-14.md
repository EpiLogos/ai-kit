# ACP native model-configuration validation

This receipt records an observed owner validation on 2026-09-14. It does not
claim independent provider billing, durable model-policy selection, Agency
authority, or availability outside the reported native session.

The public owner route was invoked through `aikit-session-space encounter` for
one already resident Codex ACP AgentSession. The owner was upgraded through its
expected-PID shutdown route and exact native reconnect; it did not issue a
fresh open. The canonical AgentSession and recorded native session identifier
were unchanged across the upgrade.

The provider reported these two bounded select controls:

- `model`, including `gpt-5.6-luna`.
- `reasoning_effort`, including `low`.

A public `model-select` request naming both reported values produced a positive
provider readback of Luna and low. The response reported `selected: true` and
`inference_observed: false`; therefore configuration itself is not presented as
an inference receipt. A single later addressed prompt, `Reply exactly:
OI_LUNA_LOW_OK. Do not use tools or edit files.`, completed with that reply and
an ordinary resident turn-completion record carrying the same provider-reported
Luna/low configuration.

Negative controls used the same resident binding and sent no prompts:

- An unadvertised model id was refused as
  `connection.acp.model_not_advertised`.
- An unadvertised effort id was refused as
  `connection.acp.reasoning_effort_not_advertised`.
- A following model readback still reported Luna/low and the same canonical and
  native identifiers.

The reconnect generated raw `provider-history-replay` receipts. Ordinary live
provider projections counted 31 before and 31 after the negative controls;
raw history-replay receipts counted 10 before and 10 after. That shows the
negative controls did not add projected live blocks.

A later owner repair can exclude a historic projection only when its own journal
proves one precise shutdown → same native session → one provider replay →
native-load binding sequence with no intervening owner write. The raw event and
stored block remain intact, and the derived causal receipt is exposed in the
owner view. Ambiguous or unmatched history remains visible; this is not
text-based deduplication.

The complete machine-local command outputs are retained at
`/tmp/oi-encounter-model-live-20260914/`. They deliberately remain outside this
repository because they contain transcript and provider payload material.

## Harness admission observed

The public owner listed two installed live-walk providers: Codex ACP and Pi.
The Codex ACP resident described above is the only harness observed with an
admitted, provider-reported cheap model and execution-budget control, and is
therefore the only harness demonstrated for the bounded cheap prompt. The Pi
executable is present, but no active Pi resident, credential availability, or
provider-advertised cheap selection was observed. No credential values were
read or copied. A future Pi test requires its own public admission and native
model readback before inference.
