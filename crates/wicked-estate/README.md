# wicked-estate

Code + infrastructure **estate graph for LLM agents** — definitions, who-calls-X, blast-radius,
scoped context — across 105 wired languages plus a mainframe/IaC estate layer (COBOL, JCL, RACF,
IMS, MQ, Terraform, CloudFormation, …). That includes hard-to-find **legacy enterprise stacks** —
VB6/VBA/VBScript/VB.NET, RPG, Delphi, ColdFusion (CFML), Progress OpenEdge ABL, PowerBuilder,
Visual FoxPro, LotusScript, Informix 4GL, and Crystal Reports formulas — several with tree-sitter
grammars authored in-house where none existed. Local-first, tree-sitter + SQLite, single static binary.

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
