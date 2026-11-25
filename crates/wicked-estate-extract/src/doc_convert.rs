//! Binary documents (PDF, Word, PowerPoint, Excel, OpenDocument, RTF, EPUB) to GitHub-flavored
//! Markdown with the [anydoc] crate. This is the ONE converter estate uses wherever it meets a
//! document it cannot read as text: the repo index stores and searches the Markdown under the
//! document's own path, and `knowledge.ingest {path}` chunks it.
//!
//! Local only. The anydoc Rust crate has no OCR and no network code (its hosted OCR lives only in
//! the npm wrapper, which estate does not use): a scanned or image-only PDF comes back as
//! [`Skipped::NeedsOcr`], and nothing leaves the host. The test
//! `anydoc_dependency_closure_has_no_network_client` is a tripwire on the dependency closure.
//!
//! Bounds: input is capped at [`MAX_INPUT_BYTES`] before it is read ([`read_capped`]), and
//! converted text at [`MAX_TEXT_BYTES`]. The shipped binaries convert in a supervised child with
//! a [`MAX_CONVERT_MEMORY_BYTES`] live heap budget and a [`CONVERT_DEADLINE`] wall-clock limit.
//! Every allocation the budget refuses becomes a memory skip, whether the converter recovers or the
//! refusal aborts the child (the budget marks its first refusal on the child's stderr, so the
//! parent tells a budget abort from a crash); any other crash or abort stays inside that child.
//! A timed-out child is killed and reaped. At most one child per indexer worker converts at once.
//! Library callers that do not enable isolation convert in-process with unwind containment only.
//! Skip reasons are fixed strings; document text and source paths never reach a child diagnostic.
//!
//! [anydoc]: https://github.com/firecrawl/anydoc

use std::alloc::{GlobalAlloc, Layout, System};
use std::fmt;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Largest document converted, in bytes (32 MiB). Bigger files are skipped, never parsed.
pub const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;

/// Maximum live heap held by one isolated conversion (1 GiB).
pub const MAX_CONVERT_MEMORY_BYTES: usize = 1 << 30;

/// Maximum wall-clock time for one isolated conversion.
pub const CONVERT_DEADLINE: Duration = Duration::from_secs(120);

/// Private child mode shared by the two shipped binaries.
pub const CHILD_ARG: &str = "__doc-convert";

const MEMORY_REASON: &str = "over the 1 GiB conversion memory budget";
const DEADLINE_REASON: &str = "took longer than the 120 s conversion limit";
const UNAVAILABLE_REASON: &str = "isolated converter unavailable";
/// Child-mode argument after the format hint: mask the Markdown before replying (#556).
const MASK_ARG: &str = "mask";
/// Child-mode argument prefix, `deadline=<ms>`: the child's own wall-clock limit (#556 R2). The
/// parent enforces the same limit, but a caller that kills the parent leaves the child with no
/// one to kill it; the child's watchdog ends it at this deadline anyway. Absent or unparsable
/// means [`CONVERT_DEADLINE`], so a child is never unbounded.
const DEADLINE_ARG: &str = "deadline=";
/// Set by whichever of the child's main thread and its watchdog replies first; the other never
/// writes, so the parent reads exactly one reply.
static CHILD_REPLIED: AtomicBool = AtomicBool::new(false);
/// Written once to the child's stderr when the budget first refuses an allocation. An infallible
/// allocation the budget refuses aborts the child before it can reply; this line is how the parent
/// still reports the budget, not a crash.
const BUDGET_MARK: &[u8] = b"wicked-estate: conversion memory budget refused an allocation\n";
/// Child stderr the parent reads; the rest is discarded.
const CHILD_STDERR_BYTES: u64 = 4096;
const UNREADABLE_REASONS: &[&str] = &[
    "cannot open",
    "not a regular file",
    "cannot read",
    "unrecognized format",
    "over a converter safety limit",
    "unsupported content",
    "missing part",
    "malformed document",
    "no text",
    "converted text is over the 1 MiB limit",
    MEMORY_REASON,
    DEADLINE_REASON,
    UNAVAILABLE_REASON,
];

/// A System allocator with an optional live-heap budget. Unarmed calls only load the limit.
/// Arm once, at child startup: freeing pre-arm blocks adds headroom through the signed counter.
pub struct BudgetAlloc {
    limit: AtomicUsize,
    live: AtomicIsize,
    refused: AtomicBool,
}

impl BudgetAlloc {
    /// Create an unarmed allocator.
    pub const fn new() -> Self {
        Self {
            limit: AtomicUsize::new(0),
            live: AtomicIsize::new(0),
            refused: AtomicBool::new(false),
        }
    }

    /// Enable accounting; zero leaves the allocator unarmed.
    pub fn arm(&self, limit: usize) {
        self.limit
            .store(limit.min(isize::MAX as usize), Ordering::Relaxed);
    }

    /// Whether any request has been refused by the budget.
    pub fn refused(&self) -> bool {
        self.refused.load(Ordering::Relaxed)
    }

    fn charge(&self, delta: isize, limit: usize) -> bool {
        // A compare-exchange loop (not `fetch_update`, deprecated on current stable).
        let mut live = self.live.load(Ordering::Relaxed);
        loop {
            let Some(next) = live
                .checked_add(delta)
                .filter(|&next| delta <= 0 || next <= limit as isize)
            else {
                if !self.refused.swap(true, Ordering::Relaxed) {
                    note_budget_refusal();
                }
                return false;
            };
            match self
                .live
                .compare_exchange_weak(live, next, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => return true,
                Err(actual) => live = actual,
            }
        }
    }
}

/// Write [`BUDGET_MARK`] to stderr from inside the allocator: one raw write on the stderr
/// descriptor, with no lock, buffer or allocation.
fn note_budget_refusal() {
    #[cfg(unix)]
    let stderr = {
        use std::os::fd::{AsRawFd, FromRawFd};
        // SAFETY: stderr's descriptor is open for the process's life; ManuallyDrop never closes it.
        std::mem::ManuallyDrop::new(unsafe {
            std::fs::File::from_raw_fd(std::io::stderr().as_raw_fd())
        })
    };
    #[cfg(windows)]
    let stderr = {
        use std::os::windows::io::{AsRawHandle, FromRawHandle};
        // SAFETY: the process's stderr handle stays open; ManuallyDrop never closes it.
        std::mem::ManuallyDrop::new(unsafe {
            std::fs::File::from_raw_handle(std::io::stderr().as_raw_handle())
        })
    };
    #[cfg(any(unix, windows))]
    let _ = (&*stderr).write(BUDGET_MARK);
}

