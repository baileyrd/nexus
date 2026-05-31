//! On-disk storage for comment threads.
//!
//! The store is stateless / cache-free: every read and write goes
//! through the JSON sidecar. Comments traffic is low-volume enough
//! that a cache would only buy us complexity and stale-read bugs.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use chrono::Utc;
use regex_lite::Regex;
use thiserror::Error;
use uuid::Uuid;

use crate::types::{Comment, CommentFile, CommentId, Thread, ThreadId};

/// Errors surfaced by [`CommentStore`].
#[derive(Debug, Error)]
pub enum CommentStoreError {
    /// `file_path` failed validation (empty, absolute, contains `..`,
    /// or otherwise outside the forge).
    #[error("invalid file path: {0}")]
    InvalidFilePath(String),
    /// I/O failure reading or writing the sidecar.
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    /// Sidecar JSON failed to parse.
    #[error("malformed sidecar at {path}: {source}")]
    Malformed {
        /// The sidecar path that failed to parse.
        path: PathBuf,
        /// Underlying serde error.
        #[source]
        source: serde_json::Error,
    },
    /// The requested thread does not exist in the file's sidecar.
    #[error("thread not found: {0}")]
    ThreadNotFound(ThreadId),
    /// The requested comment does not exist in the thread.
    #[error("comment not found: {0}")]
    CommentNotFound(CommentId),
    /// A thread cannot be left empty (deleting its last comment is
    /// rejected; callers should `delete_thread` instead).
    #[error("cannot delete the only comment in a thread; delete the thread instead")]
    LastCommentInThread,
}

/// Forge-rooted comment store. Cheap to construct; performs no I/O
/// until first call.
#[derive(Debug, Clone)]
pub struct CommentStore {
    comments_root: PathBuf,
}

impl CommentStore {
    /// Construct a store rooted at `<forge>/.forge/comments`. The
    /// directory is **not** created eagerly — first write does that.
    #[must_use]
    pub fn new(forge_root: &Path) -> Self {
        Self {
            comments_root: forge_root.join(".forge").join("comments"),
        }
    }

    /// Read the sidecar for `file_path`. Returns
    /// [`CommentFile::empty`] when the sidecar is missing — callers
    /// can treat "no comments yet" and "no file at all" identically.
    ///
    /// # Errors
    /// - [`CommentStoreError::InvalidFilePath`] for unsafe paths.
    /// - [`CommentStoreError::Io`] for read failures other than
    ///   `NotFound`.
    /// - [`CommentStoreError::Malformed`] when the sidecar is not
    ///   valid JSON matching [`CommentFile`].
    pub fn load(&self, file_path: &str) -> Result<CommentFile, CommentStoreError> {
        let normalized = normalize_relpath(file_path)?;
        let sidecar = self.sidecar_path(&normalized);
        match fs::read(&sidecar) {
            Ok(bytes) => serde_json::from_slice::<CommentFile>(&bytes).map_err(|source| {
                CommentStoreError::Malformed {
                    path: sidecar,
                    source,
                }
            }),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(CommentFile::empty(normalized)),
            Err(err) => Err(CommentStoreError::Io(err)),
        }
    }

