# Secret refs — `central.security/v1`

Capsules declare *where* a secret lives, never *what* it is. A declaration is
a `central.secret-ref/v1` string; resolution happens at projection time, on
the machine that materialises the environment.

## Grammar

| Scheme | Shape | Boundary |
|--------|-------|----------|
| `varlock://` | `varlock://<path>/<NAME>` | varlock CLI (`varlock printenv --path <path> <NAME>`) — native default; the env file may seal values or hold `keychain()` refs backed by the macOS Keychain via varlock's signed enclave |
| `pass://` | `pass://<store-path>` | pass(1) (`pass show <store-path>`) — free, gpg-backed, git-syncable |
| `keychain://` | `keychain://<service>/<account>` | OS secure store (`keyring`) — signed consumers |
| `op://` | `op://<vault>/<item>/<field>` | 1Password CLI (`op read`) — optional paid adapter |
| `env://` | `env://<NAME>` | process environment — **gated**, see below |

Capsules declare them under `[secrets]` in the capsule manifest:

```toml
[secrets]
GEMINI_API_KEY = "varlock://secrets/providers.env/GEMINI_API_KEY"
BACKUP_KEY     = "pass://providers/gemini-api-key"
```

## Resolution order

`varlock://` > `pass://` > `keychain://` > `op://` > `env://` (flagged) —
the declared preference per the 2026-09-09 owner decision (all vault
providers optional; 1Password demoted to a test-in-time adapter).

Dispatch is by the ref's declared scheme — each scheme has one resolver over
its genuine store boundary, and the core never reimplements vault access. The
order is a *preference for what to declare*, not a runtime fallback chain:
when a store is unavailable (locked daemon, unsigned-in CLI), the failure
names the remediation instead of silently trying another store.

- **`varlock://`** — the documents-side boundary and native default. The
  varlock daemon holds the device key; the resolver only ever sees the
  decrypted variable via `printenv`. A locked daemon, a missing variable and
  a missing file are three different, named errors. With `keychain()` refs
  in the env file, material sleeps in the macOS Keychain and reads go
  through varlock's signed enclave — no consumer needs Keychain access
  itself.
- **`pass://`** — the free cross-machine adapter: zx2c4's password store,
  gpg-encrypted files under `~/.password-store`, syncable through git.
  `pass show <path>` decrypts via gpg-agent; a missing entry, a missing
  receiving key and an uninitialized store are three different, named
  errors.
- **`keychain://`** — the OS secure store; the right home for machine-local
  credentials. On unsigned hosts, user-presence access control degrades to
  a precise capability error rather than a prompt (the writer/reader
  code-signature family law).
- **`op://`** — optional paid adapter, test in time. Service-account auth
  (`OP_SERVICE_ACCOUNT_TOKEN`) belongs in the keychain entry
  `keychain://workcell/op-service-account`.
- **`env://`** — legacy escape hatch, mirroring the `--from-env` law: presence
  of a matching variable in the environment never makes import eligible by
  itself. Admissible only when the operator explicitly opens the gate
  (`SuiteSecretResolver::with_env_import`); the default suite keeps it closed.

## Laws

1. **Refs are location only.** No type in `aikit-core::secret_ref` can hold
   material — a ref cannot leak a value by construction.
2. **Plan identity is the ref.** `ProjectionItem::SecretEnv` carries name +
   ref; the value is resolved at materialisation and lands only in the
   generation env manifest — which is excluded from the generation hash, so
   rotation never churns identity and the hash stays an oracle-free
   fingerprint.
3. **Refuse without a resolver.** A plan that declares a secret with no
   resolver registered is refused (`generation.secret_resolver_missing`),
   never silently built without it.
4. **Detection stays fingerprint-only.** Scanners emit env names and SHA-256
   fingerprints, never values — a detector that can emit a value is itself a
   leak source.

## Provider credential lifecycle

Model-route credentials use the same grammar. A provider key is bound
owner-natively and carries only safe state (`aikit.credential-bindings/v1`):
the provider, the declared ref when one exists, and lifecycle timestamps.

