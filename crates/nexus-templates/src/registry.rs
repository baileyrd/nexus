//! In-memory index of templates discovered for a forge.
//!
//! The registry merges three sources:
//!
//! 1. The built-in seed set ([`crate::builtins`]).
//! 2. `.template.md` files in `<forge>/.forge/templates/` (recursive).
//! 3. Future: plugin-contributed templates (via IPC).
//!
//! User-defined templates with the same `name` as a built-in **override**
//! the built-in.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::builtins;
use crate::template::{parse_template_file, Template};

/// Failure loading the registry.
#[derive(Debug, thiserror::Error)]
pub enum TemplateRegistryError {
    /// I/O failure walking the templates dir.
    #[error("I/O error reading templates dir '{path}': {source}")]
    Io {
        /// Templates directory path.
        path: String,
        /// Underlying error.
        source: std::io::Error,
    },
    /// A specific `.template.md` file failed to parse.
    #[error("failed to parse template '{file}': {source}")]
    Parse {
        /// Template file path.
        file: String,
        /// Parse error.
        source: crate::template::TemplateParseError,
    },
}

/// In-memory template index.
#[derive(Debug, Clone, Default)]
pub struct TemplateRegistry {
    by_name: HashMap<String, Template>,
}

impl TemplateRegistry {
    /// Build a registry by loading built-ins, then layering user templates
    /// from `<forge_root>/.forge/templates/` on top. A user template with
    /// the same name as a built-in replaces it.
    ///
    /// # Errors
    /// Walking failures and per-file parse failures.
    pub fn load(forge_root: &Path) -> Result<Self, TemplateRegistryError> {
        let mut by_name = HashMap::new();

        // 1. Built-ins.
        for tpl in builtins::parsed() {
            by_name.insert(tpl.meta.name.clone(), tpl);
        }

        // 2. User templates.
        let user_dir = forge_root.join(".forge/templates");
        if user_dir.exists() {
            visit_templates(&user_dir, &mut |path| {
                let tpl = parse_template_file(path).map_err(|source| {
                    TemplateRegistryError::Parse {
                        file: path.display().to_string(),
                        source,
                    }
                })?;
                by_name.insert(tpl.meta.name.clone(), tpl);
                Ok(())
            })?;
        }

        Ok(Self { by_name })
    }

    /// Build an empty registry — useful for testing.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Insert a template directly. Useful for plugin-contributed templates
    /// or for tests.
    pub fn insert(&mut self, tpl: Template) {
        self.by_name.insert(tpl.meta.name.clone(), tpl);
    }

    /// Look up a template by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Template> {
        self.by_name.get(name)
    }

    /// Iterate all templates in the registry. Order is not stable.
    pub fn iter(&self) -> impl Iterator<Item = &Template> {
        self.by_name.values()
    }

    /// Number of templates in the registry.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// Sorted list of (name, optional description) for UI listing.
    #[must_use]
    pub fn list(&self) -> Vec<(String, Option<String>)> {
        let mut out: Vec<_> = self
            .by_name
            .values()
            .map(|t| (t.meta.name.clone(), t.meta.description.clone()))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }
}

fn visit_templates(
    dir: &Path,
    cb: &mut dyn FnMut(&PathBuf) -> Result<(), TemplateRegistryError>,
) -> Result<(), TemplateRegistryError> {
    for entry in std::fs::read_dir(dir).map_err(|source| TemplateRegistryError::Io {
        path: dir.display().to_string(),
        source,
    })? {
        let entry = entry.map_err(|source| TemplateRegistryError::Io {
            path: dir.display().to_string(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            visit_templates(&path, cb)?;
        } else if path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|n| n.ends_with(".template.md"))
        {
            cb(&path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn empty_forge_returns_only_builtins() {
        let dir = tempdir().unwrap();
        let reg = TemplateRegistry::load(dir.path()).unwrap();
        assert!(reg.len() >= 4);
        assert!(reg.get("daily-journal").is_some());
        assert!(reg.get("meeting-notes").is_some());
    }

    #[test]
    fn user_template_overrides_builtin() {
        let dir = tempdir().unwrap();
        let templates_dir = dir.path().join(".forge/templates");
        fs::create_dir_all(&templates_dir).unwrap();
        fs::write(
            templates_dir.join("daily-journal.template.md"),
            "---\nname: daily-journal\ndescription: My override.\n---\nMine.\n",
        )
        .unwrap();

        let reg = TemplateRegistry::load(dir.path()).unwrap();
        let tpl = reg.get("daily-journal").unwrap();
        assert_eq!(tpl.meta.description.as_deref(), Some("My override."));
        assert_eq!(tpl.body, "Mine.\n");
    }

    #[test]
    fn user_template_in_subdir_is_loaded() {
        let dir = tempdir().unwrap();
        let templates_dir = dir.path().join(".forge/templates/personal");
        fs::create_dir_all(&templates_dir).unwrap();
        fs::write(
            templates_dir.join("custom.template.md"),
            "---\nname: my-custom\n---\nBody.\n",
        )
        .unwrap();

        let reg = TemplateRegistry::load(dir.path()).unwrap();
        assert!(reg.get("my-custom").is_some());
    }

    #[test]
    fn malformed_user_template_surfaces_error() {
        let dir = tempdir().unwrap();
        let templates_dir = dir.path().join(".forge/templates");
        fs::create_dir_all(&templates_dir).unwrap();
        fs::write(
            templates_dir.join("bad.template.md"),
            "no frontmatter here at all",
        )
        .unwrap();

        let err = TemplateRegistry::load(dir.path()).unwrap_err();
        assert!(matches!(err, TemplateRegistryError::Parse { .. }));
    }

    #[test]
    fn list_returns_sorted_pairs() {
        let dir = tempdir().unwrap();
        let reg = TemplateRegistry::load(dir.path()).unwrap();
        let list = reg.list();
        let names: Vec<_> = list.iter().map(|(n, _)| n.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn ignores_non_template_md_files() {
        let dir = tempdir().unwrap();
        let templates_dir = dir.path().join(".forge/templates");
        fs::create_dir_all(&templates_dir).unwrap();
        fs::write(
            templates_dir.join("readme.md"), // doesn't end in .template.md
            "---\nname: should-not-load\n---\nx",
        )
        .unwrap();

        let reg = TemplateRegistry::load(dir.path()).unwrap();
        assert!(reg.get("should-not-load").is_none());
    }
}
