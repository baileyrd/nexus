//! End-to-end integration tests for nexus-git.
//!
//! Creates real git repos with git2 and exercises every GitEngine method.

use std::fs;
use std::path::Path;

use nexus_git::{AutoCommitter, DiffLineKind, FileStatus, GitEngine, RepoState};

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

// ── Merge Tests ──────────────────────────────────────────────────────────────

#[test]
fn merge_fast_forward() {
    let (dir, engine) = setup();
    fs::write(dir.path().join("init.txt"), "init").unwrap();
    commit(dir.path(), "initial");

    let main = engine.state().unwrap().branch.unwrap();

    // Create feature branch, add a commit.
    engine.create_branch("feature-ff").unwrap();
    engine.switch_branch("feature-ff").unwrap();
    fs::write(dir.path().join("feature.txt"), "feature").unwrap();
    engine.stage_all().unwrap();
    engine.commit("feature work").unwrap();

    // Switch back to main and merge.
    engine.switch_branch(&main).unwrap();
    let result = engine.merge("feature-ff").unwrap();

    assert!(result.fast_forward, "should be fast-forward");
    assert!(result.conflicts.is_empty());
    assert!(result.commit_hash.is_some());
    assert!(dir.path().join("feature.txt").exists(), "feature.txt should exist after merge");
}

#[test]
fn merge_with_commit() {
    let (dir, engine) = setup();
    fs::write(dir.path().join("init.txt"), "init").unwrap();
    commit(dir.path(), "initial");

    let main = engine.state().unwrap().branch.unwrap();

    // Create feature branch and commit.
    engine.create_branch("feature-mc").unwrap();
    engine.switch_branch("feature-mc").unwrap();
    fs::write(dir.path().join("feature.txt"), "feature").unwrap();
    engine.stage_all().unwrap();
    engine.commit("feature commit").unwrap();

    // Switch back to main, make a diverging commit.
    engine.switch_branch(&main).unwrap();
    fs::write(dir.path().join("main-only.txt"), "main").unwrap();
    engine.stage_all().unwrap();
    engine.commit("main commit").unwrap();

    // Merge — should create a merge commit (not fast-forward).
    let result = engine.merge("feature-mc").unwrap();
    assert!(!result.fast_forward);
    assert!(result.conflicts.is_empty());
    assert!(result.commit_hash.is_some());

    // Both files should exist.
    assert!(dir.path().join("feature.txt").exists());
    assert!(dir.path().join("main-only.txt").exists());
}

#[test]
fn merge_with_conflicts() {
    let (dir, engine) = setup();
    fs::write(dir.path().join("shared.txt"), "original").unwrap();
    commit(dir.path(), "initial");

    let main = engine.state().unwrap().branch.unwrap();

    // Feature branch modifies shared.txt.
    engine.create_branch("feature-conflict").unwrap();
    engine.switch_branch("feature-conflict").unwrap();
    fs::write(dir.path().join("shared.txt"), "feature version").unwrap();
    engine.stage_all().unwrap();
    engine.commit("feature change").unwrap();

    // Main also modifies shared.txt.
    engine.switch_branch(&main).unwrap();
    fs::write(dir.path().join("shared.txt"), "main version").unwrap();
    engine.stage_all().unwrap();
    engine.commit("main change").unwrap();

    // Merge — should report conflicts.
    let result = engine.merge("feature-conflict").unwrap();
    assert!(!result.conflicts.is_empty(), "expected conflicts");
    assert!(
        result.conflicts.iter().any(|f| f == "shared.txt"),
        "shared.txt should be conflicted, got: {:?}",
        result.conflicts
    );
    assert!(result.commit_hash.is_none(), "should not auto-commit on conflict");

    // conflict_files should also report the conflict.
    let conflicts = engine.conflict_files().unwrap();
    assert!(!conflicts.is_empty());
}

