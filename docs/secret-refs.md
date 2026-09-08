# Secret refs — `central.security/v1`

Capsules declare *where* a secret lives, never *what* it is. A declaration is
a `central.secret-ref/v1` string; resolution happens at projection time, on
the machine that materialises the environment.

## Grammar

| Scheme | Shape | Boundary |
|--------|-------|----------|
| `op://` | `op://<vault>/<item>/<field>` | 1Password CLI (`op read`) |
| `keychain://` | `keychain://<service>/<account>` | OS secure store (`keyring`) |
| `varlock://` | `varlock://<path>/<NAME>` | varlock CLI (`varlock printenv --path <path> <NAME>`) |
| `env://` | `env://<NAME>` | process environment — **gated**, see below |

Capsules declare them under `[secrets]` in the capsule manifest:

```toml
[secrets]
GEMINI_API_KEY = "op://Central/providers/gemini"
DATABASE_URL   = "keychain://glade/production-database-url"
```

## Resolution order

`op://` > `keychain://` > `varlock://` > `env://` (flagged).

Dispatch is by the ref's declared scheme — each scheme has one resolver over
its genuine store boundary, and the core never reimplements vault access. The
order is a *preference for what to declare*, not a runtime fallback chain:
when a store is unavailable (locked daemon, unsigned-in CLI), the failure
names the remediation instead of silently trying another store.

- **`op://`** — canonical vault. Service-account auth (`OP_SERVICE_ACCOUNT_TOKEN`)
  belongs in the keychain entry `keychain://workcell/op-service-account`.
- **`keychain://`** — the OS secure store; the right home for machine-local
  credentials and for the op service-account token itself. On unsigned hosts,
  user-presence access control degrades to a precise capability error rather
  than a prompt.
- **`varlock://`** — the documents-side boundary: a varlock-sealed env file.
  The varlock daemon holds the device key; the resolver only ever sees the
  decrypted variable via `printenv`. A locked daemon, a missing variable and
  a missing file are three different, named errors.
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
