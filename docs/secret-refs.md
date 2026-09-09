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