impl Default for BudgetAlloc {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: every operation delegates the unchanged pointer and layout to System. Accounting never
// accesses allocated memory, and a failed allocation/reallocation leaves the caller's block alone.
unsafe impl GlobalAlloc for BudgetAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let limit = self.limit.load(Ordering::Relaxed);
        if limit != 0 && !self.charge(layout.size() as isize, limit) {
            return std::ptr::null_mut();
        }
        // SAFETY: the caller supplies a valid nonzero allocation layout, forwarded to System.
        let ptr = unsafe { System.alloc(layout) };
        if limit != 0 && ptr.is_null() {
            self.live
                .fetch_sub(layout.size() as isize, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let limit = self.limit.load(Ordering::Relaxed);
        if limit != 0 && !self.charge(layout.size() as isize, limit) {
            return std::ptr::null_mut();
        }
        // SAFETY: the caller supplies a valid nonzero allocation layout, forwarded to System.
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if limit != 0 && ptr.is_null() {
            self.live
                .fetch_sub(layout.size() as isize, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let limit = self.limit.load(Ordering::Relaxed);
        // SAFETY: ptr and its original layout come from the caller and are forwarded unchanged.
        unsafe { System.dealloc(ptr, layout) };
        if limit != 0 {
            self.live
                .fetch_sub(layout.size() as isize, Ordering::Relaxed);
        }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let limit = self.limit.load(Ordering::Relaxed);
        let delta = new_size as isize - layout.size() as isize;
        if limit != 0 && !self.charge(delta, limit) {
            return std::ptr::null_mut();
        }
        // SAFETY: the caller supplies the live block, original layout and valid new size.
        let resized = unsafe { System.realloc(ptr, layout, new_size) };
        if limit != 0 && resized.is_null() {
            self.live.fetch_sub(delta, Ordering::Relaxed);
        }
        resized
    }
}

static ISOLATED_EXE: OnceLock<std::io::Result<PathBuf>> = OnceLock::new();

/// Route subsequent conversions through this binary's child mode, including on lookup failure.
pub fn isolate_conversions() {
    ISOLATED_EXE.get_or_init(std::env::current_exe);
}

/// Run the hidden conversion mode before any runtime or application initialization.
pub fn run_child_if_requested(alloc: &'static BudgetAlloc) -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(CHILD_ARG)) {
        return None;
    }
    // The parent reads stderr only for the budget mark; a caught panic's message is noise.
    std::panic::set_hook(Box::new(|_| {}));
    let hint = args.next().unwrap_or_default();
    let ext = format_hint(hint.to_str().unwrap_or(""));
    let (mut mask, mut deadline) = (false, CONVERT_DEADLINE);
    for arg in args {
        match arg.to_str() {
            Some(MASK_ARG) => mask = true,
            Some(a) => {
                if let Some(ms) = a.strip_prefix(DEADLINE_ARG).and_then(|v| v.parse().ok()) {
                    deadline = Duration::from_millis(ms).min(CONVERT_DEADLINE);
                }
            }
            None => {}
        }
    }
    // Started before the budget is armed, so its own allocation is never refused.
    let watchdog = std::thread::Builder::new().spawn(move || {
        std::thread::sleep(deadline);
        if reply_once(&Err(Skipped::Unreadable(DEADLINE_REASON))) {
            std::process::exit(0);
        }
    });
    if watchdog.is_err() {
        // Without its watchdog the child would be bounded only by a parent that may be gone.
        reply_once(&Err(Skipped::Panicked));
        return Some(0);
    }
    alloc.arm(MAX_CONVERT_MEMORY_BYTES);
    let mut bytes = Vec::new();
    let mut reply = match std::io::stdin()
        .take(MAX_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
    {
        Ok(_) if bytes.len() > MAX_INPUT_BYTES => Err(Skipped::TooLarge(bytes.len() as u64)),
        Ok(_) => {
            convert_in_process(ext, &bytes).map(|md| if mask { mask_markdown(&md) } else { md })
        }
        Err(_) => Err(Skipped::Unreadable("cannot read")),
    };
    if alloc.refused() {
        reply = Err(Skipped::Unreadable(MEMORY_REASON));
    }
    if !reply_once(&reply) {
        // The watchdog is replying and exits the process when it is done.
        loop {
            std::thread::park();
        }
    }
    Some(0)
}

/// Write the child's one reply, unless the other writer (main thread or watchdog) already has.
/// The caller exits right after; flush explicitly so the reply is never left in a buffer.
fn reply_once(reply: &Result<String, Skipped>) -> bool {
    if CHILD_REPLIED.swap(true, Ordering::SeqCst) {
        return false;
    }
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(encode_reply(reply).as_bytes());
    let _ = out.flush();
    true
}

fn format_hint(ext: &str) -> &str {
    if ext.len() <= 8 && ext.bytes().all(|b| b.is_ascii_alphanumeric()) {
        ext
    } else {
        ""
    }
}

fn encode_reply(reply: &Result<String, Skipped>) -> String {
    match reply {
        Ok(md) => format!("OK\n{md}"),
        Err(Skipped::NeedsOcr(pages)) => format!("SKIP\nocr\t{pages}"),
        Err(Skipped::TooLarge(size)) => format!("SKIP\nlarge\t{size}"),
        Err(Skipped::Encrypted) => "SKIP\nencrypted".to_string(),
        Err(Skipped::Unreadable(reason)) => format!("SKIP\nunreadable\t{reason}"),
        Err(Skipped::Panicked) => "SKIP\npanicked".to_string(),
    }
}

fn decode_reply(bytes: &[u8]) -> Result<String, Skipped> {
    let wire = std::str::from_utf8(bytes).map_err(|_| Skipped::Panicked)?;
    if let Some(md) = wire.strip_prefix("OK\n") {
        return if md.len() <= MAX_TEXT_BYTES && !md.trim().is_empty() {
            Ok(md.to_string())
        } else {
            Err(Skipped::Panicked)
        };
    }
    let Some(skip) = wire.strip_prefix("SKIP\n") else {
        return Err(Skipped::Panicked);
    };
    let (kind, arg) = skip.split_once('\t').unwrap_or((skip, ""));
    Err(match kind {
        // Page numbers and ranges only ("pages 2-3, 5 of 6 need OCR"): no path or text survives.
        "ocr" => Skipped::NeedsOcr(
            arg.chars()
                .filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | ','))
                .take(64)
                .collect(),
        ),
        "large" => arg
            .parse()
            .map(Skipped::TooLarge)
            .unwrap_or(Skipped::Panicked),
        "encrypted" if arg.is_empty() => Skipped::Encrypted,
        "unreadable" => UNREADABLE_REASONS
            .iter()
            .find(|&&reason| reason == arg)
            .map(|&reason| Skipped::Unreadable(reason))
            .unwrap_or(Skipped::Panicked),
        _ => Skipped::Panicked,
    })
}

fn supervise(mut cmd: Command, bytes: &[u8], deadline: Duration) -> Result<String, Skipped> {
    let start = Instant::now();
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| Skipped::Unreadable(UNAVAILABLE_REASON))?;
    let mut stdin = child.stdin.take().ok_or(Skipped::Panicked)?;
    let stdout = child.stdout.take().ok_or(Skipped::Panicked)?;
    let stderr = child.stderr.take().ok_or(Skipped::Panicked)?;
    // Scoped threads borrow the input (up to 32 MiB) instead of copying it.
    let (status, output, errors) = std::thread::scope(|scope| {
        let writer = scope.spawn(move || stdin.write_all(bytes));
        // Capped, then the pipe is dropped: a child writing more gets EPIPE, never a full pipe.
        let errors = scope.spawn(move || {
            let mut errors = Vec::new();
            let _ = stderr.take(CHILD_STDERR_BYTES).read_to_end(&mut errors);
            errors
        });
        let reader = scope.spawn(move || {
            let mut output = Vec::new();
            stdout
                .take((MAX_TEXT_BYTES + 4096) as u64)
                .read_to_end(&mut output)?;
            Ok::<_, std::io::Error>(output)
        });
        let mut backoff = Duration::from_millis(1);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) if start.elapsed() < deadline => {
                    std::thread::sleep(backoff.min(deadline.saturating_sub(start.elapsed())));
                    backoff = (backoff * 2).min(Duration::from_millis(25));
                }
                state => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(if state.is_err() {
                        Skipped::Panicked
                    } else {
                        Skipped::Unreadable(DEADLINE_REASON)
                    });
                }
            }
        };
        // Killing and reaping our child closes both pipe ends, so blocked I/O can finish too.
        let _ = writer.join();
        (status, reader.join(), errors.join().unwrap_or_default())
    });
    let output = output.map_err(|_| Skipped::Panicked)?;
    if !status?.success() {
        let budget = errors.windows(BUDGET_MARK.len()).any(|w| w == BUDGET_MARK);
        return Err(if budget {
            Skipped::Unreadable(MEMORY_REASON)
        } else {
            Skipped::Panicked
        });
    }
    decode_reply(&output.map_err(|_| Skipped::Panicked)?)
}

