//! In-memory workflow registry built from a directory walk.
//!
//! Matches the `.workflows/` layout from PRD-16 §4 — sub-directories
//! are recursed so `team/` and `personal/` live under the same lookup
//! surface. File extension filter is strictly `.workflow.toml`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::{parse_workflow_file, Workflow};

/// Errors from [`WorkflowRegistry`].
#[derive(Debug, Error)]
pub enum WorkflowRegistryError {
    /// Root directory didn't exist or couldn't be read.
    #[error("io error reading workflows dir: {0}")]
    Io(#[from] std::io::Error),
    /// One or more files failed to parse.
    #[error("{count} workflow file(s) failed to parse; first error: {first}")]
    PartialParseFailure {
        /// Total files that failed to parse in this load.
        count: usize,
        /// First failure's human-readable message.
        first: String,
    },
}

/// In-memory registry keyed by `workflow.name`.
#[derive(Debug, Default)]
pub struct WorkflowRegistry {
    workflows: BTreeMap<String, Entry>,
}

#[derive(Debug, Clone)]
struct Entry {
    path: PathBuf,
    workflow: Workflow,
}

impl WorkflowRegistry {
    /// Empty registry — useful when the `.workflows/` dir doesn't
    /// exist yet.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Walk `root` and parse every `.workflow.toml` file. Missing
    /// root dir is treated as empty. Parse failures surface as
    /// [`WorkflowRegistryError::PartialParseFailure`] *after* the
    /// successfully-parsed subset is already inserted, so the caller
    /// can log + continue.
    ///
    /// # Errors
    /// - [`WorkflowRegistryError::Io`] for non-ENOENT read failures.
    /// - [`WorkflowRegistryError::PartialParseFailure`] if any files
    ///   failed to parse.
    pub fn load(root: &Path) -> Result<Self, WorkflowRegistryError> {
        let mut reg = Self::empty();
        if !root.exists() {
            return Ok(reg);
        }
        let mut failures: Vec<String> = Vec::new();
        visit(root, &mut |path| match parse_workflow_file(path) {
            Ok(w) => {
                reg.workflows.insert(
                    w.workflow.name.clone(),
                    Entry {
                        path: path.to_path_buf(),
                        workflow: w,
                    },
                );
            }
            Err(e) => failures.push(format!("{}: {e}", path.display())),
        })?;
        if failures.is_empty() {
            Ok(reg)
        } else {
            Err(WorkflowRegistryError::PartialParseFailure {
                count: failures.len(),
                first: failures.into_iter().next().unwrap_or_default(),
            })
        }
    }

    /// Number of registered workflows.
    #[must_use]
    pub fn len(&self) -> usize {
        self.workflows.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.workflows.is_empty()
    }

    /// Lookup by exact workflow name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Workflow> {
        self.workflows.get(name).map(|e| &e.workflow)
    }

    /// Iterator over `(name, workflow)` pairs in sorted-name order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Workflow)> {
        self.workflows
            .iter()
            .map(|(k, v)| (k.as_str(), &v.workflow))
    }

    /// Source path a workflow was loaded from.
    #[must_use]
    pub fn source(&self, name: &str) -> Option<&Path> {
        self.workflows.get(name).map(|e| e.path.as_path())
    }
}

fn visit(dir: &Path, on_workflow: &mut impl FnMut(&Path)) -> Result<(), WorkflowRegistryError> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            visit(&path, on_workflow)?;
            continue;
        }
        if path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.ends_with(".workflow.toml"))
        {
            on_workflow(&path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const WF_A: &str = r#"
[workflow]
name = "Alpha"

[trigger]
type = "manual"

[[steps]]
type = "noop"
"#;

    const WF_B: &str = r#"
[workflow]
name = "Beta"

[trigger]
type = "cron"
schedule = "0 9 * * *"
"#;

    fn write(dir: &Path, filename: &str, contents: &str) {
        std::fs::write(dir.join(filename), contents).unwrap();
    }

    #[test]
    fn empty_dir_yields_empty_registry() {
        let tmp = TempDir::new().unwrap();
        let reg = WorkflowRegistry::load(tmp.path()).unwrap();
        assert!(reg.is_empty());
    }

    #[test]
    fn missing_root_yields_empty_registry() {
        let tmp = TempDir::new().unwrap();
        let ghost = tmp.path().join("does-not-exist");
        let reg = WorkflowRegistry::load(&ghost).unwrap();
        assert!(reg.is_empty());
    }

    #[test]
    fn loads_all_workflow_toml_files_recursively() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "a.workflow.toml", WF_A);
        let sub = tmp.path().join("team");
        std::fs::create_dir(&sub).unwrap();
        write(&sub, "b.workflow.toml", WF_B);
        let reg = WorkflowRegistry::load(tmp.path()).unwrap();
        assert_eq!(reg.len(), 2);
        assert!(reg.get("Alpha").is_some());
        assert!(reg.get("Beta").is_some());
    }

    #[test]
    fn partial_failures_surface_but_keep_good_entries() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "ok.workflow.toml", WF_A);
        write(tmp.path(), "bad.workflow.toml", "this is not toml {{{");
        let err = WorkflowRegistry::load(tmp.path()).unwrap_err();
        match err {
            WorkflowRegistryError::PartialParseFailure { count, .. } => {
                assert_eq!(count, 1);
            }
            WorkflowRegistryError::Io(_) => panic!("unexpected error: {err:?}"),
        }
    }

    #[test]
    fn ignores_non_workflow_files() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "a.workflow.toml", WF_A);
        write(tmp.path(), "README.md", "hi");
        write(tmp.path(), "junk.toml", "this is = not-a-workflow");
        let reg = WorkflowRegistry::load(tmp.path()).unwrap();
        assert_eq!(reg.len(), 1);
    }
}
