//! wicked-estate#583: when the COBOL grammar reads almost none of a file, scanner patch 4 lets the
//! parse end in one error, in linear time. The index must say so per file (`PARTIAL-PARSE`, with
//! the line), never truncate the graph silently (#578's failure).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_cobol_partial_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("src")).unwrap();
    d
}

#[test]
fn a_cobol_file_the_grammar_cannot_read_is_reported_with_its_line() {
    let dir = scratch("junk");
    let junk: String = (0..10_000)
        .map(|i| format!("let value_{i} = compute(&inputs[{i}], |x| x + {i})?;\n"))
        .collect();
    fs::write(dir.join("src/JUNK.cbl"), junk).unwrap();
    fs::write(
        dir.join("src/GOOD.cbl"),
        "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. GOOD.\n       PROCEDURE DIVISION.\n       MAIN-PARA.\n           GOBACK.\n",
    )
    .unwrap();
    let db = dir.join("g.db");
    let out = Command::new(env!("CARGO_BIN_EXE_wicked-estate"))
        .current_dir(&dir)
        .env("WICKED_HOME", dir.join("home"))
        .arg("index")
        .arg(dir.join("src"))
        .arg("--db")
        .arg(&db)
        .output()
        .expect("spawn wicked-estate");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stderr={err}");
    let lines: Vec<&str> = err
        .lines()
        .filter(|l| l.starts_with("PARTIAL-PARSE:"))
        .collect();
    assert_eq!(lines.len(), 1, "stderr={err}");
    assert!(
        lines[0].contains("JUNK.cbl") && lines[0].contains("from line "),
        "{}",
        lines[0]
    );
    let _ = fs::remove_dir_all(&dir);
}