/// Extensions (lowercase, no dot) this converter reads: every extension anydoc names except CSV,
/// which is already text.
pub const DOC_EXTENSIONS: &[&str] = &[
    "pdf", "doc", "docx", "docm", "odt", "rtf", "epub", "ppt", "pps", "pot", "pptx", "pptm",
    "ppsx", "ppsm", "odp", "xls", "xlsx", "xlsm", "xlsb", "ods",
];

/// True when `ext` (any case, no dot) names a document this converter reads.
pub fn is_doc_ext(ext: &str) -> bool {
    let ext = ext.to_ascii_lowercase();
    DOC_EXTENSIONS.contains(&ext.as_str())
}

/// True when `path`'s final extension names a document this converter reads.
pub fn is_doc_path(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, e)| is_doc_ext(e))
}

/// Why a document was not converted. Never fatal to the caller: it records the reason and moves on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skipped {
    /// Scanned or image-only pages (the pages, e.g. "all 3 pages need OCR"): anydoc does no OCR,
    /// and estate never sends a document out.
    NeedsOcr(String),
    /// Over [`MAX_INPUT_BYTES`] (at least this many bytes).
    TooLarge(u64),
    /// Encrypted or password-protected.
    Encrypted,
    /// Unreadable, unsupported, over one of the converter's safety limits, or empty of text. A
    /// fixed reason, never document text.
    Unreadable(&'static str),
    /// The converter panicked on this input.
    Panicked,
}

impl fmt::Display for Skipped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Skipped::NeedsOcr(pages) => write!(f, "needs OCR ({pages}); not converted"),
            Skipped::TooLarge(n) => write!(
                f,
                "too large ({n} bytes, limit {MAX_INPUT_BYTES}); not converted"
            ),
            Skipped::Encrypted => write!(f, "encrypted; not converted"),
            Skipped::Unreadable(why) => write!(f, "not convertible ({why})"),
            Skipped::Panicked => write!(f, "converter failed on this file; not converted"),
        }
    }
}

impl Skipped {
    /// A stable, machine-readable name for this refusal, for callers that branch on it
    /// (`wicked-estate convert` exits with a code per kind, wicked-estate#556). The library owns
    /// the reason strings, so it owns this mapping too.
    pub fn kind(&self) -> &'static str {
        match self {
            Skipped::NeedsOcr(_) => "needs_ocr",
            Skipped::TooLarge(_) => "too_large",
            Skipped::Encrypted => "encrypted",
            Skipped::Panicked => "failed",
            Skipped::Unreadable(reason) => match *reason {
                "unrecognized format" | "unsupported content" => "unsupported",
                "missing part" | "malformed document" => "malformed",
                "no text" => "empty",
                "cannot open" | "not a regular file" | "cannot read" => "unreadable",
                DEADLINE_REASON => "deadline",
                MEMORY_REASON
                | "over a converter safety limit"
                | "converted text is over the 1 MiB limit" => "over_budget",
                _ => "failed",
            },
        }
    }
}

/// Read `path`, refusing past [`MAX_INPUT_BYTES`] without reading the rest (the size is checked
/// before the read, and the read itself is capped, so a file that grows meanwhile is still bounded).
/// Only a regular file is read: a FIFO or device named `x.pdf` is refused without blocking, also
/// when the path is swapped for one after the check (the open does not block, and the opened
/// descriptor's own type and size are checked again).
pub fn read_capped(path: &std::path::Path) -> Result<Vec<u8>, Skipped> {
    read_capped_at(path, MAX_INPUT_BYTES)
}

/// [`read_capped`] with a lower cap: `max` is clamped to [`MAX_INPUT_BYTES`].
pub fn read_capped_at(path: &std::path::Path, max: usize) -> Result<Vec<u8>, Skipped> {
    let max = max.min(MAX_INPUT_BYTES);
    let meta = std::fs::metadata(path).map_err(|_| Skipped::Unreadable("cannot open"))?;
    if !meta.is_file() {
        return Err(Skipped::Unreadable("not a regular file"));
    }
    if meta.len() > max as u64 {
        return Err(Skipped::TooLarge(meta.len()));
    }
    let (file, len) = open_regular(path)?;
    if len > max as u64 {
        return Err(Skipped::TooLarge(len));
    }
    let mut bytes = Vec::with_capacity(len as usize);
    file.take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Skipped::Unreadable("cannot read"))?;
    if bytes.len() > max {
        return Err(Skipped::TooLarge(bytes.len() as u64));
    }
    Ok(bytes)
}

