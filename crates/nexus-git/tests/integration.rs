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

// ── Level 2: Write Operations ────────────────────────────────────────────────

#[test]
fn staging_and_commit_workflow() {
    let (dir, engine) = setup();

    // Create and stage files.
    fs::write(dir.path().join("a.txt"), "alpha").unwrap();
    fs::write(dir.path().join("b.txt"), "beta").unwrap();

    engine.stage_file(Path::new("a.txt")).unwrap();
    let statuses = engine.file_statuses().unwrap();
    let a_status = statuses.iter().find(|s| s.path == Path::new("a.txt")).unwrap();
    assert_eq!(a_status.status, FileStatus::Added);
    // b.txt should still be untracked.
    let b_status = statuses.iter().find(|s| s.path == Path::new("b.txt")).unwrap();
    assert_eq!(b_status.status, FileStatus::Untracked);

    // Stage all remaining.
    engine.stage_all().unwrap();

    // Commit.
    let hash = engine.commit("add files").unwrap();
    assert_eq!(hash.len(), 7);
    assert!(!engine.state().unwrap().is_dirty);

    // Log shows the commit.
    let log = engine.log(10).unwrap();
    assert_eq!(log.len(), 1);
    assert!(log[0].message.contains("add files"));

    // Modify, stage, unstage, verify.
    fs::write(dir.path().join("a.txt"), "alpha updated").unwrap();
    engine.stage_file(Path::new("a.txt")).unwrap();
    assert_eq!(
        engine.file_status(Path::new("a.txt")).unwrap(),
        FileStatus::Staged,
    );
    engine.unstage_file(Path::new("a.txt")).unwrap();
    assert_eq!(
        engine.file_status(Path::new("a.txt")).unwrap(),
        FileStatus::Modified,
    );
}

#[test]
fn branch_create_switch_delete() {
    let (dir, engine) = setup();
    fs::write(dir.path().join("init.txt"), "init").unwrap();
    commit(dir.path(), "initial");

    // Create branches.
    engine.create_branch("feature-x").unwrap();
    engine.create_branch("feature-y").unwrap();

    let branches = engine.branches().unwrap();
    let names: Vec<&str> = branches.iter().map(|b| b.name.as_str()).collect();
    assert!(names.contains(&"feature-x"));
    assert!(names.contains(&"feature-y"));

    // One should be head.
    assert!(branches.iter().any(|b| b.is_head));

    // Switch to feature-x.
    engine.switch_branch("feature-x").unwrap();
    assert_eq!(engine.state().unwrap().branch.as_deref(), Some("feature-x"));

    // Create a commit on feature-x.
    fs::write(dir.path().join("feature.txt"), "feature work").unwrap();
    engine.stage_all().unwrap();
    engine.commit("feature commit").unwrap();

    // Switch back to main/master.
    let main_branch = branches.iter().find(|b| b.is_head).unwrap().name.clone();
    engine.switch_branch(&main_branch).unwrap();

    // feature.txt should not exist on main.
    assert!(
        !dir.path().join("feature.txt").exists(),
        "feature.txt should not exist on main branch"
    );

    // Delete feature-y (not current, not ahead).
    engine.delete_branch("feature-y").unwrap();
    let branches = engine.branches().unwrap();
    assert!(!branches.iter().any(|b| b.name == "feature-y"));

    // Cannot delete current branch.
    let current = engine.state().unwrap().branch.unwrap();
    assert!(engine.delete_branch(&current).is_err());
}

#[test]
fn unstage_all_reverts_index() {
    let (dir, engine) = setup();
    fs::write(dir.path().join("file.txt"), "content").unwrap();
    commit(dir.path(), "initial");

    fs::write(dir.path().join("file.txt"), "changed").unwrap();
    fs::write(dir.path().join("new.txt"), "new").unwrap();
    engine.stage_all().unwrap();

    // Both should be staged.
    let statuses = engine.file_statuses().unwrap();
    assert!(statuses.iter().any(|s| s.status == FileStatus::Staged || s.status == FileStatus::Added));

    // Unstage all.
    engine.unstage_all().unwrap();
    let statuses = engine.file_statuses().unwrap();
    // file.txt should be Modified, new.txt should be Untracked.
    let file_s = statuses.iter().find(|s| s.path == Path::new("file.txt")).unwrap();
    assert_eq!(file_s.status, FileStatus::Modified);
    let new_s = statuses.iter().find(|s| s.path == Path::new("new.txt")).unwrap();
    assert_eq!(new_s.status, FileStatus::Untracked);
}
