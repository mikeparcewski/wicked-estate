//! #322 rule 7: the COBOL external scanner gets the same fuzz smoke as the vendored grammars.
//!
//! Its parse tables come from the arborium-cobol crate, so `scripts/fuzz-smoke-vendored-parsers.py`
//! (which builds each grammar from its `grammar.json`) cannot load it. This test feeds the real
//! extraction route a fixed, seeded set of inputs instead: every seed cut off at each of the first
//! columns of every line (the file ends in the sequence area, the indicator or a token), repeated
//! openers, control bytes, a long line, and mutations. Every input must return. Extraction runs on
//! a worker thread, so a scanner stuck in a loop fails the test, naming the input, instead of
//! hanging the run; the stuck thread ends with the test process.

use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::Duration;
use wicked_estate_core::{Extractor, Language, SourceFile};
use wicked_estate_extract::CicsSqlExtractor;

/// A fixed-format program with the constructs the scanner handles itself: sequence numbers,
/// comment and continuation indicators, an IDENTIFICATION DIVISION comment entry, string
/// literals, EXEC blocks and text past column 72.
const PROGRAM: &str = "\
000100 IDENTIFICATION DIVISION.
000200 PROGRAM-ID. FUZZSEED.
000300 AUTHOR. A FREE-TEXT COMMENT ENTRY.
000400 DATE-WRITTEN. TODAY.
000500*A COMMENT LINE
000600 ENVIRONMENT DIVISION.
000700 DATA DIVISION.
000800 WORKING-STORAGE SECTION.
000900 01  WS-TEXT     PIC X(60) VALUE \"A STRING LITERAL THAT RUNS TO THE
001000-    \"CONTINUED HERE\".
001100 01  WS-N        PIC 9(4) VALUE 0.
001200 PROCEDURE DIVISION.
001300 MAIN-PARA.
001400     EXEC SQL SELECT A INTO :WS-N FROM T END-EXEC.
001500     EXEC CICS RETURN END-EXEC.
001600     PERFORM WORK-PARA THRU WORK-EXIT.
001700     DISPLAY 'DONE' WS-TEXT.                                      IGNORED
001800     STOP RUN.
001900 WORK-PARA.
002000     ADD 1 TO WS-N.
002100 WORK-EXIT.
002200     EXIT.
";

const SAMPLE: &str = include_str!("fixtures/cobol/sample.cob");

/// xorshift64*: deterministic, so a failure names an input that reproduces.
struct Rng(u64);

impl Rng {
    fn step(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.step() % n.max(1) as u64) as usize
    }
}

const PIECES: &[&str] = &[
    "\n", "\r\n", " ", "      ", "*", "-", "/", "\"", "'", ".", "\u{1a}", "\0", "\t", "EXEC ",
    "SQL ", "CICS ", "END-EXEC", "PERFORM ", "THRU ", "DIVISION", "AUTHOR.", "000100", "X",
];

fn inputs() -> Vec<(String, String)> {
    let seeds = [SAMPLE, PROGRAM];
    let mut out = Vec::new();
    for (s, seed) in seeds.iter().enumerate() {
        let mut starts = vec![0];
        starts.extend(seed.match_indices('\n').map(|(i, _)| i + 1));
        for start in starts {
            for k in 0..=8 {
                let end = (start + k).min(seed.len());
                out.push((
                    format!("seed {s} cut at byte {end}"),
                    seed[..end].to_string(),
                ));
            }
        }
    }
    for &piece in PIECES {
        out.push((format!("repeat {piece:?}"), piece.repeat(2_000)));
        out.push((format!("{piece:?} alone"), piece.to_string()));
    }
    out.push(("long line".to_string(), "X".repeat(100_000)));
    let mut rng = Rng(0x0322_0634);
    for i in 0..200 {
        let mut text = seeds[rng.below(seeds.len())].to_string();
        for _ in 0..1 + rng.below(8) {
            let at = rng.below(text.len() + 1);
            match rng.below(4) {
                0 => text.insert_str(at, PIECES[rng.below(PIECES.len())]),
                1 => {
                    let end = (at + 1 + rng.below(40)).min(text.len());
                    text.replace_range(at..end, "");
                }
                2 => {
                    let end = (at + 1 + rng.below(80)).min(text.len());
                    let chunk = text[at..end].repeat(2 + rng.below(20));
                    text.insert_str(at, &chunk);
                }
                _ => text.truncate(at),
            }
        }
        out.push((format!("mutation {i}"), text));
    }
    out
}

#[test]
fn the_cobol_scanner_returns_on_every_fuzz_input() {
    let inputs = inputs();
    // The worker says which input it starts, then `None` once every input has returned.
    let (tell, progress) = channel::<Option<String>>();
    std::thread::spawn(move || {
        for (label, text) in inputs {
            if tell.send(Some(label)).is_err() {
                return;
            }
            // A parse error is expected; only a hang or a panic fails.
            let _ = CicsSqlExtractor::new().extract(&SourceFile {
                path: "FUZZ.cbl".to_string(),
                language: Language::new("cobol"),
                text,
            });
        }
        let _ = tell.send(None);
    });
    let mut last = String::from("(none)");
    loop {
        match progress.recv_timeout(Duration::from_secs(20)) {
            Ok(Some(label)) => last = label,
            Ok(None) => break,
            Err(RecvTimeoutError::Timeout) => {
                panic!("the extraction of {last:?} did not return within 20 s")
            }
            Err(RecvTimeoutError::Disconnected) => panic!("the extraction of {last:?} panicked"),
        }
    }
}