/// Open `path` for reading without blocking (a FIFO with no writer would block a plain open), and
/// accept it only if the descriptor itself is a regular file. Returns the file and its length.
fn open_regular(path: &std::path::Path) -> Result<(std::fs::File, u64), Skipped> {
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(libc::O_NONBLOCK);
    }
    let file = opts
        .open(path)
        .map_err(|_| Skipped::Unreadable("cannot open"))?;
    let meta = file
        .metadata()
        .map_err(|_| Skipped::Unreadable("cannot open"))?;
    if !meta.is_file() {
        return Err(Skipped::Unreadable("not a regular file"));
    }
    Ok((file, meta.len()))
}

/// Largest converted text kept, in bytes (1 MiB, the index's own text limit). Both callers get it.
pub const MAX_TEXT_BYTES: usize = 1024 * 1024;

/// Convert one document to Markdown. `path` only names the format when the bytes carry no
/// signature (anydoc detects the format from the content first). Inline-delimiter escapes are
/// removed (`settle\_invoice` reads `settle_invoice`, so identifiers and token shapes match as in
/// any text); nothing else is changed. It is NOT redacted here: callers redact it with the markdown
/// rules, like any `.md` file.
pub fn to_markdown(path: &str, bytes: &[u8]) -> Result<String, Skipped> {
    to_markdown_within(path, bytes, CONVERT_DEADLINE)
}

/// [`to_markdown`] with a shorter wall-clock limit for the isolated child: `deadline` is clamped
/// to [`CONVERT_DEADLINE`]. Without isolation ([`isolate_conversions`] not called) there is no
/// child to time out, so the limit does not apply.
pub fn to_markdown_within(path: &str, bytes: &[u8], deadline: Duration) -> Result<String, Skipped> {
    convert_bounded(path, bytes, deadline, false)
}

/// [`to_markdown_within`], masked with [`mask_markdown`] INSIDE the isolated child, so masking runs
/// under the same deadline and memory budget as the conversion (`wicked-estate convert`, #556).
pub fn to_masked_markdown_within(
    path: &str,
    bytes: &[u8],
    deadline: Duration,
) -> Result<String, Skipped> {
    convert_bounded(path, bytes, deadline, true)
}

/// The masking `knowledge.ingest` gives a converted document: the Markdown source rules (as the
/// index masks a `.md` file), then the free-text rules every knowledge node gets (known token
/// shapes and secret-named assignments, estate#406). Same length as the input.
pub fn mask_markdown(md: &str) -> String {
    let md = crate::redact::redact_source("convert.md", md);
    wicked_estate_core::redact::redact_free_text(&md).into_owned()
}

fn convert_bounded(
    path: &str,
    bytes: &[u8],
    deadline: Duration,
    mask: bool,
) -> Result<String, Skipped> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(Skipped::TooLarge(bytes.len() as u64));
    }
    let ext = path.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    if let Some(exe) = ISOLATED_EXE.get() {
        let exe = exe
            .as_ref()
            .map_err(|_| Skipped::Unreadable(UNAVAILABLE_REASON))?;
        let deadline = deadline.min(CONVERT_DEADLINE);
        return supervise(child_command(exe, ext, mask, deadline), bytes, deadline);
    }
    let md = convert_in_process(ext, bytes)?;
    Ok(if mask { mask_markdown(&md) } else { md })
}

/// The isolated child's command line: the format hint, its own deadline, and the mask flag.
fn child_command(exe: &std::path::Path, ext: &str, mask: bool, deadline: Duration) -> Command {
    let mut cmd = Command::new(exe);
    cmd.arg(CHILD_ARG)
        .arg(format_hint(ext))
        .arg(format!("{DEADLINE_ARG}{}", deadline.as_millis()));
    if mask {
        cmd.arg(MASK_ARG);
    }
    cmd
}

fn convert_in_process(ext: &str, bytes: &[u8]) -> Result<String, Skipped> {
    // A malformed document must not take the index run down with it. Detection opens containers
    // too, so it runs inside the unwind boundary as well.
    let converted = {
        std::panic::catch_unwind(|| {
            let format =
                anydoc::Format::from_bytes(bytes).or_else(|| anydoc::Format::from_extension(ext));
            format.map(|f| anydoc::to_markdown_bytes(bytes, f))
        })
        .map_err(|_| Skipped::Panicked)?
    };
    let Some(converted) = converted else {
        return Err(Skipped::Unreadable("unrecognized format"));
    };
    let md = converted.map_err(skipped_for)?;
    if md.trim().is_empty() {
        return Err(Skipped::Unreadable("no text"));
    }
    if md.len() > MAX_TEXT_BYTES {
        return Err(Skipped::Unreadable(
            "converted text is over the 1 MiB limit",
        ));
    }
    Ok(unescape_delimiters(&md))
}

/// The refusal for a converter error. The converter works on bytes already in memory, so its
/// `Io` errors are reads of streams inside the document (OLE streams, PDF objects): a corrupt
/// document, `malformed`, not a file that could not be read (#556 R2).
fn skipped_for(e: anydoc::ConvertError) -> Skipped {
    match e {
        anydoc::ConvertError::NeedsOcr { .. } => {
            // "page 2 of 5 needs OCR" / "all 3 pages need OCR": page numbers only.
            Skipped::NeedsOcr(e.to_string())
        }
        anydoc::ConvertError::Encrypted => Skipped::Encrypted,
        anydoc::ConvertError::ResourceLimit { .. } => {
            Skipped::Unreadable("over a converter safety limit")
        }
        anydoc::ConvertError::Unsupported(_) => Skipped::Unreadable("unsupported content"),
        anydoc::ConvertError::MissingPart { .. } => Skipped::Unreadable("missing part"),
        _ => Skipped::Unreadable("malformed document"),
    }
}

/// `\_` `\*` `\~` and `` \` `` -> the bare character. Only the inline delimiters an escape can split
/// an identifier or token at; block-structure escapes (`\#`, `\-`, `\|`, ...) are kept, so a
/// paragraph that starts with `#` does not become a heading.
fn unescape_delimiters(md: &str) -> String {
    let mut out = String::with_capacity(md.len());
    let mut chars = md.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\'
            && chars
                .peek()
                .is_some_and(|n| matches!(n, '_' | '*' | '~' | '`'))
        {
            continue;
        }
        out.push(c);
    }
    out
}

