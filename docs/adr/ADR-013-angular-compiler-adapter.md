# ADR-013: Angular compiler adapter (TS-S3)

- **Status:** Accepted (go, with conditions)
- **Date:** 2026-10-10
- **Issue:** #277 (TS-S3). It builds on #275 (TS-S2C, the semantic-evidence envelope) and #215 (field reads join the class field slot).

## Context
Angular template bindings (`[user]="current"`) wire values between components through the template compiler, which tree-sitter never sees. The TypeScript extractor matches `@Input()` by shape only (`tree-sitter-convention`, Heuristic). That proves neither `@angular/core` identity nor which member a binding reaches: aliases, inheritance and signal inputs all break shape matching.

## Decision
A **companion adapter**, `adapters/angular-evidence/` (Node ESM), drives the Angular compiler and emits `SemanticEvidence` v1 with `input_binding` facts. The Rust engine projects them (ENGINE-CONTRACT §3.5). The adapter never ships inside the Rust binary.

**Validated against** `@angular/compiler-cli` / `@angular/compiler` / `@angular/core` **22.2.2** with TypeScript 6.0.3. All are pinned exactly, with a committed lockfile.

**APIs used:**
- `NgtscProgram` with `readConfiguration` and `loadNgStructureAsync`;
- `NgCompiler.getTemplateTypeChecker()`, then `getTemplate`, `getSymbolOfNode`, `getTsSymbolOfSymbol` and `getExpressionTarget`;
- the `@angular/compiler` template AST (`TmplAstRecursiveVisitor`, `BindingType`, expression AST classes).

All are exported from the package roots and used by the Angular language service. None is part of Angular's documented public API. The type checker also needs the private compiler option `_enableTemplateTypeChecker`.

**What a probe of the compiler proved** (the fixtures pin each case):

| Case | Evidence from the compiler |
|---|---|
| alias | `[aliasIn]` resolves to member `renamed` |
| inheritance | inherited inputs resolve to the declaring abstract base |
| signal and model inputs | resolved |
| two-way | the `[(x)]` input half is `BindingType.TwoWay` |
| control flow | `@if`, `@for` and `ng-template` bindings resolve |
| duplicate names | two classes named `Child` stay apart |
| templates | inline and `templateUrl` templates both resolve; spans are UTF-16 offsets into the template's own file |
| DOM vs input | a DOM property is `DomBinding`, never an input |
| DOM event vs output | a DOM event targets `Element`, an output targets the directive member (input to TS-S4) |
| library without source | an input declared in a `.d.ts` is counted external, never projected |

## Conditions (each enforced)
1. **Version gate.** The adapter refuses any Angular major other than 22, for both the compiler and the project's `@angular/core`. A malformed version is refused too. Refusal is exit 2 with no envelope.
2. **Compiler-derived fixtures.** CI's `angular-adapter` job reinstalls the pinned compiler and re-derives every committed `evidence.json` (`npm run fixtures:check`), so the fixtures can never be hand-edited.
3. **Nothing invented.** Template locals (`let-x`, `@for` items, `#ref`), pipes, calls and non-member reads are listed as `unresolved` and counted by the engine. Shim (`.ngtypecheck.ts`) locations are never emitted.
4. **Revalidate per major.** Moving to Angular 23 means rerunning the probe cases and regenerating the fixtures. Bumping the pin alone is not enough.

## Consequences
- `flow_evidence: compiler` is now emitted, and only through §3.5 ingest. Query files still cannot claim it.
- The TypeScript base plane gives the properties and get/set accessors of decorated classes their `{class}:field:{name}` slot, and `abstract class` declarations become `Class` nodes. A decorator's spelling is not matched (`@Cmp` and `@ng.Component` are Angular too, and a query cannot resolve imports), so every decorated class is included. This is a visible node delta, bounded to decorated classes; undecorated classes are unchanged.
- Out of scope here: outputs and events (TS-S4), host bindings, multi-member property paths, RxJS and taint.