    /// Persist a [`CommentFile`] to disk, replacing any existing
    /// sidecar. When `file.threads` is empty the sidecar is removed
    /// to avoid littering the forge with empty JSON.
    ///
    /// # Errors
    /// I/O failures bubble up unchanged.
    pub fn save(&self, file: &CommentFile) -> Result<(), CommentStoreError> {
        let normalized = normalize_relpath(&file.file_path)?;
        let sidecar = self.sidecar_path(&normalized);
        if file.threads.is_empty() {
            match fs::remove_file(&sidecar) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(CommentStoreError::Io(err)),
            }
        } else {
            if let Some(parent) = sidecar.parent() {
                fs::create_dir_all(parent)?;
            }
            let body =
                serde_json::to_vec_pretty(file).map_err(|source| CommentStoreError::Malformed {
                    path: sidecar.clone(),
                    source,
                })?;
            fs::write(&sidecar, body)?;
            Ok(())
        }
    }

    /// List threads anchored in `file_path`. Equivalent to
    /// `load(file_path).threads`; provided as a convenience for the
    /// common read path.
    ///
    /// # Errors
    /// See [`CommentStore::load`].
    pub fn list_threads(&self, file_path: &str) -> Result<Vec<Thread>, CommentStoreError> {
        Ok(self.load(file_path)?.threads)
    }

    /// Create a new thread anchored to `block_id` in `file_path`,
    /// seeded with one initial comment.
    ///
    /// # Errors
    /// Path validation + I/O failures.
    pub fn create_thread(
        &self,
        file_path: &str,
        block_id: Uuid,
        body: String,
        author: Option<String>,
    ) -> Result<Thread, CommentStoreError> {
        let mut file = self.load(file_path)?;
        let now = Utc::now();
        let comment = Comment {
            id: Uuid::new_v4(),
            author,
            mentions: extract_mentions(&body),
            body,
            created_at: now,
            updated_at: None,
        };
        let thread = Thread {
            id: Uuid::new_v4(),
            block_id,
            resolved: false,
            resolved_at: None,
            resolved_by: None,
            created_at: now,
            comments: vec![comment],
        };
        file.threads.push(thread.clone());
        self.save(&file)?;
        Ok(thread)
    }

    /// Append a reply to an existing thread.
    ///
    /// # Errors
    /// [`CommentStoreError::ThreadNotFound`] if `thread_id` is not in
    /// the file's sidecar; otherwise path / I/O errors.
    pub fn add_reply(
        &self,
        file_path: &str,
        thread_id: ThreadId,
        body: String,
        author: Option<String>,
    ) -> Result<Comment, CommentStoreError> {
        let mut file = self.load(file_path)?;
        let thread = file
            .threads
            .iter_mut()
            .find(|t| t.id == thread_id)
            .ok_or(CommentStoreError::ThreadNotFound(thread_id))?;
        let comment = Comment {
            id: Uuid::new_v4(),
            author,
            mentions: extract_mentions(&body),
            body,
            created_at: Utc::now(),
            updated_at: None,
        };
        thread.comments.push(comment.clone());
        self.save(&file)?;
        Ok(comment)
    }

    /// Toggle a thread's `resolved` flag. When transitioning from
    /// unresolved → resolved, `resolved_at` and `resolved_by` are
    /// stamped; the reverse transition clears them.
    ///
    /// # Errors
    /// [`CommentStoreError::ThreadNotFound`] if missing.
    pub fn set_resolved(
        &self,
        file_path: &str,
        thread_id: ThreadId,
        resolved: bool,
        author: Option<String>,
    ) -> Result<Thread, CommentStoreError> {
        let mut file = self.load(file_path)?;
        let thread = file
            .threads
            .iter_mut()
            .find(|t| t.id == thread_id)
            .ok_or(CommentStoreError::ThreadNotFound(thread_id))?;
        if resolved && !thread.resolved {
            thread.resolved = true;
            thread.resolved_at = Some(Utc::now());
            thread.resolved_by = author;
        } else if !resolved && thread.resolved {
            thread.resolved = false;
            thread.resolved_at = None;
            thread.resolved_by = None;
        }
        let snapshot = thread.clone();
        self.save(&file)?;
        Ok(snapshot)
    }

    /// Delete a thread outright.
    ///
    /// # Errors
    /// [`CommentStoreError::ThreadNotFound`] if missing.
    pub fn delete_thread(
        &self,
        file_path: &str,
        thread_id: ThreadId,
    ) -> Result<(), CommentStoreError> {
        let mut file = self.load(file_path)?;
        let before = file.threads.len();
        file.threads.retain(|t| t.id != thread_id);
        if file.threads.len() == before {
            return Err(CommentStoreError::ThreadNotFound(thread_id));
        }
        self.save(&file)?;
        Ok(())
    }

    /// Delete a single comment from a thread. Refuses to remove the
    /// last remaining comment in a thread — callers should call
    /// [`CommentStore::delete_thread`] for that.
    ///
    /// # Errors
    /// [`CommentStoreError::ThreadNotFound`] / [`CommentStoreError::CommentNotFound`] /
    /// [`CommentStoreError::LastCommentInThread`].
    pub fn delete_comment(
        &self,
        file_path: &str,
        thread_id: ThreadId,
        comment_id: CommentId,
    ) -> Result<(), CommentStoreError> {
        let mut file = self.load(file_path)?;
        let thread = file
            .threads
            .iter_mut()
            .find(|t| t.id == thread_id)
            .ok_or(CommentStoreError::ThreadNotFound(thread_id))?;
        if thread.comments.len() == 1 && thread.comments[0].id == comment_id {
            return Err(CommentStoreError::LastCommentInThread);
        }
        let before = thread.comments.len();
        thread.comments.retain(|c| c.id != comment_id);
        if thread.comments.len() == before {
            return Err(CommentStoreError::CommentNotFound(comment_id));
        }
        self.save(&file)?;
        Ok(())
    }

    /// Edit a comment's body in place; updates `updated_at` and
    /// re-extracts mentions. Author / id are immutable.
    ///
    /// # Errors
    /// [`CommentStoreError::ThreadNotFound`] / [`CommentStoreError::CommentNotFound`].
    pub fn edit_comment(
        &self,
        file_path: &str,
        thread_id: ThreadId,
        comment_id: CommentId,
        body: String,
    ) -> Result<Comment, CommentStoreError> {
        let mut file = self.load(file_path)?;
        let thread = file
            .threads
            .iter_mut()
            .find(|t| t.id == thread_id)
            .ok_or(CommentStoreError::ThreadNotFound(thread_id))?;
        let comment = thread
            .comments
            .iter_mut()
            .find(|c| c.id == comment_id)
            .ok_or(CommentStoreError::CommentNotFound(comment_id))?;
        comment.mentions = extract_mentions(&body);
        comment.body = body;
        comment.updated_at = Some(Utc::now());
        let snapshot = comment.clone();
        self.save(&file)?;
        Ok(snapshot)
    }

    fn sidecar_path(&self, normalized_relpath: &str) -> PathBuf {
        let mut out = self.comments_root.clone();
        for segment in normalized_relpath.split('/') {
            out.push(segment);
        }
        // Append `.json` to the *file* component so two markdown
        // files can coexist with a directory of the same stem.
        let mut filename = out
            .file_name()
            .map(std::ffi::OsStr::to_os_string)
            .unwrap_or_default();
        filename.push(".json");
        out.set_file_name(filename);
        out
    }
}

