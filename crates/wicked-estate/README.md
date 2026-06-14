# wicked-estate

Code + infrastructure **estate graph for LLM agents** — definitions, who-calls-X, blast-radius,
scoped context — across 91 wired languages plus a mainframe/IaC estate layer (COBOL, JCL, RACF,
IMS, MQ, Terraform, CloudFormation, …). Local-first, tree-sitter + SQLite, single static binary.

```sh
cargo install wicked-estate
wicked-estate index ./my-project --db graph.db
wicked-estate blast-radius MyType --db graph.db
```

Full docs, features, design notes, and source:
**https://github.com/mikeparcewski/wicked-estate**

Optional semantic search (off by default to keep the build light):
`cargo install wicked-estate --features model2vec` (static) or `--features fastembed` (ONNX/BGE).

MIT licensed.
