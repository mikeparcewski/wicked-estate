//! (wicked-estate#333, after wicked-core#957) Opening the memory store must not kill the thread that
//! opens it when the process's stderr refuses every write. A Node host (wicked-crew loads
//! wicked-core, which links this crate, as a napi addon) hands the process a NON-BLOCKING stderr
//! pipe; when its reader falls behind, a write fails with EAGAIN. std's `eprintln!` panics on a
//! failed write, and the panic message is lost the same way. The embedder fallback in
//! `MemoryEngine::open` (model2vec unavailable, so HashEmbedder) prints a line there, so on a host
//! with no network or no cached model the engine thread that opened the store died with no trace.
//!
//! This test runs itself as a child whose stderr is a full non-blocking pipe, makes the model2vec
//! load fail offline (an existing, empty model directory), opens the store on a spawned thread and
//! requires that thread to finish and the store to work.
// model2vec only: with `semantic-bge` too, the engine picks FastEmbedder, which this test cannot
// force to fail offline.
#![cfg(all(unix, feature = "semantic", not(feature = "semantic-bge")))]

use std::time::{Duration, Instant};

const CHILD: &str = "WICKED_ESTATE_333_STDERR_CHILD";

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("wicked-estate-333-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The child's work: open the store on its own thread (an engine thread in the host) and use it.
/// Returns what the parent asserts on; never panics on this thread (a panic message has nowhere to
/// go), so a refused write shows up as `thread panicked` in the result file.
fn child_open_store() -> String {
    let dir = scratch("child");
    let db = dir.join("memory.db").to_str().unwrap().to_string();
    let worker = std::thread::spawn(move || -> Result<usize, String> {
        let eng = wicked_estate_memory::MemoryEngine::open(&db).map_err(|e| e.to_string())?;
        eng.all_memories()
            .map(|m| m.len())
            .map_err(|e| e.to_string())
    });
    let got = match worker.join() {
        Ok(Ok(n)) => format!("opened {n}"),
        Ok(Err(e)) => format!("open failed: {e}"),
        Err(_) => "thread panicked".to_string(),
    };
    let _ = std::fs::remove_dir_all(&dir);
    got
}

#[test]
fn memory_store_opens_when_stderr_refuses_every_write() {
    if let Some(result) = std::env::var_os(CHILD) {
        let got = child_open_store();
        std::fs::write(result, got).unwrap();
        return;
    }
    use std::os::fd::FromRawFd;
    let mut fds = [0i32; 2];
    // SAFETY: a plain pipe(2); both fds are owned below.
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0, "pipe");
    let (read_end, write_end) = (fds[0], fds[1]);
    // SAFETY: fcntl on the fd we just created.
    unsafe {
        let fl = libc::fcntl(write_end, libc::F_GETFL);
        assert_eq!(
            libc::fcntl(write_end, libc::F_SETFL, fl | libc::O_NONBLOCK),
            0
        );
    }
    // Fill it: the reader (this test) never reads, so every later write by the child is EAGAIN.
    let chunk = [b'x'; 4096];
    loop {
        // SAFETY: writing a stack buffer to our own fd.
        let n = unsafe { libc::write(write_end, chunk.as_ptr().cast(), chunk.len()) };
        if n < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock, "fill: {err}");
            break;
        }
    }
    // Full: even one byte is refused.
    // SAFETY: writing a stack byte to our own fd.
    let one = unsafe { libc::write(write_end, chunk.as_ptr().cast(), 1) };
    assert_eq!(
        (one, std::io::Error::last_os_error().kind()),
        (-1, std::io::ErrorKind::WouldBlock),
        "the pipe must refuse a one-byte write"
    );
    let parent = scratch("parent");
    let result = parent.join("result.txt");
    // An existing directory with no model in it: model2vec fails to load without touching the
    // network, so `MemoryEngine::open` takes the HashEmbedder fallback and prints its line.
    let empty_model = parent.join("no-model");
    std::fs::create_dir_all(&empty_model).unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "memory_store_opens_when_stderr_refuses_every_write",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD, &result)
        .env("CI_MODEL2VEC_MODEL", &empty_model)
        .stdout(std::process::Stdio::null())
        // SAFETY: `write_end` is ours and moves into the child's stderr (closed here on drop).
        .stderr(unsafe { std::process::Stdio::from_raw_fd(write_end) })
        .spawn()
        .expect("spawn the child");
    let deadline = Instant::now() + Duration::from_secs(120);
    let status = loop {
        if let Some(st) = child.try_wait().unwrap() {
            break Some(st);
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    // SAFETY: closing the read end we own.
    unsafe { libc::close(read_end) };
    let got = std::fs::read_to_string(&result).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&parent);
    assert!(
        status.is_some_and(|s| s.success()) && got == "opened 0",
        "with a stderr that refuses writes, the memory store must still open on its thread: \
         child {status:?}, result {got:?}"
    );
}