fn normalize_relpath(input: &str) -> Result<String, CommentStoreError> {
    if input.is_empty() {
        return Err(CommentStoreError::InvalidFilePath(
            "path is empty".to_string(),
        ));
    }
    let path = Path::new(input);
    if path.is_absolute() {
        return Err(CommentStoreError::InvalidFilePath(format!(
            "absolute paths not allowed: {input}"
        )));
    }
    let mut out = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(seg) => {
                let s = seg
                    .to_str()
                    .ok_or_else(|| {
                        CommentStoreError::InvalidFilePath(format!("non-utf8 segment in {input}"))
                    })?
                    .to_string();
                if s.is_empty() {
                    continue;
                }
                out.push(s);
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(CommentStoreError::InvalidFilePath(format!(
                    "path escapes forge root: {input}"
                )));
            }
        }
    }
    if out.is_empty() {
        return Err(CommentStoreError::InvalidFilePath(format!(
            "path has no normal segments: {input}"
        )));
    }
    Ok(out.join("/"))
}

fn extract_mentions(body: &str) -> Vec<String> {
    // `@name` where name is [A-Za-z0-9_-] (1..32 chars). Keep
    // conservative: don't match inside email addresses.
    static RE_PATTERN: &str = r"(?:^|[^\w])@([A-Za-z0-9_-]{1,32})";
    let re = Regex::new(RE_PATTERN).expect("static regex compiles");
    let mut seen = Vec::new();
    for cap in re.captures_iter(body) {
        if let Some(m) = cap.get(1) {
            let name = m.as_str().to_string();
            if !seen.contains(&name) {
                seen.push(name);
            }
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn store() -> (TempDir, CommentStore) {
        let dir = TempDir::new().unwrap();
        let store = CommentStore::new(dir.path());
        (dir, store)
    }

    #[test]
    fn load_missing_returns_empty() {
        let (_d, s) = store();
        let f = s.load("notes/foo.md").unwrap();
        assert_eq!(f.file_path, "notes/foo.md");
        assert!(f.threads.is_empty());
        assert_eq!(f.version, CommentFile::VERSION);
    }

    #[test]
    fn create_then_list_roundtrip() {
        let (_d, s) = store();
        let block = Uuid::new_v4();
        let t = s
            .create_thread("foo.md", block, "first!".into(), Some("alice".into()))
            .unwrap();
        let listed = s.list_threads("foo.md").unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, t.id);
        assert_eq!(listed[0].block_id, block);
        assert_eq!(listed[0].comments.len(), 1);
        assert_eq!(listed[0].comments[0].body, "first!");
        assert_eq!(listed[0].comments[0].author.as_deref(), Some("alice"));
        assert!(!listed[0].resolved);
    }

    #[test]
    fn add_reply_appends() {
        let (_d, s) = store();
        let t = s
            .create_thread("foo.md", Uuid::new_v4(), "q?".into(), None)
            .unwrap();
        s.add_reply("foo.md", t.id, "answer".into(), Some("bob".into()))
            .unwrap();
        let listed = s.list_threads("foo.md").unwrap();
        assert_eq!(listed[0].comments.len(), 2);
        assert_eq!(listed[0].comments[1].body, "answer");
    }

    #[test]
    fn add_reply_unknown_thread_errors() {
        let (_d, s) = store();
        let err = s
            .add_reply("foo.md", Uuid::new_v4(), "x".into(), None)
            .unwrap_err();
        assert!(matches!(err, CommentStoreError::ThreadNotFound(_)));
    }

    #[test]
    fn resolve_and_unresolve_round_trip() {
        let (_d, s) = store();
        let t = s
            .create_thread("foo.md", Uuid::new_v4(), "x".into(), None)
            .unwrap();
        let resolved = s
            .set_resolved("foo.md", t.id, true, Some("carol".into()))
            .unwrap();
        assert!(resolved.resolved);
        assert!(resolved.resolved_at.is_some());
        assert_eq!(resolved.resolved_by.as_deref(), Some("carol"));

        let again = s.set_resolved("foo.md", t.id, false, None).unwrap();
        assert!(!again.resolved);
        assert!(again.resolved_at.is_none());
        assert!(again.resolved_by.is_none());
    }

    #[test]
    fn delete_thread_removes_sidecar_when_empty() {
        let (d, s) = store();
        let t = s
            .create_thread("foo.md", Uuid::new_v4(), "x".into(), None)
            .unwrap();
        let sidecar = d.path().join(".forge/comments/foo.md.json");
        assert!(sidecar.exists());
        s.delete_thread("foo.md", t.id).unwrap();
        assert!(!sidecar.exists(), "empty sidecar should be removed");
        assert!(s.list_threads("foo.md").unwrap().is_empty());
    }

    #[test]
    fn delete_thread_not_found_errors() {
        let (_d, s) = store();
        let err = s.delete_thread("foo.md", Uuid::new_v4()).unwrap_err();
        assert!(matches!(err, CommentStoreError::ThreadNotFound(_)));
    }

    #[test]
    fn delete_comment_not_last() {
        let (_d, s) = store();
        let t = s
            .create_thread("foo.md", Uuid::new_v4(), "first".into(), None)
            .unwrap();
        let reply = s.add_reply("foo.md", t.id, "second".into(), None).unwrap();
        s.delete_comment("foo.md", t.id, reply.id).unwrap();
        let listed = s.list_threads("foo.md").unwrap();
        assert_eq!(listed[0].comments.len(), 1);
        assert_eq!(listed[0].comments[0].body, "first");
    }

    #[test]
    fn delete_comment_refuses_last() {
        let (_d, s) = store();
        let t = s
            .create_thread("foo.md", Uuid::new_v4(), "only".into(), None)
            .unwrap();
        let only = t.comments[0].id;
        let err = s.delete_comment("foo.md", t.id, only).unwrap_err();
        assert!(matches!(err, CommentStoreError::LastCommentInThread));
    }

    #[test]
    fn edit_comment_updates_body_and_mentions() {
        let (_d, s) = store();
        let t = s
            .create_thread("foo.md", Uuid::new_v4(), "hi".into(), None)
            .unwrap();
        let cid = t.comments[0].id;
        let edited = s
            .edit_comment("foo.md", t.id, cid, "ping @alice".into())
            .unwrap();
        assert_eq!(edited.body, "ping @alice");
        assert!(edited.updated_at.is_some());
        assert_eq!(edited.mentions, vec!["alice".to_string()]);
    }

    #[test]
    fn nested_paths_dont_collide() {
        let (d, s) = store();
        s.create_thread("a/foo.md", Uuid::new_v4(), "1".into(), None)
            .unwrap();
        s.create_thread("b/foo.md", Uuid::new_v4(), "2".into(), None)
            .unwrap();
        assert!(d.path().join(".forge/comments/a/foo.md.json").exists());
        assert!(d.path().join(".forge/comments/b/foo.md.json").exists());
        assert_eq!(s.list_threads("a/foo.md").unwrap().len(), 1);
        assert_eq!(s.list_threads("b/foo.md").unwrap().len(), 1);
    }

    #[test]
    fn rejects_absolute_path() {
        let (_d, s) = store();
        let err = s.list_threads("/etc/passwd").unwrap_err();
        assert!(matches!(err, CommentStoreError::InvalidFilePath(_)));
    }

    #[test]
    fn rejects_parent_traversal() {
        let (_d, s) = store();
        let err = s.list_threads("../escape.md").unwrap_err();
        assert!(matches!(err, CommentStoreError::InvalidFilePath(_)));
    }

    #[test]
    fn rejects_empty_path() {
        let (_d, s) = store();
        let err = s.list_threads("").unwrap_err();
        assert!(matches!(err, CommentStoreError::InvalidFilePath(_)));
    }

    #[test]
    fn malformed_sidecar_surfaces_error() {
        let (d, s) = store();
        let p = d.path().join(".forge/comments/foo.md.json");
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, b"{ not json").unwrap();
        let err = s.load("foo.md").unwrap_err();
        assert!(matches!(err, CommentStoreError::Malformed { .. }));
    }

    #[test]
    fn extract_mentions_dedupes_and_skips_emails() {
        let mentions = extract_mentions("hi @alice and @alice and @bob; not foo@example.com");
        assert_eq!(mentions, vec!["alice".to_string(), "bob".to_string()]);
    }

    #[test]
    fn save_roundtrips_via_disk() {
        let (_d, s) = store();
        let t = s
            .create_thread("foo.md", Uuid::new_v4(), "hi".into(), None)
            .unwrap();
        // Re-load the store fresh — every read goes through disk.
        let listed = s.list_threads("foo.md").unwrap();
        assert_eq!(listed[0].id, t.id);
    }

    #[test]
    fn normalize_collapses_curdir_segments() {
        assert_eq!(normalize_relpath("./foo.md").unwrap(), "foo.md");
        assert_eq!(normalize_relpath("a/./b.md").unwrap(), "a/b.md");
    }
}