#[test]
fn merge_abort_restores_state() {
    let (dir, engine) = setup();
    fs::write(dir.path().join("shared.txt"), "original").unwrap();
    commit(dir.path(), "initial");

    let main = engine.state().unwrap().branch.unwrap();

    // Create conflicting branches.
    engine.create_branch("feature-abort").unwrap();
    engine.switch_branch("feature-abort").unwrap();
    fs::write(dir.path().join("shared.txt"), "feature").unwrap();
    engine.stage_all().unwrap();
    engine.commit("feature").unwrap();

    engine.switch_branch(&main).unwrap();
    fs::write(dir.path().join("shared.txt"), "main").unwrap();
    engine.stage_all().unwrap();
    engine.commit("main").unwrap();

    // Start merge (produces conflicts).
    let result = engine.merge("feature-abort").unwrap();
    assert!(!result.conflicts.is_empty());

    // Abort.
    engine.abort_merge().unwrap();

    // Should be back to clean state.
    let state = engine.state().unwrap();
    assert_eq!(state.repo_state, nexus_git::RepoState::Clean);
    // shared.txt should have main's content.
    let content = fs::read_to_string(dir.path().join("shared.txt")).unwrap();
    assert_eq!(content, "main");
}

#[test]
fn push_pull_local_bare_repo() {
    // Set up a bare repo as "remote".
    let bare_dir = tempfile::tempdir().unwrap();
    git2::Repository::init_bare(bare_dir.path()).unwrap();

    // Set up a working repo and add the bare as a remote.
    let (dir, engine) = setup();
    {
        let repo = git2::Repository::open(dir.path()).unwrap();
        repo.remote("origin", &format!("file://{}", bare_dir.path().display()))
            .unwrap();
    }

    // Create a commit and push.
    fs::write(dir.path().join("file.txt"), "content").unwrap();
    engine.stage_all().unwrap();
    engine.commit("initial").unwrap();

    let main = engine.state().unwrap().branch.unwrap();
    engine.push("origin", &main).unwrap();

    // Clone into a second working copy to verify push worked.
    let clone_dir = tempfile::tempdir().unwrap();
    git2::Repository::clone(
        &format!("file://{}", bare_dir.path().display()),
        clone_dir.path(),
    )
    .unwrap();
    assert!(
        clone_dir.path().join("file.txt").exists(),
        "cloned repo should have file.txt"
    );

    // Make a commit in the clone and push.
    {
        let clone_repo = git2::Repository::open(clone_dir.path()).unwrap();
        let mut config = clone_repo.config().unwrap();
        config.set_str("user.name", "Clone User").unwrap();
        config.set_str("user.email", "clone@test.com").unwrap();
    }
    fs::write(clone_dir.path().join("new.txt"), "from clone").unwrap();
    commit(clone_dir.path(), "clone commit");
    {
        let clone_repo = git2::Repository::open(clone_dir.path()).unwrap();
        let mut remote = clone_repo.find_remote("origin").unwrap();
        remote
            .push(&[&format!("refs/heads/{main}:refs/heads/{main}")], None)
            .unwrap();
    }

    // Pull in original repo.
    let result = engine.pull("origin", &main).unwrap();
    assert!(result.conflicts.is_empty());
    assert!(
        dir.path().join("new.txt").exists(),
        "pulled file should exist"
    );
}

// ── Auto-Commit Tests ────────────────────────────────────────────────────────

#[test]
fn auto_commit_full_workflow() {
    let (dir, _engine) = setup();

    // Create an initial commit so the repo isn't empty.
    fs::write(dir.path().join("init.txt"), "init").unwrap();
    commit(dir.path(), "initial");

    let mut committer = AutoCommitter::new(dir.path(), 0);

    // Clean state — nothing to commit.
    let r1 = committer.check_and_commit().unwrap();
    assert!(r1.commit_hash.is_none());

    // Make changes — should auto-commit.
    fs::write(dir.path().join("notes.md"), "# Notes\n").unwrap();
    fs::write(dir.path().join("todo.md"), "# Todo\n").unwrap();
    let r2 = committer.check_and_commit().unwrap();
    assert!(r2.commit_hash.is_some());
    assert_eq!(r2.files_changed, 2);
    assert!(r2.message.unwrap().starts_with("auto:"));

    // Verify the commit appears in the log.
    let engine = GitEngine::open(dir.path()).unwrap();
    let log = engine.log(10).unwrap();
    assert!(log.iter().any(|e| e.message.starts_with("auto:")));

    // Working tree should be clean now.
    assert!(!engine.state().unwrap().is_dirty);
}
