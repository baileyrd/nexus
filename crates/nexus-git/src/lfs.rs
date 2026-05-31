//! BL-091 follow-up — Git-LFS staging-path routing.
//!
//! The BL-091 phase A closure landed the LFS *read* path in
//! `nexus-storage::lfs` (pointer detection + `git lfs smudge`
//! passthrough). The write path was deferred — `engine.stage_file`
//! routed every file through libgit2's `index.add_path`, which
//! happily wrote the raw working-tree bytes (the 50 MB PNG, not the
//! 134-byte pointer) into the index for any LFS-tracked attachment.
//!
//! This module closes that gap. Two helpers, both shell-outs to the
//! git CLI to match the read-side posture:
//!
//! - [`is_lfs_tracked`] — ask `git check-attr filter -- <path>`
//!   whether a file matches an LFS attribute. Returns `false` if
//!   git isn't on `PATH` (degrade-gracefully — BL-091 already opts
//!   into "fall through to non-LFS behaviour" when git tooling is
//!   missing).
//! - [`stage_via_git_cli`] — run `git add -- <path>` from the repo
//!   root so git's own gitattributes filter pipeline runs `git lfs
//!   clean` before the index is written. Returns an error if the
//!   subprocess fails or git isn't on `PATH`; the caller surfaces
//!   that rather than silently staging raw bytes (the BL-091 bug
//!   we're fixing).
//!
//! Why shell out instead of `index.add_frombuffer`? The git-CLI
//! pipeline already knows how to apply every filter the user has
//! configured (LFS, eol normalization, custom filters from
//! `.gitattributes`) and how to interact with `~/.gitconfig`'s
//! filter definitions. Re-implementing that against libgit2 is a
//! much larger surface than the shell-out to `clean` we'd otherwise
//! reduce to.

use std::path::Path;
use std::process::{Command, Stdio};

use crate::error::GitError;

/// True if `path` (repo-relative) is configured as an LFS-tracked
/// file under `cwd`'s `.gitattributes`. Implemented by shelling out
/// to `git check-attr filter -- <path>` and looking for a `lfs`
/// attribute on the response.
///
/// Returns `false` ("not tracked") when:
/// - the git CLI isn't on `PATH`,
/// - the subprocess exits non-zero (e.g. `cwd` isn't a git repo),
/// - the response doesn't contain a `filter: lfs` line.
///
/// Matches the BL-091 read-path's degrade-gracefully posture: a
/// missing git binary means we can't tell, so the caller falls
/// through to the non-LFS path.
///
/// Spec: [git-check-attr(1)] prints lines of the form
/// `<path>: filter: <attr>`. For an LFS file the attr is `lfs`;
/// `unspecified` / `unset` / a different filter (`crlf`, etc.)
/// don't trigger LFS routing.
///
/// [git-check-attr(1)]: https://git-scm.com/docs/git-check-attr
#[must_use]
pub fn is_lfs_tracked(cwd: &Path, path: &Path) -> bool {
    let output = Command::new("git")
        .args(["check-attr", "filter", "--"])
        .arg(path)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();
    let Ok(output) = output else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines().any(|line| {
        // Successful-output shape is "<path>: filter: lfs\n" when
        // tracked; "<path>: filter: unspecified\n" otherwise. We
        // only light up on an explicit `lfs` filter to avoid false
        // positives from custom filters that happen to be
        // configured.
        line.split(": ")
            .nth(2)
            .is_some_and(|filter| filter.trim() == "lfs")
    })
}