/// Words a chunk from [`chunk_markdown`] holds at most.
pub const CHUNK_WORDS: usize = 800;

/// A code-fence line: its character and length (CommonMark: 3+ backticks or tildes, indented at
/// most 3 spaces; a backtick fence's info string holds no backtick, so a one-line code span is not
/// a fence).
fn fence_line(line: &str) -> Option<(char, usize)> {
    let t = line.trim_start_matches(' ');
    if line.len() - t.len() > 3 {
        return None;
    }
    let c = t.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let n = t.chars().take_while(|x| *x == c).count();
    let rest = &t[n * c.len_utf8()..];
    (n >= 3 && !(c == '`' && rest.contains('`'))).then_some((c, n))
}

/// `line` in pieces of at most `max` words, cut at word starts so the text between words, spacing
/// included, is kept as written.
fn split_words(line: &str, max: usize) -> Vec<&str> {
    let mut starts = Vec::new();
    let mut prev_space = true;
    for (i, ch) in line.char_indices() {
        if !ch.is_whitespace() && prev_space {
            starts.push(i);
        }
        prev_space = ch.is_whitespace();
    }
    let mut pieces = Vec::new();
    let mut k = 0;
    while k < starts.len() {
        let from = if k == 0 { 0 } else { starts[k] };
        let to = starts.get(k + max).copied().unwrap_or(line.len());
        pieces.push(line[from..to].trim_end());
        k += max;
    }
    pieces
}

