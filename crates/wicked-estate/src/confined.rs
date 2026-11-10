//! Confined reads (wicked-estate#405, DES-S7 §5 "Reads are confined").
//!
//! Every estate read of repository content resolves inside the repository root: the root is
//! canonicalized once, and a path is read only when its canonical form (symlinks resolved, `..`
//! normalized) is still under it. Absolute paths outside the root, `~` paths, `..` escapes and
//! symlinks that point outside are refused, never read.

use std::path::{Path, PathBuf};

/// A canonical repository root that reads are confined to.
#[derive(Debug, Clone)]
pub struct ConfinedRoot {
    root: PathBuf,
}

impl ConfinedRoot {
    /// Canonicalize `root` once. `None` when it does not exist.
    pub fn new(root: &Path) -> Option<Self> {
        root.canonicalize().ok().map(|root| Self { root })
    }

    /// The canonical path for `p` AS A WALK PRODUCED IT (absolute, or relative to the current
    /// directory exactly like the root the walk started from) when it resolves inside the root;
    /// `None` otherwise (including when it does not exist).
    pub fn resolve(&self, p: &Path) -> Option<PathBuf> {
        if p.to_string_lossy().starts_with('~') {
            return None;
        }
        let canon = p.canonicalize().ok()?;
        canon.starts_with(&self.root).then_some(canon)
    }

    /// The canonical path for a REPO-RELATIVE path (a document named inside an evidence envelope)
    /// when it resolves inside the root. Absolute and `~` paths are refused outright.
    pub fn resolve_rel(&self, rel: &Path) -> Option<PathBuf> {
        if rel.is_absolute() || rel.to_string_lossy().starts_with('~') {
            return None;
        }
        self.resolve(&self.root.join(rel))
    }

    /// Read the walk-produced `p` (see [`resolve`](Self::resolve)) when it resolves inside the
    /// root.
    pub fn read(&self, p: &Path) -> Option<Vec<u8>> {
        std::fs::read(self.resolve(p)?).ok()
    }

    /// Read the repo-relative `rel` as UTF-8 text when it resolves inside the root.
    pub fn read_rel_to_string(&self, rel: &Path) -> Option<String> {
        std::fs::read_to_string(self.resolve_rel(rel)?).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_everything_that_leaves_the_root() {
        let base = std::env::temp_dir().join(format!("confined-{}", std::process::id()));
        let repo = base.join("repo");
        let outside = base.join("outside");
        std::fs::create_dir_all(repo.join("sub")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(repo.join("sub/in.txt"), "in").unwrap();
        std::fs::write(outside.join("canary.txt"), "CANARY").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.join("canary.txt"), repo.join("link.txt")).unwrap();
        let c = ConfinedRoot::new(&repo).unwrap();
        assert_eq!(
            c.read_rel_to_string(Path::new("sub/in.txt")).as_deref(),
            Some("in")
        );
        assert_eq!(
            c.read(&repo.join("sub/in.txt")).as_deref(),
            Some(&b"in"[..])
        );
        assert!(
            c.read_rel_to_string(Path::new("../outside/canary.txt"))
                .is_none(),
            "dot-dot"
        );
        assert!(
            c.read_rel_to_string(&outside.join("canary.txt")).is_none(),
            "absolute"
        );
        assert!(
            c.read(&outside.join("canary.txt")).is_none(),
            "walked path outside"
        );
        assert!(
            c.read_rel_to_string(Path::new("~/.kube/config")).is_none(),
            "home path"
        );
        #[cfg(unix)]
        {
            assert!(
                c.read_rel_to_string(Path::new("link.txt")).is_none(),
                "symlink out (rel)"
            );
            assert!(
                c.read(&repo.join("link.txt")).is_none(),
                "symlink out (walked)"
            );
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A root given relative to the current directory (`index some/repo`): the walk yields
    /// `some/repo/src/a.rs`, which must resolve, not be joined onto the root a second time.
    #[test]
    fn a_relative_root_reads_its_walked_paths() {
        let base = std::env::temp_dir().join(format!("confined-rel-{}", std::process::id()));
        std::fs::create_dir_all(base.join("some/repo/src")).unwrap();
        std::fs::write(base.join("some/repo/src/a.rs"), "fn a() {}").unwrap();
        let cwd = std::env::current_dir().unwrap();
        let rel_root = pathdiff(&base.join("some/repo"), &cwd);
        let c = ConfinedRoot::new(&rel_root).unwrap();
        assert_eq!(
            c.read(&rel_root.join("src/a.rs")).as_deref(),
            Some(&b"fn a() {}"[..])
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// `target` relative to `from` (both absolute), with `..` hops.
    fn pathdiff(target: &Path, from: &Path) -> PathBuf {
        let t: Vec<_> = target.components().collect();
        let f: Vec<_> = from.components().collect();
        let common = t.iter().zip(&f).take_while(|(a, b)| a == b).count();
        let mut out = PathBuf::new();
        for _ in common..f.len() {
            out.push("..");
        }
        for c in &t[common..] {
            out.push(c);
        }
        out
    }
}