/// Stage `path` (repo-relative) via the git CLI so its
/// `.gitattributes` filters (notably LFS's `clean`) run before the
/// index is written. Returns a `GitError::Io` on subprocess failure
/// or non-zero exit so the caller can surface the error rather
/// than fall through to the libgit2 path that would write the raw
/// bytes (the BL-091 bug we're fixing).
///
/// # Errors
///
/// Returns [`GitError::Io`] if git isn't on `PATH`, the subprocess
/// fails to spawn, or `git add` exits non-zero. The stderr is
/// captured and surfaced in the error message.
pub fn stage_via_git_cli(repo_root: &Path, path: &Path) -> Result<(), GitError> {
    let output = Command::new("git")
        .args(["add", "--"])
        .arg(path)
        .current_dir(repo_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|err| GitError::Io(format!("BL-091: spawn `git add` failed: {err}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(GitError::Io(format!(
            "BL-091: `git add -- {}` exited {}: {}",
            path.display(),
            output.status,
            stderr.trim(),
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Build a fresh git repo at `dir` with optional `.gitattributes`
    /// content and an initial commit so subsequent commands have a
    /// HEAD to reason against.
    fn init_repo(dir: &Path, gitattributes: Option<&str>) {
        let status = Command::new("git")
            .arg("init")
            .arg("-q")
            .current_dir(dir)
            .status()
            .expect("git init");
        assert!(status.success());
        // The repo needs a user identity so commits don't fail in
        // CI; conservative fallback values.
        for (key, val) in [("user.email", "test@example.com"), ("user.name", "test")] {
            let s = Command::new("git")
                .args(["config", key, val])
                .current_dir(dir)
                .status()
                .expect("git config");
            assert!(s.success());
        }
        if let Some(text) = gitattributes {
            fs::write(dir.join(".gitattributes"), text).expect("write gitattributes");
        }
    }

    fn git_available() -> bool {
        Command::new("git")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[test]
    fn is_lfs_tracked_detects_filter_directive() {
        if !git_available() {
            return;
        }
        let tmp = TempDir::new().expect("tmp");
        init_repo(
            tmp.path(),
            Some("*.png filter=lfs diff=lfs merge=lfs -text\n"),
        );
        fs::write(tmp.path().join("a.png"), b"fake png").expect("write");
        assert!(is_lfs_tracked(tmp.path(), Path::new("a.png")));
    }

    #[test]
    fn is_lfs_tracked_returns_false_for_uncovered_path() {
        if !git_available() {
            return;
        }
        let tmp = TempDir::new().expect("tmp");
        init_repo(
            tmp.path(),
            Some("*.png filter=lfs diff=lfs merge=lfs -text\n"),
        );
        fs::write(tmp.path().join("README.md"), b"# hi").expect("write");
        assert!(!is_lfs_tracked(tmp.path(), Path::new("README.md")));
    }

    #[test]
    fn is_lfs_tracked_returns_false_when_no_gitattributes() {
        if !git_available() {
            return;
        }
        let tmp = TempDir::new().expect("tmp");
        init_repo(tmp.path(), None);
        fs::write(tmp.path().join("a.png"), b"fake png").expect("write");
        assert!(!is_lfs_tracked(tmp.path(), Path::new("a.png")));
    }

    #[test]
    fn is_lfs_tracked_ignores_non_lfs_filters() {
        if !git_available() {
            return;
        }
        let tmp = TempDir::new().expect("tmp");
        // A custom filter — not lfs.
        init_repo(tmp.path(), Some("*.txt filter=customfilter\n"));
        fs::write(tmp.path().join("note.txt"), b"hi").expect("write");
        assert!(!is_lfs_tracked(tmp.path(), Path::new("note.txt")));
    }

    #[test]
    fn is_lfs_tracked_returns_false_outside_a_repo() {
        if !git_available() {
            return;
        }
        let tmp = TempDir::new().expect("tmp");
        // No `git init` — `git check-attr` exits non-zero.
        fs::write(tmp.path().join("x.png"), b"hi").expect("write");
        assert!(!is_lfs_tracked(tmp.path(), Path::new("x.png")));
    }

    #[test]
    fn stage_via_git_cli_stages_a_normal_file() {
        if !git_available() {
            return;
        }
        let tmp = TempDir::new().expect("tmp");
        init_repo(tmp.path(), None);
        fs::write(tmp.path().join("a.txt"), b"hello").expect("write");
        stage_via_git_cli(tmp.path(), Path::new("a.txt")).expect("stage");
        // `git diff --cached --name-only` should show the staged file.
        let out = Command::new("git")
            .args(["diff", "--cached", "--name-only"])
            .current_dir(tmp.path())
            .output()
            .expect("git diff");
        assert!(out.status.success());
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            text.lines().any(|l| l == "a.txt"),
            "expected a.txt staged; got: {text:?}"
        );
    }

    #[test]
    fn stage_via_git_cli_returns_error_for_nonexistent_path() {
        if !git_available() {
            return;
        }
        let tmp = TempDir::new().expect("tmp");
        init_repo(tmp.path(), None);
        // No file written — `git add` exits non-zero.
        let err = stage_via_git_cli(tmp.path(), Path::new("ghost.txt")).unwrap_err();
        match err {
            GitError::Io(msg) => assert!(
                msg.contains("BL-091"),
                "error should be tagged with BL-091; got: {msg}",
            ),
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
