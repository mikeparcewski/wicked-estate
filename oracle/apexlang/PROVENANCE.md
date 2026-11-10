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
- 19,971 resolved reference edges. The oracle checked 15,020 of them: **15,020 correct, 0 wrong**.
  Wilson 95% lower bound **0.9997** (floor 0.97 with n >= 200): **ships**.
- 4,951 edges were unlabelled, with reasons in the labels file. 1,660 of them are oracle/skills
  edges, which have no SQL export.
- Mutant check that the oracle can fail: each oracle/apex edge was given a different target of the
  same kind in the same application. Of the mutants the oracle could check, it labelled 14,125
  wrong and 7 correct. Those 7 are same-key coincidences. So the oracle discriminates.

`labels-sample.tsv` holds 400 of the census rows, drawn with `random.seed(20261110)`:
`score.py labels-sample.tsv` gives 299 correct, 0 wrong, 101 unlabelled, and a lower bound of
0.9873.

Resolution over the whole corpus (37,236 references; the `full_pinned_corpus_resolution` test):
20,936 typed and resolved; 40 typed but ambiguous (`masterColumn @ID` with several `ID` columns on
a page, `operation @put` across REST sources, legacy data-load definitions); 251 never-bind by the
table; 1 with an unknown property; 16,008 theme-relative `@/x` references to built-in theme
templates, which the exports do not contain.
