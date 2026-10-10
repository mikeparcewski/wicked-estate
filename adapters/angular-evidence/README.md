# wicked-estate Angular evidence adapter (TS-S3)

This adapter drives the Angular compiler and writes the compiler-resolved template **input bindings** as one `SemanticEvidence` v1 document. wicked-estate then ingests that document into its support plane. The contract is in `docs/ENGINE-CONTRACT.md` §3.5, and the decision record is `docs/adr/ADR-013-angular-compiler-adapter.md`.

```
npm ci
node extract.mjs --project path/to/tsconfig.json --root path/to/repo [--snapshot <id>] > evidence.json
```

- It is validated against **Angular 22.2.2** only, pinned exactly. Any other Angular major, or a malformed version, exits **2** and writes no envelope.
- Document paths are relative to `--root`, which should be the same root you `wicked-estate index`.
- Columns are UTF-16, the compiler's unit; the engine converts them to UTF-8 bytes against the source.
- Reads the compiler could not tie to a class member are listed under `unresolved`, never guessed. These are template locals, pipes, calls, and inputs from libraries without source.
- `fixtures/` holds real Angular projects together with the envelopes this adapter generated from them. CI runs `npm run fixtures:check` to prove those envelopes are the compiler's output.

This adapter does **not** handle outputs or template events (TS-S4), host bindings, or anything beyond input bindings. It is not taint analysis.
