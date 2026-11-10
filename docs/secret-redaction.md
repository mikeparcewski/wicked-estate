# Secret redaction

`wicked-estate index` stores the text of the files it indexes, so that retrieval can return real
code, and it builds node signatures and a full-text index from that text. Before 0.33.0, a secret
value in a committed file (a compose `environment` password, a Helm `values.yaml` password, a
Kubernetes `Secret`, a string literal in code) was stored in all three places
([estate#405](https://github.com/mikeparcewski/wicked-estate/issues/405)). From 0.33.0 it is not.

## What is stored

**The index stores keys, references, and values from structurally trusted fields only.** Every
file's text is redacted *before* it is parsed, so the stored text, node signatures, the full-text
index and unresolved-reference names all come from the redacted text. Masking keeps the same byte
length and line breaks (`password: REDACTEDREDA`), so spans and line numbers stay valid.

| What | Rule |
|---|---|
| Config formats: JSON, YAML, TOML, HCL/`.tf`/`.tfvars`, INI, Dockerfile | **Default-deny.** A value is kept only when it is a boolean or null, a reference (`${VAR}`, `{{ .Values.x }}`, `secretKeyRef`), or a trusted field of a known schema (compose `image`/`ports`/service names; Kubernetes names, labels, selectors, ports, images, Ingress rules; `package.json`, `package-lock.json`, `tsconfig.json`, `Cargo.toml` and `pyproject.toml` fields the cross-repo linker reads). Every other value is masked. |
| Env-style values (compose `environment`, k8s `env`, ConfigMap `data`, Dockerfile `ENV`/`ARG`) | Masked unless the key ends in `_URL`, `_URI`, `_ENDPOINT`, `_BASE_PATH`, `_HOST`, `_ADDR`, `_ADDRESS`, `_SERVICE` or `_PORT`. Then a URL keeps its scheme and port. Its host is kept when it is a service named in the same file, an IP address or a registrable domain (left-most labels beyond the registrable domain are masked unless known). Each path segment is kept only when it is a base-path word (`api`, `v1`, `graphql`, ...). Userinfo, query and fragment are always masked. Topic and queue values are masked. |
| A key that names a secret (`password`, `token`, `apiKey`, `client_secret`, ...) | Its value is masked, even in a trusted field. |
| Kubernetes `Secret` `data`/`stringData`, `SealedSecret` `encryptedData`, `secrets*.yaml` | Every value masked; keys kept. |
| Code (every language) | A string literal is masked when it has a known token shape or a long random-looking run, or when it is bound to a secret-named variable, property, parameter or env lookup (`const password = "..."`, `{ apiKey: "..." }`, `process.env.TOKEN \|\| "..."`). Module specifiers are never masked. |
| A file type with no redaction rule (grammar-less mainframe formats such as JCL, runtime plugin grammars, files only a drop-in rule reads) | Token shapes (below) plus the value of every `name=value` / `name: value` whose name names a secret (`PASSWORD=...`). Not default-deny. |
| Every file, including comments and prose | Known token shapes (AWS, GitHub, GitLab, Slack, Stripe, Google, OpenAI/Anthropic-style keys, npm tokens, JWTs, private-key blocks) and URL credentials (`scheme://user:pass@`) are masked. |
| Credential files | Never indexed, even when committed: `.env`, `.env.*` (except `.env.example`, `.env.sample`, `.env.template`), `*.env`, `.npmrc`, `.pypirc`, `.netrc`, `*.pem`, `*.key`, `id_*`, `*.p12`, `*.pfx`, `*.keystore`, `*.jks`, `kubeconfig*`, `credentials*`. |

Every store write also passes a backstop that masks known token shapes and URL credentials in
every free-text column (node signatures, docs and metadata, edge and support metadata,
unresolved-reference names, annotations, semantic notes, telemetry imports, `tfstate` ids, meta
values). This covers text that never passes the extractor.

Reads are confined to the repository root: a symlink, `..` path or absolute path that resolves
outside it is never read (drop-in rule files, semantic-evidence documents and the source walk).

The rules are data: `crates/wicked-estate-extract/secret-rules.toml`.

## Existing stores

A store written before 0.33.0 still holds the old text. It is cleaned:

- **automatically** by the first write command that opens it (`index`, `watch`, `scip`,
  `evidence`, `tfstate`, `import-telemetry`, `annotate`, `semantics`, `compact`), before that
  command writes anything; or
- **explicitly**, now: `wicked-estate redact-store --db <path>`.

A store counts as a code graph when it records an indexed root or holds any indexed file, so a
graph written before `indexed_root` existed is cleaned too. Memory and knowledge stores hold
neither and are left alone.

The migration re-redacts every stored file text (current and archived versions), re-keys and
deletes the old blobs, re-slices node signatures from the masked text, scrubs every other
free-text column, drops the query cache, rebuilds and optimizes the full-text index, marks every
file for re-extraction, and records the policy version. It then checkpoints the WAL with TRUNCATE
and runs `VACUUM`, so freed pages that held old text do not stay in the database file. It reports
counts only. Identity strings are scrubbed the same way (a token-shaped value that became part of
a symbol id or name is masked in every copy), and a node whose file text was never stored loses
its signature and doc (the next index re-derives them). If a reader is attached, the WAL may not
truncate: the store records that its cleanup is pending, the report says so, and running
`redact-store` again when the store is idle finishes the job.

**Until a SQLite store is cleaned, estate does not serve its text.** Reads withhold stored source text,
signatures and docs and bypass the query cache. CLI read commands print a `REDACTION-PENDING:`
line naming the command that cleans the store.

Backups and copies of a database file made before the migration are outside estate's reach.
Delete them, or run `redact-store` on each one.

## What is not covered (the residuals)

These are stated in every answer that returns stored source text (a `PRIVACY:` diagnostic, or the
`privacy` field of `source --json`):

1. **A secret under an innocent name in code.** `const t = "..."` with a short or low-entropy
   value is not detected. For source text the guarantee is "no *detected* secret", not "no
   secret". Not storing code text at all is the only full fix.
2. **A secret placed inside a hostname or repository owner.** Registrable domains, known service
   hostnames and repository identities (`host/owner/repo`) are kept as public identifiers. A
   credential deliberately shaped as one is indistinguishable from an address unless it has a
   known token shape.
3. **The length of a masked value is visible**, because masking keeps the same length.
4. **Memory and knowledge stores** (`memory.db`, `knowledge.db`) are not redacted yet. A pasted
   credential in a captured memory or an ingested document is stored as written
   ([estate#406](https://github.com/mikeparcewski/wicked-estate/issues/406)).
5. **Non-SQLite backends.** Postgres is a source-build feature (`--features postgres`; the
   released binaries and `cargo install wicked-estate` do not include it). It gets the write
   backstop and redacted text from the indexer, but has no migration and no read guard: a
   Postgres store written before 0.33.0 must be re-created to be clean, and `redact-store` on one
   fails with that message instead of reporting it clean.
6. **File types with no redaction rule** get token shapes and secret-named `name=value` values
   masked, not default-deny (see the table above).
7. **Drop-in extra-edge rules** (`.wicked-estate-extractors/*.toml`) read their target files as
   committed: a rule is a user-declared identity extractor, so what it captures is stored as
   node names and ids (known token shapes still masked). Do not point a rule at a value you would
   not want stored.