```text
aikit credential discover                    # candidate keys on this machine, presence only
aikit credential setup <ref> --ref op://…    # declare an external location (no material read)
aikit credential setup <ref>                 # bind material into the OS secure store
aikit credential rotate <ref> --ref op://…   # new material or location, same credential ref
aikit credential revoke <ref>                # refuse at next use; operator stores stay put
aikit credential verify <ref>                # one operator-invoked live check of the key
```

Laws this surface keeps:

* A declared ref (`--ref`) is a location. AIKit never reads or stores the
  material behind it; a resolver (`op`, `varlock`, `pass`, the OS keychain)
  materialises at the one moment of use, and `env://` is refused — import
  explicitly with `--from-env --env-var` instead.
* Discovery findings are presence-only: variable names and locations, never
  values. Harness auth files contribute key names only.
* Rotation changes the material or its location while the credential ref and
  first-bound timestamp stay stable; the routing join and the settings
  inventory read those facts, so availability follows the binding, and a
  revoked binding makes the route unusable at the next check.
* The settings disclosure (`aikit system --json`, section `models`) renders
  the inventory as presence, refs and timestamps — there is no field a
  secret value could occupy.
* `verify` is operator-invoked only — no launch, resolution or detection
  path ever checks a key against its provider. One minimal read (usually the
  provider's models endpoint) yields working / refused / unreachable plus
  the HTTP status class, and the outcome records `last_verified_at` on the
  binding only when it is definitive: the key worked, or the provider
  refused it outright (401/403). A provider with no known check is refused
  honestly; the key and the Authorization header are never printed, logged
  or persisted.

## Harness key delivery

Each harness profile (`aikit.harness-profile/v1`, models layer,
`key-delivery`) declares, per provider the harness can serve, the env var
its native launch reads for the key — or the own-login fact that it
authenticates through a store of its own, and the reason no env-var path is
declared where that is the truth (zcode, opencode, openclaw, cursor-cli,
ollama). Declared facts as of 2026-09-19:

| Harness | Provider | Env var | Effect when the credential is bound |
|---------|----------|---------|--------------------------------------|
| claude-code | `provider:anthropic` | `ANTHROPIC_API_KEY` | injected into the scrubbed launch environment |
| codex | `provider:openai` | `OPENAI_API_KEY` | injected into the scrubbed launch environment |
| gemini | `provider:gemini` | `GEMINI_API_KEY` | injected into the scrubbed launch environment |
| kimi | `provider:moonshot` | `MOONSHOT_API_KEY` | injected; unbound launches refuse with the bind remediation |
| qwen-code | `provider:dashscope` | `DASHSCOPE_API_KEY` | injected; unbound launches refuse with the bind remediation |
| pi | — | — | own auth store (`~/.pi/agent/auth.json`); selected-model policy delivers its credential explicitly |
| zcode | — | — | own managed login; no env-var key path declared |
| opencode | — | — | own per-provider store (`opencode auth login`); no fixed env-var path |
| openclaw | — | — | auth profiles in its own config; no env-var key path |
| cursor-cli | — | — | own subscription login; no env-var key path |
| ollama | — | — | local serving reads no provider key |

At launch, a bound credential materialises through the same seam the
selected-model path uses (OS store, explicit `--from-env` import, or a
declared ref through the resolver suite) and is injected under the declared
variable into the scrubbed final-child environment — never an empty or
ambient value. Where the profile records an own-login fact for the provider,
an unbound key is an honest absence and the harness's native login stands;
the Codex model-selected `provider:openai` path additionally requires the
selected Codex executable to report a ChatGPT login, then launches under a
scrubbed environment without `OPENAI_API_KEY` delivery. `CODEX_HOME`, when
set, is retained so that the probe and child use the same native login store;
where it does not, an unbound key refuses the launch with the bind
remediation instead of starting a body that cannot authenticate. A revoked
or expired binding refuses either way.
