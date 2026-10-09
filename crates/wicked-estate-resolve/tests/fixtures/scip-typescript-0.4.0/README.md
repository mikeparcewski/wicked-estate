# scip-typescript 0.4.0 sample (TS-S2C)

`index.scip` is real indexer output:

```
npx @sourcegraph/scip-typescript@0.4.0 index --output index.scip
```

run in this directory (`src/util.ts`, `src/main.ts`, `tsconfig.json`, `package.json`). The only
edit is `metadata.project_root`, which is rewritten to `file:///workspace/scip-ts-sample` so that
no machine path is committed; every document and occurrence is the indexer's own.

What it pins: SCIP has no call role, so `const f = helper;` (main.ts 5:12) and `helper(LIMIT)`
(main.ts 6:9) are identical role-less occurrences of `helper().`. Both must stay references.
