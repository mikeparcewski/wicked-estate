use anyhow::bail;
use std::path::{Path, PathBuf};

pub struct ScipResult {
    pub lang: &'static str,
    pub path: PathBuf,
}

struct Indexer {
    lang: &'static str,
    markers: &'static [&'static str],
    // Whether to glob for *.csproj / *.sln instead of exact name match.
    glob_marker: bool,
    argv: &'static [&'static str],
    hint: &'static str,
}

static INDEXERS: &[Indexer] = &[
    Indexer {
        lang: "typescript",
        markers: &["tsconfig.json", "package.json"],
        glob_marker: false,
        argv: &[
            "npx",
            "@sourcegraph/scip-typescript@0.4.0",
            "index",
            "--output",
            ".scip/typescript.scip",
        ],
        hint: "ensure Node.js is installed; indexer is run via npx",
    },
    Indexer {
        lang: "rust",
        markers: &["Cargo.toml"],
        glob_marker: false,
        argv: &["rust-analyzer", "scip", ".", "--output", ".scip/rust.scip"],
        hint: "install rust-analyzer: https://rust-analyzer.github.io",
    },
    Indexer {
        lang: "go",
        markers: &["go.mod"],
        glob_marker: false,
        argv: &["scip-go", "--output=.scip/go.scip"],
        hint: "install scip-go: go install github.com/sourcegraph/scip-go/cmd/scip-go@latest",
    },
    Indexer {
        lang: "python",
        markers: &["pyproject.toml", "setup.py", "setup.cfg", "requirements.txt"],
        glob_marker: false,
        argv: &["scip-python", "index", ".", "--output", ".scip/python.scip"],
        hint: "install scip-python: pip install scip-python",
    },
    Indexer {
        lang: "java",
        markers: &["pom.xml", "build.gradle", "build.gradle.kts"],
        glob_marker: false,
        argv: &["scip-java", "index", "--output", ".scip/java.scip"],
        hint: "install scip-java: https://github.com/sourcegraph/scip-java",
    },
    Indexer {
        lang: "ruby",
        markers: &["Gemfile"],
        glob_marker: false,
        argv: &["scip-ruby", "--output=.scip/ruby.scip"],
        hint: "install scip-ruby: gem install scip-ruby",
    },
    Indexer {
        lang: "scala",
        markers: &["build.sbt"],
        glob_marker: false,
        argv: &[
            "cs",
            "launch",
            "sourcegraph:scip-scala",
            "--",
            "--output=.scip/scala.scip",
        ],
        hint: "install coursier (cs) and the scip-scala launcher",
    },
    Indexer {
        lang: "csharp",
        // markers is unused when glob_marker is true; kept empty for symmetry.
        markers: &[],
        glob_marker: true,
        argv: &["dotnet", "scip", "--output", ".scip/csharp.scip"],
        hint: "install dotnet-scip: dotnet tool install -g Sourcegraph.Scip.Dotnet",
    },
    Indexer {
        lang: "cpp",
        markers: &["compile_commands.json"],
        glob_marker: false,
        argv: &[
            "scip-clang",
            "index",
            "--compdb=compile_commands.json",
            "--output=.scip/cpp.scip",
        ],
        hint: "install scip-clang: https://github.com/sourcegraph/scip-clang",
    },
];

fn detected(indexer: &Indexer, root: &Path) -> bool {
    if indexer.glob_marker {
        // Non-recursive glob: check for *.csproj or *.sln directly in root.
        let Ok(entries) = std::fs::read_dir(root) else {
            return false;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let s = name.to_string_lossy();
            if s.ends_with(".csproj") || s.ends_with(".sln") {
                return true;
            }
        }
        false
    } else {
        indexer.markers.iter().any(|m| root.join(m).exists())
    }
}

pub fn auto_scip(root: &Path) -> anyhow::Result<Vec<ScipResult>> {
    let scip_dir = root.join(".scip");
    if let Err(e) = std::fs::create_dir_all(&scip_dir) {
        eprintln!("notice: could not create {}: {e}", scip_dir.display());
        return Ok(Vec::new());
    }

    let mut results = Vec::new();
    let mut failed: Vec<String> = Vec::new();

    for indexer in INDEXERS {
        if !detected(indexer, root) {
            continue;
        }

        let (program, args) = match indexer.argv.split_first() {
            Some(pair) => pair,
            None => continue,
        };

        eprintln!(
            "notice: detected {} — running: {}",
            indexer.lang,
            indexer.argv.join(" ")
        );

        let outcome = std::process::Command::new(program)
            .args(args)
            .current_dir(root)
            .status();

        match outcome {
            Ok(status) if status.success() => {
                let path = root.join(".scip").join(format!("{}.scip", indexer.lang));
                results.push(ScipResult { lang: indexer.lang, path });
            }
            Ok(status) => {
                // Indexer was found and ran but exited non-zero — this is a real failure,
                // not a "not installed" case. Collect and propagate so the caller can exit non-zero.
                eprintln!(
                    "error: {} SCIP indexer exited with {} — {}",
                    indexer.lang, status, indexer.hint
                );
                failed.push(format!("{} (exit {})", indexer.lang, status));
            }
            Err(_) => {
                // Indexer binary not present — advisory notice only, not an error.
                eprintln!(
                    "notice: {} SCIP indexer not found — {}",
                    indexer.lang, indexer.hint
                );
            }
        }
    }

    results.retain(|r| r.path.exists());

    if !failed.is_empty() {
        bail!("SCIP indexer(s) failed: {}", failed.join(", "));
    }
    Ok(results)
}
