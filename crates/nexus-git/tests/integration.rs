//! End-to-end integration tests for nexus-git.
//!
//! Creates real git repos with git2 and exercises every GitEngine method.

use std::fs;
use std::path::Path;

use nexus_git::{DiffLineKind, FileStatus, GitEngine, RepoState};

/// Create a temp dir, init a git repo, configure user, and return engine.
fn setup() -> (tempfile::TempDir, GitEngine) {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();

    let mut config = repo.config().unwrap();
    config.set_str("user.name", "Integration Test").unwrap();
    config.set_str("user.email", "test@nexus.dev").unwrap();
    drop(config);
    drop(repo);

    let engine = GitEngine::open(dir.path()).unwrap();
    (dir, engine)
}

fn commit(dir: &Path, message: &str) {
    let repo = git2::Repository::open(dir).unwrap();
    let mut index = repo.index().unwrap();
    index
        .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let sig = repo.signature().unwrap();

    let parents: Vec<git2::Commit<'_>> = match repo.head() {
        Ok(head) => vec![head.peel_to_commit().unwrap()],
        Err(_) => vec![],
    };
    let parent_refs: Vec<&git2::Commit<'_>> = parents.iter().collect();

    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)
        .unwrap();
}

#[test]
fn full_lifecycle() {
    let (dir, engine) = setup();

    // ── Empty repo ───────────────────────────────────────────────────────────
    let state = engine.state().unwrap();
    assert_eq!(state.head_oid, "(none)");
    assert_eq!(state.repo_state, RepoState::Clean);
    assert!(engine.log(10).unwrap().is_empty());

    // ── First commit ─────────────────────────────────────────────────────────
    fs::write(dir.path().join("README.md"), "# Nexus\n\nA test project.\n").unwrap();
    fs::write(dir.path().join("notes.md"), "# Notes\n\nSome notes.\n").unwrap();
    commit(dir.path(), "initial commit");

    let state = engine.state().unwrap();
    assert!(state.branch.is_some());
    assert_ne!(state.head_oid, "(none)");
    assert!(!state.is_dirty);

    // ── Untracked file ───────────────────────────────────────────────────────
    fs::write(dir.path().join("new.txt"), "untracked").unwrap();
    let statuses = engine.file_statuses().unwrap();
    assert!(
        statuses.iter().any(|s| s.status == FileStatus::Untracked),
        "expected untracked file"
    );
    assert!(engine.state().unwrap().is_dirty);

    // ── Commit the new file ──────────────────────────────────────────────────
    commit(dir.path(), "add new.txt");
    assert!(!engine.state().unwrap().is_dirty);

    // ── Modify and diff ──────────────────────────────────────────────────────
    fs::write(dir.path().join("README.md"), "# Nexus\n\nUpdated content.\n").unwrap();
    let statuses = engine.file_statuses().unwrap();
    assert!(
        statuses
            .iter()
            .any(|s| s.path.to_string_lossy() == "README.md" && s.status == FileStatus::Modified),
        "expected README.md modified"
    );

    let hunks = engine.diff_file(Path::new("README.md")).unwrap();
    assert!(!hunks.is_empty(), "expected diff hunks");
    let has_added = hunks
        .iter()
        .any(|h| h.lines.iter().any(|l| l.kind == DiffLineKind::Added));
    assert!(has_added, "expected added lines in diff");

    // ── Commit modification ──────────────────────────────────────────────────
    commit(dir.path(), "update README");

    // ── Log ──────────────────────────────────────────────────────────────────
    let log = engine.log(10).unwrap();
    assert_eq!(log.len(), 3);
    assert!(log[0].message.contains("update README"));
    assert!(log[2].message.contains("initial commit"));

    // ── Log for specific file ────────────────────────────────────────────────
    let readme_log = engine.log_file(Path::new("README.md"), 10).unwrap();
    assert_eq!(readme_log.len(), 2, "README.md changed in 2 commits");

    let notes_log = engine.log_file(Path::new("notes.md"), 10).unwrap();
    assert_eq!(notes_log.len(), 1, "notes.md changed in 1 commit");

    // ── Blame ────────────────────────────────────────────────────────────────
    let blame = engine.blame(Path::new("README.md")).unwrap();
    assert!(!blame.is_empty());
    assert_eq!(blame[0].author, "Integration Test");

    // ── File status for single file ──────────────────────────────────────────
    let status = engine.file_status(Path::new("README.md")).unwrap();
    assert_eq!(status, FileStatus::Unmodified);

    fs::write(dir.path().join("README.md"), "changed again").unwrap();
    let status = engine.file_status(Path::new("README.md")).unwrap();
    assert_eq!(status, FileStatus::Modified);
}

#[test]
fn open_non_repo_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(GitEngine::open(dir.path()).is_err());
}

#[test]
fn log_limit_respected() {
    let (dir, engine) = setup();
    for i in 0..10 {
        fs::write(dir.path().join(format!("file{i}.txt")), format!("content {i}")).unwrap();
        commit(dir.path(), &format!("commit {i}"));
    }
    let log = engine.log(5).unwrap();
    assert_eq!(log.len(), 5);
}
