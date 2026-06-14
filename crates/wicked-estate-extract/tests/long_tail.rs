//! Long-tail coverage sweep — verify the newly-wired languages actually extract symbols through
//! their authored `.scm` queries (query compiles + captures bind to real node types). One failing
//! language is a query-vs-grammar mismatch to fix, not a flake.

use wicked_estate_core::{EdgeKind, Extractor, Language, NodeKind, SourceFile};
use wicked_estate_extract::treesitter::TreeSitterExtractor;

struct Case {
    lang: &'static str,
    file: &'static str,
    src: &'static str,
    min_defs: usize,
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            lang: "solidity",
            file: "wallet.sol",
            src: "pragma solidity ^0.8.0;\nimport \"./IToken.sol\";\ncontract Wallet {\n    uint256 public balance;\n    function deposit(uint256 amount) public { balance = add(balance, amount); }\n    function add(uint256 a, uint256 b) internal pure returns (uint256) { return a + b; }\n}\n",
            min_defs: 3,
        },
        Case {
            lang: "thrift",
            file: "svc.thrift",
            src: "include \"shared.thrift\"\nenum Status { ACTIVE = 1, INACTIVE = 2 }\nstruct User {\n  1: required i64 id,\n  2: optional string name,\n}\nservice UserService {\n  User getUser(1: i64 id),\n  void deleteUser(1: i64 id),\n}\n",
            min_defs: 3,
        },
        Case {
            lang: "verilog",
            file: "counter.v",
            src: "module counter(input clk, input rst, output reg [7:0] count);\n  always @(posedge clk) begin\n    if (rst) count <= 8'b0;\n    else count <= count + 1;\n  end\nendmodule\nmodule top;\n  counter c0(.clk(clk), .rst(rst), .count(count));\nendmodule\n",
            min_defs: 2,
        },
        Case {
            lang: "vhdl",
            file: "counter.vhd",
            src: "entity counter is\n  port (clk : in bit; count : out integer);\nend entity counter;\narchitecture rtl of counter is\nbegin\n  process (clk)\n  begin\n  end process;\nend architecture rtl;\n",
            min_defs: 2,
        },
        Case {
            lang: "d",
            file: "app.d",
            src: "module app.greeter;\nimport std.stdio;\nclass Greeter {\n    string name;\n    void greet() { writeln(\"hi\", name); }\n}\nint add(int a, int b) { return a + b; }\nvoid main() { writeln(add(2, 3)); }\n",
            min_defs: 3,
        },
        Case {
            lang: "starlark",
            file: "defs.bzl",
            src: "load(\"//tools:helpers.bzl\", \"make_label\")\nDEFAULT_TAGS = [\"manual\"]\ndef cc_wrapper(name, srcs, deps = []):\n    label = make_label(name)\n    native.cc_library(name = label, srcs = srcs)\n    return label\n",
            min_defs: 1,
        },
        Case {
            lang: "cuda",
            file: "saxpy.cu",
            src: "#include <cstdio>\n__global__ void saxpy(int n, float a, float *x, float *y) {\n    int i = blockIdx.x * blockDim.x + threadIdx.x;\n    if (i < n) y[i] = a * x[i] + y[i];\n}\nint main() {\n    saxpy<<<16, 256>>>(4096, 2.0f, 0, 0);\n    printf(\"done\\n\");\n    return 0;\n}\n",
            min_defs: 2,
        },
        Case {
            lang: "arduino",
            file: "blink.ino",
            src: "#include <Arduino.h>\nconst int LED = 13;\nvoid setup() { pinMode(LED, OUTPUT); }\nvoid loop() { digitalWrite(LED, HIGH); delay(1000); }\n",
            min_defs: 2,
        },
        Case {
            lang: "apex",
            file: "AccountService.cls",
            src: "public class AccountService implements Schedulable {\n    private Integer maxRetries = 3;\n    public AccountService() { this.maxRetries = 5; }\n    public void execute(SchedulableContext ctx) {\n        Account a = new Account(Name = 'Acme');\n        System.debug(a.Id);\n    }\n}\n",
            min_defs: 3,
        },
        Case {
            lang: "racket",
            file: "util.rkt",
            src: "#lang racket\n(require racket/string)\n(define MAX-SIZE 100)\n(define (greet name)\n  (string-append \"hi \" name))\n(define (main)\n  (displayln (greet \"world\")))\n",
            min_defs: 2,
        },
    ]
}

#[test]
fn newly_wired_languages_extract_symbols() {
    let mut failures = Vec::new();
    let mut report = Vec::new();
    for c in cases() {
        let Some(ex) = TreeSitterExtractor::for_language(c.lang) else {
            failures.push(format!("{}: NOT REGISTERED in LANG_TABLE", c.lang));
            continue;
        };
        let sf = SourceFile {
            path: c.file.to_string(),
            language: Language::new(c.lang),
            text: c.src.to_string(),
        };
        let extraction = match ex.extract(&sf) {
            Ok(e) => e,
            Err(e) => {
                failures.push(format!("{}: extract error: {e}", c.lang));
                continue;
            }
        };
        let defs = extraction
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        let calls = extraction
            .refs
            .iter()
            .filter(|r| r.kind == EdgeKind::Calls)
            .count();
        report.push(format!("{}: {defs} defs, {calls} calls", c.lang));
        if defs < c.min_defs {
            failures.push(format!(
                "{}: expected >={} defs, got {defs} (query vs grammar mismatch)",
                c.lang, c.min_defs
            ));
        }
    }
    eprintln!("long-tail extraction:\n  {}", report.join("\n  "));
    assert!(
        failures.is_empty(),
        "long-tail failures:\n  {}",
        failures.join("\n  ")
    );
}
