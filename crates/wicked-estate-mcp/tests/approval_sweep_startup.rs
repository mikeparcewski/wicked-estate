//! The invisible-until-complete protocol through the real binary (DES-KNOWLEDGE-DATA-001 §10.1):
//! an approval interrupted after its node was written (its journal still there, the proposal still
//! pending) is hidden by a `--readonly` server, which writes nothing, and rolled back by the next
//! write-mode server's start-up sweep before it answers a single read. On-disk stores, a real
//! process per phase.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use wicked_estate_knowledge::{KnowledgeApi, KnowledgeEngine};
use wicked_estate_memory::MemoryEngine;
use wicked_estate_memory_core::ProposalState;
use wicked_estate_overlay::XedgeStore;

/// Run one server process over `home`, send `requests` (after initialize), return the responses
/// by id. Stdin closes after the last request; the server exits.
fn serve(home: &Path, readonly: bool, requests: &[Value]) -> Vec<Value> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_wicked-estate-mcp"));
    cmd.env_clear()
        .env("HOME", home)
        .env("WICKED_HOME", home)
        .env("WICKED_ESTATE_DB", home.join("graph.db"))
        .current_dir(home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if readonly {
        cmd.arg("--readonly");
    }
    let mut child = cmd.spawn().expect("spawn wicked-estate-mcp");
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        let init = json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{
            "protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"sweep","version":"0"}}});
        writeln!(stdin, "{init}").unwrap();
        for r in requests {
            writeln!(stdin, "{r}").unwrap();
        }
    }
    drop(child.stdin.take());
    let out = child.stdout.take().expect("stdout");
    let responses: Vec<Value> = BufReader::new(out)
        .lines()
        .map_while(Result::ok)
        .filter_map(|l| serde_json::from_str(&l).ok())
        .filter(|v: &Value| v["id"].as_u64().is_some_and(|id| id > 0))
        .collect();
    let _ = child.wait();
    responses
}

fn recall(id: u64) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{
        "name":"knowledge.recall","arguments":{"query":"refunds over 500 need a manager"}}})
}

fn items(resp: &Value) -> Vec<Value> {
    let text = resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("no tool result: {resp}"));
    let v: Value = serde_json::from_str(text).unwrap();
    v["items"].as_array().cloned().unwrap_or_default()
}

fn engines(home: &Path) -> (MemoryEngine, KnowledgeEngine) {
    let xedge = Arc::new(XedgeStore::open(home.join("xedge.db").to_str().unwrap()).unwrap());
    let mem = MemoryEngine::open(home.join("memory.db").to_str().unwrap())
        .unwrap()
        .with_xedge_store(Arc::clone(&xedge));
    let know = KnowledgeEngine::open(home.join("knowledge.db").to_str().unwrap())
        .unwrap()
        .with_xedge_store(xedge);
    (mem, know)
}

#[test]
fn readonly_hides_an_interrupted_approval_and_the_write_mode_startup_sweep_rolls_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // An approval that died after writing its node (journal at t=1: long past the grace).
    let pid = {
        let (mut mem, mut know) = engines(home);
        let pid = mem
            .submit_proposal(
                "knowledge",
                json!({"content": "refunds over 500 need a manager", "source": "docs/refunds.md"}),
                Default::default(),
                Default::default(),
                1,
            )
            .unwrap();
        let proposal = mem
            .list_proposals(Some("knowledge"), Some(ProposalState::Pending))
            .unwrap()
            .remove(0);
        know.write_proposal(&proposal, &Default::default(), 1)
            .unwrap();
        assert_eq!(know.applying_proposals().unwrap().len(), 1);
        pid
    };

    // A read-only server writes nothing, so it cannot sweep: it hides the node instead.
    let ro = serve(home, true, &[recall(1)]);
    assert_eq!(ro.len(), 1, "{ro:?}");
    assert!(
        items(&ro[0]).is_empty(),
        "readonly must hide it: {:?}",
        ro[0]
    );
    {
        let (_, know) = engines(home);
        assert_eq!(
            know.applying_proposals().unwrap().len(),
            1,
            "readonly left the journal alone"
        );
    }

    // The write-mode server sweeps before its first read: rolled back, still pending.
    let rw = serve(home, false, &[recall(1)]);
    assert!(items(&rw[0]).is_empty(), "{:?}", rw[0]);
    let (mut mem, mut know) = engines(home);
    assert!(know.applying_proposals().unwrap().is_empty(), "swept");
    let pending = mem
        .list_proposals(Some("knowledge"), Some(ProposalState::Pending))
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, pid);
    // And it approves cleanly afterwards.
    let mut writer = |p: &wicked_estate_memory_core::Proposal, now: i64| {
        know.write_proposal(p, &Default::default(), now)
    };
    mem.approve_proposal_with_knowledge(&pid, 5, &mut writer)
        .unwrap();
    know.finish_proposal(&pid).unwrap();
    drop((mem, know));
    let after = serve(home, true, &[recall(1)]);
    assert_eq!(items(&after[0]).len(), 1, "{:?}", after[0]);
}
