//! Slug generation from arbitrary display names.

/// Convert a display name to a URL-safe slug.
///
/// Algorithm:
/// 1. Lowercase all characters.
/// 2. Replace spaces (and adjacent non-slug chars) with `-`.
/// 3. Strip characters that are not `[a-z0-9\-_]`.
/// 4. Collapse multiple consecutive hyphens to a single `-`.
/// 5. Trim leading/trailing hyphens.
///
/// Returns an empty string when the input contains no slug-safe characters.
#[must_use]
pub fn slugify(input: &str) -> String {
    let lowercased = input.to_lowercase();

    // Replace spaces with hyphens first, then strip invalid characters.
    let replaced: String = lowercased
        .chars()
        .map(|c| if c == ' ' { '-' } else { c })
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();

    // Collapse consecutive hyphens.
    let mut slug = String::with_capacity(replaced.len());
    let mut prev_hyphen = false;
    for c in replaced.chars() {
        if c == '-' {
            if !prev_hyphen {
                slug.push('-');
            }
            prev_hyphen = true;
        } else {
            slug.push(c);
            prev_hyphen = false;
        }
    }

    // Trim leading/trailing hyphens.
    slug.trim_matches('-').to_string()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_slug() {
        assert_eq!(slugify("My Great Document!"), "my-great-document");
    }

    #[test]
    fn spaces_become_hyphens() {
        assert_eq!(slugify("hello world"), "hello-world");
    }

    #[test]
    fn underscores_preserved() {
        assert_eq!(slugify("snake_case_name"), "snake_case_name");
    }

    #[test]
    fn consecutive_spaces_collapse() {
        assert_eq!(slugify("a   b"), "a-b");
    }

    #[test]
    fn leading_trailing_hyphens_trimmed() {
        assert_eq!(slugify("  hello  "), "hello");
    }

    #[test]
    fn empty_input() {
        assert_eq!(slugify(""), "");
    }

    #[test]
    fn all_invalid_chars() {
        assert_eq!(slugify("!!!"), "");
    }

    #[test]
    fn numbers_preserved() {
        assert_eq!(slugify("Note 42"), "note-42");
    }

    #[test]
    fn unicode_stripped() {
        // Non-ASCII chars are removed since they aren't in [a-z0-9-_].
        let result = slugify("café");
        // 'c', 'a', 'f' survive; 'é' is non-ASCII and gets dropped.
        assert!(result.starts_with("caf"));
        assert!(!result.contains('é'));
    }

    #[test]
    fn mixed_punctuation() {
        assert_eq!(slugify("Hello, World!"), "hello-world");
    }

    #[test]
    fn already_a_slug() {
        assert_eq!(slugify("already-a-slug"), "already-a-slug");
    }
}
