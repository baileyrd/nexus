//! Recursive embed resolution with depth and cycle guards.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::error::MarkdownError;
use crate::markdown::wikilinks;

/// Maximum allowed embed nesting depth.
pub const MAX_EMBED_DEPTH: usize = 10;

/// Resolve all `![[embed]]` references in `root_path`, returning the fully
/// substituted markdown text.
///
/// `reader` is a callable that reads file contents — injected for testability.
/// `forge_root` is the vault root used to resolve relative embed targets.
///
/// # Errors
///
/// - [`MarkdownError::EmbedDepthExceeded`] when nesting exceeds [`MAX_EMBED_DEPTH`].
/// - [`MarkdownError::CircularEmbed`] when a cycle is detected.
pub fn resolve_embeds(
    root_path: &Path,
    forge_root: &Path,
    reader: &dyn Fn(&Path) -> std::io::Result<String>,
) -> Result<String, MarkdownError> {
    let canonical = canonicalize_best_effort(root_path);
    let mut visited = HashSet::new();
    visited.insert(canonical.clone());
    resolve_recursive(root_path, &canonical, forge_root, reader, &mut visited, 0)
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn resolve_recursive(
    path: &Path,
    _canonical: &Path,
    forge_root: &Path,
    reader: &dyn Fn(&Path) -> std::io::Result<String>,
    visited: &mut HashSet<PathBuf>,
    depth: usize,
) -> Result<String, MarkdownError> {
    if depth >= MAX_EMBED_DEPTH {
        return Err(MarkdownError::EmbedDepthExceeded {
            file: path.display().to_string(),
            max: MAX_EMBED_DEPTH,
        });
    }

    let content = reader(path).unwrap_or_default();
    let links = wikilinks::scan(&content);
    let embeds: Vec<_> = links
        .into_iter()
        .filter(|l| l.link_type == wikilinks::LinkType::Embed)
        .collect();

    if embeds.is_empty() {
        return Ok(content);
    }

    let parent_dir = path.parent().unwrap_or(forge_root);
    let mut result = content;

    for embed in &embeds {
        let embed_path = resolve_embed_target(&embed.target, parent_dir, forge_root);
        let embed_canonical = canonicalize_best_effort(&embed_path);

        if visited.contains(&embed_canonical) {
            // Collect the cycle path for the error.
            let cycle: Vec<String> = visited
                .iter()
                .map(|p| p.display().to_string())
                .chain(std::iter::once(embed_canonical.display().to_string()))
                .collect();
            return Err(MarkdownError::CircularEmbed { cycle });
        }

        visited.insert(embed_canonical.clone());
        let embedded = resolve_recursive(
            &embed_path,
            &embed_canonical,
            forge_root,
            reader,
            visited,
            depth + 1,
        )?;
        visited.remove(&embed_canonical);

        // Substitute the embed placeholder with the resolved content.
        let embed_syntax = format!("![[{}]]", embed.target);
        result = result.replace(&embed_syntax, &embedded);
    }

    Ok(result)
}

/// Resolve an embed target string to a `PathBuf`.
///
/// Tries `parent_dir/target` first, then `forge_root/target`.
fn resolve_embed_target(target: &str, parent_dir: &Path, forge_root: &Path) -> PathBuf {
    let relative = parent_dir.join(target);
    if relative.exists() {
        return relative;
    }
    forge_root.join(target)
}

/// Best-effort canonicalization — falls back to the original path on error.
fn canonicalize_best_effort(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_reader(
        files: HashMap<PathBuf, &'static str>,
    ) -> impl Fn(&Path) -> std::io::Result<String> {
        move |p: &Path| {
            files.get(p).map_or_else(
                || {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "not found",
                    ))
                },
                |s| Ok((*s).to_string()),
            )
        }
    }

    #[test]
    fn no_embeds_returns_content_unchanged() {
        let forge_root = PathBuf::from("/forge");
        let root = PathBuf::from("/forge/a.md");
        let mut files = HashMap::new();
        files.insert(root.clone(), "# Hello\nNo embeds here.\n");

        let reader = make_reader(files);
        let result = resolve_embeds(&root, &forge_root, &reader).unwrap();
        assert_eq!(result, "# Hello\nNo embeds here.\n");
    }

    #[test]
    fn simple_embed_substitution() {
        let forge_root = PathBuf::from("/forge");
        let root = PathBuf::from("/forge/parent.md");
        let child = PathBuf::from("/forge/child.md");

        let mut files = HashMap::new();
        files.insert(root.clone(), "Before\n![[child.md]]\nAfter\n");
        files.insert(child.clone(), "## Child Content\n");

        let reader = make_reader(files);
        let result = resolve_embeds(&root, &forge_root, &reader).unwrap();
        assert!(result.contains("## Child Content"));
        assert!(result.contains("Before"));
        assert!(result.contains("After"));
    }

    #[test]
    fn depth_limit_exceeded_returns_error() {
        // Create a chain: a → b → c → … → (depth 10)
        let forge_root = PathBuf::from("/forge");
        let root = PathBuf::from("/forge/a.md");

        // All files embed the next one to force depth overflow.
        let reader = |p: &Path| -> std::io::Result<String> {
            let stem = p
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);
            Ok(format!("![[{}.md]]\n", stem + 1))
        };

        let result = resolve_embeds(&root, &forge_root, &reader);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, MarkdownError::EmbedDepthExceeded { .. }));
    }
}
