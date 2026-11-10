# X1 package-link oracle (PLAN §6.3, §6.4)

`x1-labels.json` has one row for each link the linker predicted on the corpus below. Each row was labelled by `label.py` from the package managers' own resolution, never from the linker's reading of the manifests:
- npm uses Node's `require.resolve` plus `realpath`, from the importing file;
- cargo uses `cargo metadata --all-features`, with `[patch]` applied.

`score.py x1-labels.json` reproduces the verdict.

| Case | Repo | Commit | How it was indexed |
|---|---|---|---|
| npm-cli | npm/cli | be1b7bc870d8 | one label; workspaces linked through `package-lock.json`, installed with `npm ci --ignore-scripts` for the oracle |
| tokio | tokio-rs/tokio | c1489e0ef85f | one label; cargo workspace with `[patch.crates-io]` path patches |
| c6 | mikeparcewski/wicked-crew + wicked-studio | 855ee632b45f / f08065fa37f9 | two labels; studio installed with `npm ci` (registry-source to `crew-api-types`), plus crew's own workspace links |
| A3 | mikeparcewski/wicked-e2e-library-booking | 81b84ab7bdfc | alone, and as a third label with c6: no link to or from it |

The run used estate#413 at 20a399a plus the cargo-patch fold, as a debug build, with node v26.11.1 and cargo 1.99.0.

Result:
- package-import links: n = 421, all 421 correct;
- Wilson 95% lower bound 0.9910, against a floor of 0.97 with n >= 50: **ships**;
- the 293 links from the two public repositories alone are also all correct;
- by identity: path 383/383, registry-source 38/38 (n < 50, not judged on its own).

Not yet run: the weak-oracle mutant (packages linked by name alone), and recall. The "possible" items per case are in each case's `link --json` `possible` list.
