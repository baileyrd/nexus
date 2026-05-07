//! BL-091 — Git-LFS pointer detection + smudge passthrough.
//!
//! Git-LFS stores large blobs (images, audio, video, datasets, model
//! weights) as ASCII *pointer* files in the working tree. The actual
//! content lives on an LFS server and is only materialised when the
//! caller has the `git-lfs` binary installed and runs `git lfs
//! smudge`. Without this module, Nexus would silently hand the raw
//! pointer text to a caller asking for the real content.
//!
//! ## What this module does
//!
//! - [`is_pointer`] — fast prefix check on the first line of a file.
//! - [`parse_pointer`] — pulls `oid` + `size` out of a valid pointer
//!   so the caller can decide what to do (e.g. report "this PNG is
//!   12 MB" without smudging).
//! - [`smudge`] — invokes `git lfs smudge` as a subprocess and
//!   returns the resolved bytes, or `None` if git-lfs is not on
//!   `PATH` / the subprocess failed. The caller falls back to
//!   pointer text + a warning event.
//!
//! ## What this module does *not* do
//!
//! The *write* path (detecting LFS-tracked patterns from
//! `.gitattributes` and routing through `git lfs clean` before
//! staging) is deferred — see BL-091 closure notes. Reads are the
//! higher-impact gap because they're the path a user hits when
//! they open an attachment.

use std::path::Path;
use std::process::{Command, Stdio};

/// First line of every Git-LFS pointer file.
pub const POINTER_VERSION_LINE: &str = "version https://git-lfs.github.com/spec/v1";

/// Decoded contents of an LFS pointer file. The pointer format is
/// stable per [the Git-LFS spec][1] — `version`, `oid sha256:<hex>`,
/// `size <bytes>` plus optional extension fields, separated by `\n`,
/// terminated by `\n`.
///
/// [1]: https://github.com/git-lfs/git-lfs/blob/main/docs/spec.md
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LfsPointer {
    /// Hex sha-256 OID of the content blob.
    pub oid: String,
    /// Size of the resolved content in bytes.
    pub size: u64,
}

/// True when `bytes` starts with an LFS pointer's version line.
/// Cheap enough to call on every read.
#[must_use]
pub fn is_pointer(bytes: &[u8]) -> bool {
    let prefix = POINTER_VERSION_LINE.as_bytes();
    if bytes.len() < prefix.len() + 1 {
        return false;
    }
    if !bytes.starts_with(prefix) {
        return false;
    }
    // The version line must terminate with `\n` (or `\r\n`, tolerated).
    let next = bytes[prefix.len()];
    next == b'\n' || next == b'\r'
}

/// Parse a candidate pointer file's bytes into an [`LfsPointer`].
/// Returns `None` if the input isn't a valid pointer (missing
/// version line, malformed `oid` / `size` keys, etc.).
#[must_use]
pub fn parse_pointer(bytes: &[u8]) -> Option<LfsPointer> {
    if !is_pointer(bytes) {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let mut oid = None;
    let mut size = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("oid sha256:") {
            oid = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("size ") {
            size = rest.trim().parse::<u64>().ok();
        }
    }
    Some(LfsPointer { oid: oid?, size: size? })
}

/// Run `git lfs smudge` against `pointer_bytes` and return the
/// resolved content. Returns `None` if:
///
/// - `git-lfs` is not on `PATH`,
/// - the subprocess exits non-zero (e.g. offline + no local cache),
/// - the working directory `cwd` isn't a git repo.
///
/// The caller is expected to fall back to the pointer text with a
/// `tracing::warn!` so operators see the degradation in logs.
#[must_use]
pub fn smudge(cwd: &Path, pointer_bytes: &[u8]) -> Option<Vec<u8>> {
    let mut child = Command::new("git")
        .args(["lfs", "smudge"])
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    use std::io::Write;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(pointer_bytes).ok()?;
        // Drop closes the pipe so smudge can finish reading.
    }
    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        tracing::warn!(
            stderr = %String::from_utf8_lossy(&output.stderr),
            "BL-091: git lfs smudge exited non-zero; falling back to pointer text",
        );
        return None;
    }
    Some(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_pointer() -> Vec<u8> {
        format!(
            "{POINTER_VERSION_LINE}\noid sha256:abc123def456\nsize 1234\n"
        )
        .into_bytes()
    }

    #[test]
    fn is_pointer_recognises_valid_header() {
        assert!(is_pointer(&sample_pointer()));
    }

    #[test]
    fn is_pointer_rejects_short_input() {
        assert!(!is_pointer(b""));
        assert!(!is_pointer(b"version https://git-lfs.github.com"));
    }

    #[test]
    fn is_pointer_rejects_unrelated_text() {
        assert!(!is_pointer(b"# Heading\n\nSome markdown content"));
        assert!(!is_pointer(b"\x89PNG\r\n\x1a\n")); // PNG magic bytes
    }

    #[test]
    fn is_pointer_rejects_substring_in_body() {
        // The version string appearing anywhere other than the start
        // of the file is not a pointer (anti-spoofing).
        let payload = format!("# Note\n\n{POINTER_VERSION_LINE}\n");
        assert!(!is_pointer(payload.as_bytes()));
    }

    #[test]
    fn parse_pointer_extracts_oid_and_size() {
        let p = parse_pointer(&sample_pointer()).expect("parse");
        assert_eq!(p.oid, "abc123def456");
        assert_eq!(p.size, 1234);
    }

    #[test]
    fn parse_pointer_returns_none_on_missing_oid() {
        let bytes = format!("{POINTER_VERSION_LINE}\nsize 1234\n");
        assert!(parse_pointer(bytes.as_bytes()).is_none());
    }

    #[test]
    fn parse_pointer_returns_none_on_garbage_size() {
        let bytes =
            format!("{POINTER_VERSION_LINE}\noid sha256:abc\nsize not-a-number\n");
        assert!(parse_pointer(bytes.as_bytes()).is_none());
    }

    #[test]
    fn parse_pointer_returns_none_on_non_pointer() {
        assert!(parse_pointer(b"# heading\n").is_none());
    }
}
