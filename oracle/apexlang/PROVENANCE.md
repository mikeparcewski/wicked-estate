# APEXlang corpus pin and reference-precision oracle (estate#310, #313)

## Corpus

`corpus.tsv` lists every file of the pinned corpus with its sha256 and size (3,198 rows: 3,160
`.apx`, 36 `.apex/apexlang.json`, 2 licences). `apps.tsv` gives one row per application with
its `mmdVersion`: 26.2.0+3479 for the 33 oracle/apex applications, and 26.1.0+3102 for the 3
oracle/skills examples.

| Repo | Commit | What | License |
|---|---|---|---|
| oracle/apex (branch 26.2) | fbe095e4c6e783b3fa930ffae927e4826e549190 | every `*.apx` (2,961) in 33 sample, starter and utility applications | UPL-1.0 |
| oracle/skills | 014cdb29f12dfa6a689bbe8a80ee1c5f0bf15be1 | `apex/apexlang/26.1/apexlang-example-applications/assets` (199 `.apx`, 3 applications) | UPL-1.0 |

`scripts/apexlang-corpus.py fetch <dir>` sparse-clones both repositories at these commits and
checks every file against `corpus.tsv`: a missing file, an extra file or a changed byte fails it.
CI's gate job runs it and then the parse and resolution gates. Nothing comes from docs.oracle.com.

## Precision oracle

`label.py` labels each resolved APEXlang reference edge from Oracle's own legacy SQL export of the
same application at the same commit (`<app>/sql/<app>.sql`; fetch with `--with-sql-exports`). In
that export a reference is a numeric component id, for example
`p_item_plug_id=>wwv_flow_imp.id(N)`, written by APEX itself. The oracle never reads the
resolver's property table. For each edge it:
1. finds the owner's SQL record: the same page, plus the same static id, item name or page id. For
   list entries and dynamic-action steps, the parent's static id must match too;
2. reads the SQL parameter that stores that property (`MAPPING` in `label.py`, written from the
   export API);
3. checks that the record this parameter points at has the edge target's kind, key and page.

An edge it cannot check is labelled `unlabelled`, with the reason, and still counted. That covers
the oracle/skills examples (no SQL export), properties with no mapped parameter (saved-report
columns, branch buttons, ...) and owners with no static id.

Run on the corpus above with this branch (debug build, 2026-11-10):

```
scripts/apexlang-corpus.py fetch <dir> --with-sql-exports
APEXLANG_CORPUS_DIR=<dir> APEXLANG_EDGES=edges.tsv \
  cargo test -p wicked-estate --test apexlang_index full_pinned_corpus_resolution
python3 oracle/apexlang/label.py <dir> edges.tsv labels.tsv
python3 oracle/apexlang/score.py labels.tsv        # -> census.txt
```

Result, a census of every resolved edge rather than a sample (`census.txt`):
- 19,967 resolved reference edges. The oracle checked 15,980 of them: **15,980 correct, 0 wrong**.
  Wilson 95% lower bound **0.9998** (floor 0.97 with n >= 200): **ships**.
- 3,987 edges were unlabelled, with reasons in the labels file. 1,660 of them are oracle/skills
  edges, which have no SQL export.
- Mutant checks that the oracle can fail:
  - wrong target: each oracle/apex edge was given a different target of the same kind in the same
    application. All 15,083 mutants the oracle could check were labelled wrong;
  - wrong parent: each list or breadcrumb entry target was moved under another list of the same
    application. All 1,090 were labelled wrong.

`labels-sample.tsv` holds 400 of the census rows, drawn with `random.seed(20261110)`:
`score.py labels-sample.tsv` gives 313 correct, 0 wrong, 87 unlabelled, and a lower bound of
0.9879.

Resolution over the whole corpus (43,322 refs; the `full_pinned_corpus_resolution` test):
- all 6,086 holder links (a top-level component -> its application or container) resolve;
- of the 37,236 `@` references:
  - 20,932 are typed and resolved;
  - 10 are typed but ambiguous (data-load definitions named in two kinds of definition);
  - 1 has an unknown property;
  - 904 are theme-relative `@/x` inside a theme, naming built-in templates the exports do not contain;
  - 15,389 are never-bind by design: 15,104 theme-relative `@/x` on pages, which do not know their
    theme, plus template subscriptions, API parameter names, plugin attribute dependencies, master
    columns and REST operations.
