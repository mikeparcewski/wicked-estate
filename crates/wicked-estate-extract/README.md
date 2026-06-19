# wicked-estate-extract

Tree-sitter extraction layer: turns source files into `Extraction` values (nodes, local edges, unresolved refs) for 73+ languages without any core change per new language.

## What it does

- Runs tree-sitter grammars against source files and emits symbols, calls, imports, and intra-file edges via `.scm` query files — one file per language, no compiled-in `match lang { … }` arms.
- Maintains a `languages.toml` manifest; adding a language is one manifest row plus a `<name>.scm` query, no Rust change required. The capability matrix is generated from that manifest, not hand-maintained.
- Provides grammar-less extractors for JCL, HLASM, RACF, IMS, IBM MQ, and CICS/SQL.
- Provides a `CloudCollector` trait with real AWS/Azure/GCP implementations (feature-gated) and a `MockCloudCollector` for testing.
- Includes vendored in-house grammars for languages with no crates.io grammar: RPG IV, Progress ABL, VB6, VBA, VBScript, LotusScript, Informix 4GL, Visual FoxPro, PowerBuilder PowerScript, Crystal Reports formulas, CFML.

## Key types / traits

| Item | Description |
|---|---|
| `TreeSitterExtractor` | Main `Extractor` impl; dispatches to the correct grammar + `.scm` query by file extension. |
| `IaCExtractor` | `Extractor` for HCL/Terraform, CloudFormation, ARM, Bicep, Kubernetes YAML, and Pulumi. |
| `ExtractTier` | Depth a language reaches: `Document`, `Detected`, `Tags`, `Structural`, `Precise`. |
| `ExtractCap` | A capture family a language provides: `Symbols`, `Calls`, `Imports`, `Extends`, `Implements`, `Framework`. |
| `LanguageSpec` | One row from `languages.toml`: name, extensions, grammar crate, tier, caps. |
| `CloudCollector` | Trait for read-only cloud-resource enumeration (observe-only, no secret storage). |
| `TfstateCollector` | Parses `terraform.tfstate` files into resource nodes + `depends_on` edges. |
| `JclExtractor` / `HlasmExtractor` / `RacfExtractor` | Grammar-less extractors for IBM mainframe artifacts. |

## Usage

```rust
use wicked_estate_extract::{TreeSitterExtractor, by_extension};
use wicked_estate_core::{Extractor, SourceFile};

let lang = by_extension("rs").expect("rust registered");
let extractor = TreeSitterExtractor::new();
let source = SourceFile::new("src/lib.rs", source_bytes);
let extraction = extractor.extract(&source)?;
// extraction.nodes, .local_edges, .refs → ready for the resolver
```

## Crate features

| Feature | Effect |
|---|---|
| `cloud-aws` | Enables the real `AwsCloudCollector` (aws-config, Resource Explorer v2, EC2, IAM). |
| `cloud-azure` | Enables the real `AzureCloudCollector` (azure_identity, azure_mgmt_resources). |
| `cloud-gcp` | Enables the real `GcpCloudCollector` (google-cloud-asset-v1, google-cloud-auth). |
| `cloud-all` | Convenience alias enabling all three cloud collectors. |

All cloud features are off by default; the default build has no async runtime dependency.

Part of **[wicked-estate](https://github.com/mikeparcewski/wicked-estate)** — a code + infrastructure
estate graph for LLM agents (definitions, who-calls-X, blast-radius, scoped context). Local-first,
tree-sitter + SQLite, single static binary. See the umbrella [`wicked-estate`](https://crates.io/crates/wicked-estate)
crate to use the whole thing.

MIT licensed.