/// Split converted Markdown into chunks for knowledge ingest: one per H1/H2 section (a `#` line
/// inside a code fence is not a heading), each packed line by line and cut at a line boundary
/// before it passes [`CHUNK_WORDS`] words, so every chunk is at most that long. Lines are kept as
/// written; only a single line over the limit (a long paragraph or table row) is cut, at word
/// starts. A cut inside a code block is not re-fenced: chunks are retrieval units, and anydoc's
/// Office and PDF output rarely holds code. Chunks with no text are dropped.
pub fn chunk_markdown(md: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    let mut words = 0usize;
    let mut fence: Option<(char, usize)> = None;
    let mut emit = |cur: &mut Vec<&str>, words: &mut usize| {
        let text = cur.join("\n");
        let text = text.trim_start_matches('\n').trim_end();
        if !text.trim().is_empty() {
            chunks.push(text.to_string());
        }
        cur.clear();
        *words = 0;
    };
    for line in md.lines() {
        let is_fence = fence_line(line);
        if fence.is_none() && (line.starts_with("# ") || line.starts_with("## ")) {
            emit(&mut cur, &mut words);
        }
        match (fence, is_fence) {
            // A closing fence: the same character, at least as long, nothing after it.
            (Some((c, n)), Some((c2, n2)))
                if c == c2 && n2 >= n && line.trim().chars().all(|x| x == c) =>
            {
                fence = None;
            }
            (None, Some(open)) => fence = Some(open),
            _ => {}
        }
        let n = line.split_whitespace().count();
        if words + n > CHUNK_WORDS {
            emit(&mut cur, &mut words);
        }
        if n > CHUNK_WORDS {
            let pieces = split_words(line, CHUNK_WORDS);
            let last = pieces.len() - 1;
            for (i, piece) in pieces.into_iter().enumerate() {
                cur.push(piece);
                words = piece.split_whitespace().count();
                if i < last {
                    emit(&mut cur, &mut words);
                }
            }
            continue;
        }
        cur.push(line);
        words += n;
    }
    emit(&mut cur, &mut words);
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_round_trips_markdown_and_every_skip_kind() {
        let replies = [
            Ok("# Runbook\nsettle_invoice".to_string()),
            Err(Skipped::NeedsOcr("all 3 pages need OCR".to_string())),
            Err(Skipped::TooLarge(42_000_000)),
            Err(Skipped::Encrypted),
            Err(Skipped::Unreadable("no text")),
            Err(Skipped::Panicked),
        ];
        for reply in replies {
            assert_eq!(decode_reply(encode_reply(&reply).as_bytes()), reply);
        }
        for reason in UNREADABLE_REASONS {
            let reply = Err(Skipped::Unreadable(reason));
            assert_eq!(decode_reply(encode_reply(&reply).as_bytes()), reply);
        }
    }

    #[test]
    fn wire_rejects_untrusted_output_and_sanitizes_ocr() {
        for bad in [
            b"SKIP\nunreadable\tsource text".as_slice(),
            b"SKIP\nunknown",
            b"OK\n\xff",
            b"",
            b"OK\n",
            b"SKIP\nlarge\tnot a number",
        ] {
            assert_eq!(decode_reply(bad), Err(Skipped::Panicked));
        }
        let oversized = format!("OK\n{}", "x".repeat(MAX_TEXT_BYTES + 1));
        assert_eq!(decode_reply(oversized.as_bytes()), Err(Skipped::Panicked));
        let ocr = format!("SKIP\nocr\tpage /tmp/private.pdf\n{}", "éA!".repeat(80));
        let Err(Skipped::NeedsOcr(pages)) = decode_reply(ocr.as_bytes()) else {
            panic!("expected sanitized OCR reply");
        };
        assert_eq!(pages.len(), 64);
        assert!(
            pages
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b' ' | b'-' | b','))
        );
        assert!(pages.starts_with("page tmpprivatepdf"));
    }

    #[test]
    fn budget_refuses_growth_keeps_the_block_and_credits_frees() {
        use std::alloc::{GlobalAlloc, Layout};

        let alloc = BudgetAlloc::new();
        alloc.arm(32);
        let layout = Layout::from_size_align(16, 8).unwrap();
        // SAFETY: layouts are valid; successful blocks are used within bounds and freed once.
        unsafe {
            let first = alloc.alloc(layout);
            assert!(!first.is_null());
            first.write(73);
            let second = alloc.alloc_zeroed(layout);
            assert!(!second.is_null());
            assert_eq!(second.read(), 0);
            assert!(alloc.alloc(layout).is_null());
            assert!(alloc.alloc_zeroed(layout).is_null());
            assert!(alloc.realloc(first, layout, 24).is_null());
            assert_eq!(
                first.read(),
                73,
                "failed realloc preserves the original block"
            );
            assert!(alloc.refused());
            alloc.dealloc(second, layout);
            let grown = alloc.realloc(first, layout, 24);
            assert!(!grown.is_null());
            assert_eq!(grown.read(), 73);
            let shrunk = alloc.realloc(grown, Layout::from_size_align(24, 8).unwrap(), 8);
            assert!(!shrunk.is_null());
            let rest = alloc.alloc(Layout::from_size_align(24, 8).unwrap());
            assert!(!rest.is_null(), "shrinking realloc credits the delta");
            alloc.dealloc(rest, Layout::from_size_align(24, 8).unwrap());
            alloc.dealloc(shrunk, Layout::from_size_align(8, 8).unwrap());
        }
    }

    #[test]
    fn unarmed_budget_forwards_and_prearm_frees_add_headroom() {
        use std::alloc::{GlobalAlloc, Layout};

        let alloc = BudgetAlloc::new();
        let layout = Layout::from_size_align(64, 8).unwrap();
        // SAFETY: valid layouts, each successful System-backed block is freed exactly once.
        unsafe {
            let block = alloc.alloc(layout);
            assert!(!block.is_null());
            let block = alloc.realloc(block, layout, 128);
            assert!(!block.is_null());
            let zeroed = alloc.alloc_zeroed(layout);
            assert!(!zeroed.is_null());
            alloc.dealloc(zeroed, layout);
            assert!(!alloc.refused());
            alloc.arm(1);
            alloc.dealloc(block, Layout::from_size_align(128, 8).unwrap());
            let block = alloc.alloc(layout);
            assert!(
                !block.is_null(),
                "pre-arm frees cannot wrap the live counter"
            );
            alloc.dealloc(block, layout);
            assert!(!alloc.refused());
        }
    }

    #[cfg(unix)]
    #[test]
    fn supervise_kills_a_stalled_child_before_the_deadline() {
        let mut cmd = std::process::Command::new("sleep");
        cmd.arg("30");
        let start = std::time::Instant::now();
        assert_eq!(
            supervise(cmd, b"", std::time::Duration::from_millis(200)),
            Err(Skipped::Unreadable(DEADLINE_REASON))
        );
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[test]
    fn supervise_contains_a_signal_and_decodes_a_reply_after_stdin_is_closed() {
        let mut crash = std::process::Command::new("sh");
        crash.args(["-c", "kill -9 $$"]);
        assert_eq!(
            supervise(crash, b"input", std::time::Duration::from_secs(5)),
            Err(Skipped::Panicked)
        );
        let mut reply = std::process::Command::new("sh");
        reply.args(["-c", "cat >/dev/null; printf 'SKIP\\nunreadable\\tno text'"]);
        assert_eq!(
            supervise(reply, &[0; 128 * 1024], std::time::Duration::from_secs(5)),
            Err(Skipped::Unreadable("no text"))
        );
    }

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!(
            "{}/tests/fixtures/docs/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("fixture")
    }

    #[test]
    fn docx_converts_to_markdown_with_its_heading_and_identifier() {
        let md = to_markdown("docs/spec.docx", &fixture("spec.docx")).expect("docx converts");
        assert!(md.contains("Ledger reconciliation spec"), "{md}");
        assert!(md.contains("reconcile_ledger"), "{md}");
        // The fixture really carries the canary that callers' markdown redaction must mask.
        assert!(md.contains("AKIAABCDEFGHIJKLMNOP"), "{md}");
    }

    #[test]
    fn pptx_converts_every_slide() {
        let md = to_markdown("deck.pptx", &fixture("deck.pptx")).expect("pptx converts");
        assert!(md.contains("settle_invoice"), "{md}");
        assert!(md.contains("retry_budget"), "{md}");
    }

    #[test]
    fn text_pdf_converts() {
        let md = to_markdown("runbook.pdf", &fixture("runbook.pdf")).expect("pdf converts");
        assert!(md.contains("settle_invoice"), "{md}");
    }

    #[test]
    fn scanned_pdf_is_skipped_as_needs_ocr() {
        let err = to_markdown("scan.pdf", &fixture("scanned.pdf")).expect_err("no text layer");
        assert!(matches!(err, Skipped::NeedsOcr(_)), "{err:?}");
        assert!(err.to_string().starts_with("needs OCR"), "{err}");
    }

    #[test]
    fn oversized_input_is_skipped_before_parsing() {
        let big = vec![0u8; MAX_INPUT_BYTES + 1];
        assert_eq!(
            to_markdown("big.pdf", &big),
            Err(Skipped::TooLarge(MAX_INPUT_BYTES as u64 + 1))
        );
        let tmp = std::env::temp_dir().join(format!("doc-convert-big-{}.pdf", std::process::id()));
        std::fs::write(&tmp, &big).unwrap();
        let read = read_capped(&tmp);
        std::fs::remove_file(&tmp).ok();
        assert_eq!(read, Err(Skipped::TooLarge(MAX_INPUT_BYTES as u64 + 1)));
    }

    #[test]
    fn garbage_with_a_doc_extension_is_a_skip_not_a_panic() {
        let err = to_markdown("x.docx", b"PK\x03\x04 not really a zip").expect_err("garbage");
        assert!(
            matches!(err, Skipped::Unreadable(_) | Skipped::Panicked),
            "{err:?}"
        );
    }

    #[test]
    fn chunks_split_at_h1_h2_outside_fences_and_cap_long_sections() {
        let md = "intro\n\n# One\nbody one\n\n```\n# not a heading\n```\n## Two\nbody two\n### Two.a\nstill two\n";
        let chunks = chunk_markdown(md);
        assert_eq!(chunks.len(), 3, "{chunks:?}");
        assert_eq!(chunks[0], "intro");
        assert!(chunks[1].starts_with("# One") && chunks[1].contains("# not a heading"));
        assert!(chunks[2].starts_with("## Two") && chunks[2].contains("still two"));

        // Paragraphs: cut at the last blank line before the limit.
        let para = "word ".repeat(500);
        let long = format!("# Big\n{para}\n\n{para}\n\n{para}\n");
        let chunks = chunk_markdown(&long);
        assert_eq!(chunks.len(), 3, "{}", chunks.len());
        assert!(
            chunks
                .iter()
                .all(|c| c.split_whitespace().count() <= CHUNK_WORDS + 2)
        );

        // One line over the limit splits by words, so every chunk fits a recall budget.
        let one = format!("# Huge\n\n{}\n", "word ".repeat(3001));
        let chunks = chunk_markdown(&one);
        assert!(chunks.len() >= 4, "{}", chunks.len());
        assert!(
            chunks
                .iter()
                .all(|c| c.split_whitespace().count() <= CHUNK_WORDS)
        );

        // A paragraph over several lines: packed line by line, never past the limit.
        let line = |w: usize| -> String { format!("{}\n", "w ".repeat(w)) };
        let multi = format!("{}\n{}{}", line(100), line(600), line(600));
        let chunks = chunk_markdown(&multi);
        assert!(
            chunks
                .iter()
                .all(|c| c.split_whitespace().count() <= CHUNK_WORDS),
            "{chunks:?}"
        );

        // A 3-backtick line inside a 4-backtick fence does not close it.
        let nested = "# A\n````\n```\n# still code\n```\n````\n## B\nb\n";
        let chunks = chunk_markdown(nested);
        assert_eq!(chunks.len(), 2, "{chunks:?}");
        assert!(chunks[0].contains("# still code"));
    }

    #[test]
    fn a_long_code_block_keeps_its_lines_and_every_chunk_fits() {
        let code: String = (0..810).map(|i| format!("    print({i})\n")).collect();
        let md = format!("# Example\n```python\n{code}```\nafter\n");
        let chunks = chunk_markdown(&md);
        assert!(chunks.len() >= 2, "{}", chunks.len());
        assert!(
            chunks
                .iter()
                .all(|c| c.split_whitespace().count() <= CHUNK_WORDS)
        );
        let joined = chunks.join("\n");
        for i in [0, 405, 809] {
            assert!(
                joined.contains(&format!("    print({i})")),
                "line {i} kept, indented"
            );
        }
        // A one-line code span and a 4-space-indented line are not fences.
        assert_eq!(fence_line("```foo```"), None);
        assert_eq!(fence_line("    ```"), None);
        assert_eq!(fence_line("   ~~~~ rust"), Some(('~', 4)));
        // A long line is cut at word starts: spacing inside each piece is kept as written.
        let line = format!("s = \"a  b\"; {}", "w ".repeat(900));
        let pieces = split_words(&line, CHUNK_WORDS);
        assert_eq!(pieces.len(), 2);
        assert!(
            pieces[0].starts_with("s = \"a  b\";"),
            "{}",
            &pieces[0][..20]
        );
        let total: usize = pieces.iter().map(|p| p.split_whitespace().count()).sum();
        assert_eq!(
            total,
            line.split_whitespace().count(),
            "no word lost or doubled"
        );
    }

    /// A private key inside a long Word paragraph keeps its one-line markers through conversion,
    /// so the markdown redaction masks its body (a layout pass that re-flowed lines once broke the
    /// marker and leaked the body).
    #[test]
    fn a_private_key_in_a_long_paragraph_is_masked_by_the_markdown_redaction() {
        let md = to_markdown("handover.docx", &fixture("pem.docx")).expect("converts");
        let begin = ["-----BEGIN", " PRIVATE KEY-----"].concat();
        assert!(md.contains(&begin), "marker on one line: {md}");
        let body = "QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo";
        assert!(md.contains(body), "{md}");
        let red = crate::redact::redact_source("handover.docx.md", &md);
        assert!(!red.contains(body), "key body masked: {red}");
        assert!(red.contains("pemcanary_body"), "prose kept: {red}");
    }

    #[test]
    fn delimiter_escapes_are_removed_and_block_escapes_kept() {
        assert_eq!(
            unescape_delimiters(r"call settle\_invoice \*now\* \`x\` \~y"),
            "call settle_invoice *now* `x` ~y"
        );
        // A protected `#` stays protected: the paragraph does not become a heading.
        assert_eq!(
            unescape_delimiters(r"\# not a heading"),
            r"\# not a heading"
        );
        assert_eq!(unescape_delimiters("no escapes here"), "no escapes here");
    }

    #[test]
    fn a_fifo_named_like_a_document_is_refused_without_blocking() {
        #[cfg(unix)]
        {
            let dir = std::env::temp_dir().join(format!("doc-convert-fifo-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let fifo = dir.join("pipe.pdf");
            let made = std::process::Command::new("mkfifo").arg(&fifo).status();
            if made.is_ok_and(|s| s.success()) {
                assert_eq!(
                    read_capped(&fifo),
                    Err(Skipped::Unreadable("not a regular file"))
                );
            }
            std::fs::remove_dir_all(&dir).ok();
        }
    }

    #[test]
    fn skip_reasons_never_carry_document_text() {
        let err = to_markdown("x.docx", b"PK\x03\x04 SECRETPAYLOAD").expect_err("garbage");
        assert!(!err.to_string().contains("SECRETPAYLOAD"), "{err}");
    }

    /// #556 R2 F1: the child is told its deadline, so it can end itself if its parent is gone.
    #[test]
    fn the_child_is_given_its_own_deadline_556() {
        let cmd = child_command(
            std::path::Path::new("wicked-estate"),
            "docx",
            true,
            Duration::from_millis(250),
        );
        let args: Vec<_> = cmd.get_args().map(|a| a.to_str().unwrap()).collect();
        assert_eq!(args, [CHILD_ARG, "docx", "deadline=250", MASK_ARG]);
    }

    /// #556 R2 F4: the converter reads bytes already in memory, so its I/O error is a corrupt
    /// document, not a file that could not be read.
    #[test]
    fn an_in_document_read_error_is_malformed_556() {
        let e = anydoc::ConvertError::Io(std::io::ErrorKind::UnexpectedEof.into());
        assert_eq!(skipped_for(e).kind(), "malformed");
    }

    #[test]
    fn every_refusal_has_a_kind_556() {
        // Every reason the child may send back names a kind; only the missing converter is a
        // plain failure, like a panic.
        for &reason in UNREADABLE_REASONS {
            let kind = Skipped::Unreadable(reason).kind();
            assert_eq!(
                kind == "failed",
                reason == UNAVAILABLE_REASON,
                "{reason:?} -> {kind}"
            );
        }
        assert_eq!(Skipped::Unreadable(DEADLINE_REASON).kind(), "deadline");
        assert_eq!(Skipped::Unreadable(MEMORY_REASON).kind(), "over_budget");
        assert_eq!(Skipped::Unreadable("no text").kind(), "empty");
        assert_eq!(
            Skipped::NeedsOcr("all 1 pages need OCR".into()).kind(),
            "needs_ocr"
        );
        assert_eq!(Skipped::TooLarge(9).kind(), "too_large");
        assert_eq!(Skipped::Encrypted.kind(), "encrypted");
        assert_eq!(Skipped::Panicked.kind(), "failed");
    }

    #[test]
    fn a_lower_read_cap_refuses_before_reading_556() {
        // The fixture itself: read_capped_at only reads it.
        let path = std::path::PathBuf::from(format!(
            "{}/tests/fixtures/docs/spec.docx",
            env!("CARGO_MANIFEST_DIR")
        ));
        let len = fixture("spec.docx").len();
        assert_eq!(
            read_capped_at(&path, len - 1),
            Err(Skipped::TooLarge(len as u64))
        );
        assert_eq!(read_capped_at(&path, len).unwrap().len(), len);
    }

    #[test]
    fn ocr_page_ranges_survive_the_wire_556() {
        for pages in ["pages 2-3 of 4 need OCR", "pages 1, 3-4 of 6 need OCR"] {
            let reply = Err(Skipped::NeedsOcr(pages.to_string()));
            assert_eq!(decode_reply(encode_reply(&reply).as_bytes()), reply);
        }
    }

    /// An infallible allocation the budget refuses aborts the child before it replies; the mark it
    /// left on stderr makes that the budget, while an abort with no mark stays a crash.
    #[cfg(unix)]
    #[test]
    fn a_budget_abort_is_the_budget_and_a_bare_abort_a_crash_556() {
        let mark = std::str::from_utf8(BUDGET_MARK).unwrap().trim_end();
        let mut budget = std::process::Command::new("sh");
        budget.args([
            "-c",
            &format!("cat >/dev/null; echo '{mark}' >&2; kill -6 $$"),
        ]);
        assert_eq!(
            supervise(budget, b"x", Duration::from_secs(10)),
            Err(Skipped::Unreadable(MEMORY_REASON))
        );
        let mut crash = std::process::Command::new("sh");
        crash.args([
            "-c",
            "cat >/dev/null; echo 'thread panicked' >&2; kill -6 $$",
        ]);
        assert_eq!(
            supervise(crash, b"x", Duration::from_secs(10)),
            Err(Skipped::Panicked)
        );
    }

    /// The path check can race a swap to a FIFO; the open itself must neither block nor accept it.
    #[test]
    fn the_opened_descriptor_must_be_a_regular_file_556() {
        #[cfg(unix)]
        {
            let dir = tempdir_for("open-regular");
            let fifo = dir.join("pipe.pdf");
            let made = std::process::Command::new("mkfifo").arg(&fifo).status();
            if made.is_ok_and(|s| s.success()) {
                let (tx, rx) = std::sync::mpsc::channel();
                let path = fifo.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(open_regular(&path).map(|(_, len)| len));
                });
                let opened = rx
                    .recv_timeout(Duration::from_secs(10))
                    .expect("opening a FIFO blocked");
                assert_eq!(opened, Err(Skipped::Unreadable("not a regular file")));
            }
            std::fs::remove_dir_all(&dir).ok();
        }
    }

    /// The size check comes before the open: an unreadable file over the cap is too large, not
    /// unreadable. And a cap above the converter's own is clamped to it (a sparse file one byte
    /// over [`MAX_INPUT_BYTES`] takes no disk).
    #[test]
    fn the_cap_is_checked_before_opening_and_clamped_556() {
        let dir = tempdir_for("cap");
        let big = dir.join("big.pdf");
        let f = std::fs::File::create(&big).unwrap();
        f.set_len(MAX_INPUT_BYTES as u64 + 1).unwrap();
        drop(f);
        assert_eq!(
            read_capped_at(&big, usize::MAX),
            Err(Skipped::TooLarge(MAX_INPUT_BYTES as u64 + 1))
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let locked = dir.join("locked.docx");
            std::fs::write(&locked, b"0123456789").unwrap();
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
            assert_eq!(read_capped_at(&locked, 4), Err(Skipped::TooLarge(10)));
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    /// `convert` masks as `knowledge.ingest` does: the Markdown rules, then the free-text rules.
    #[test]
    fn mask_markdown_masks_secret_assignments_like_knowledge_556() {
        // ggignore: made-up secret values for the masking test
        let md = "DB_PASSWORD=hunter2-example\n\npassword: marmoset\n\nAuthorization: Bearer innocentlookingsecret\n\nAPI_KEY=\"shortsecret\"\n";
        let masked = mask_markdown(md);
        assert_eq!(masked.len(), md.len());
        for secret in [
            "hunter2-example",
            "marmoset",
            "innocentlookingsecret",
            "shortsecret",
        ] {
            assert!(!masked.contains(secret), "{secret}: {masked}");
        }
        let knowledge = wicked_estate_core::redact::redact_free_text(
            &crate::redact::redact_source("doc.pdf.md", md),
        )
        .into_owned();
        assert_eq!(masked, knowledge);
    }

    fn tempdir_for(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("doc-convert-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn csv_and_markdown_are_not_documents() {
        assert!(is_doc_path("a/b/Spec.DOCX"));
        assert!(is_doc_path("r.pdf"));
        assert!(!is_doc_path("data.csv"));
        assert!(!is_doc_path("README.md"));
        assert!(!is_doc_path("pdf"));
    }

    /// Hosted OCR must be unreachable: the converter's whole dependency closure, read from the
    /// workspace lockfile, carries no HTTP, TLS or async-network client. A new anydoc (or a
    /// transitive bump) that pulls one in fails here before it can ship.
    #[test]
    fn anydoc_dependency_closure_has_no_network_client() {
        let lock =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock"))
                .expect("workspace Cargo.lock");
        let lock: toml::Value = toml::from_str(&lock).expect("lockfile parses");
        let pkgs = lock["package"].as_array().expect("packages");
        let deps_of = |name: &str| -> Vec<String> {
            pkgs.iter()
                .filter(|p| p["name"].as_str() == Some(name))
                .flat_map(|p| p.get("dependencies").and_then(|d| d.as_array()).cloned())
                .flatten()
                .filter_map(|d| {
                    d.as_str()
                        .map(|s| s.split(' ').next().unwrap_or(s).to_string())
                })
                .collect()
        };
        let mut seen = std::collections::BTreeSet::new();
        let mut todo = vec!["anydoc".to_string()];
        while let Some(n) = todo.pop() {
            if seen.insert(n.clone()) {
                todo.extend(deps_of(&n));
            }
        }
        assert!(seen.contains("pdf-inspector"), "closure walked: {seen:?}");
        let banned = [
            "reqwest",
            "hyper",
            "ureq",
            "curl",
            "isahc",
            "attohttpc",
            "rustls",
            "native-tls",
            "openssl",
            "tokio",
            "h2",
            "surf",
        ];
        let hit: Vec<&str> = banned
            .iter()
            .copied()
            .filter(|b| seen.contains(*b))
            .collect();
        assert!(hit.is_empty(), "anydoc pulls in a network client: {hit:?}");
    }
}
