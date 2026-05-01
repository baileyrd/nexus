//! Filter expression evaluator for Obsidian `.base` files.
//!
//! Pure logic: takes a parsed [`super::FilterNode`] and a [`NoteFacts`]
//! describing one note (frontmatter + file intrinsics) and returns
//! whether the note should appear in the base. No I/O, no `SQLite`.
//!
//! See ADR 0019 for the supported grammar. Anything outside it is
//! reported via [`EvalReport::unsupported`] rather than silently
//! failing — callers surface the list to the user as a banner.
//!
//! ## Grammar (v1)
//!
//! ```text
//! expr   := lhs op rhs
//!         | lhs '.' method '(' literal ')'
//!         | '!' expr
//! lhs    := identifier ('.' identifier)*
//! op     := '==' | '!=' | '>' | '<' | '>=' | '<='
//! method := contains | startsWith | endsWith
//! rhs    := literal
//! ```

use std::collections::BTreeMap;

use serde_json::Value;

use super::FilterNode;

// ── Public surface ──────────────────────────────────────────────────────────

/// All facts about a single note that the evaluator can reference.
///
/// Built once per candidate note by the query layer (`nexus-storage`),
/// then evaluated against every leaf expression in the filter tree.
#[derive(Debug, Clone, Default)]
pub struct NoteFacts {
    /// Filename without extension (`file.name`).
    pub name: String,
    /// Forge-relative path (`file.path`).
    pub path: String,
    /// Extension without the leading dot (`file.ext`).
    pub ext: String,
    /// Containing folder, forge-relative (`file.folder`). Empty for
    /// notes at the forge root.
    pub folder: String,
    /// Creation timestamp, Unix seconds (`file.ctime`).
    pub ctime: i64,
    /// Modification timestamp, Unix seconds (`file.mtime`).
    pub mtime: i64,
    /// Tags from frontmatter and inline (`file.tags`). Lowercased,
    /// without the leading `#`.
    pub tags: Vec<String>,
    /// Frontmatter properties, keyed by property name. Values follow
    /// JSON conventions so YAML scalars, arrays, and maps all flow
    /// through unchanged.
    pub frontmatter: BTreeMap<String, Value>,
}

/// Result of evaluating a filter tree against a single note.
#[derive(Debug, Clone, Default)]
pub struct EvalReport {
    /// Whether the note matches.
    pub matched: bool,
    /// Distinct unsupported expressions encountered during evaluation.
    /// Stable across notes — callers typically deduplicate across
    /// the whole vault before showing a banner.
    pub unsupported: Vec<String>,
}

/// Evaluate a filter tree against one note.
///
/// `None` for `node` means "no filter configured" — every note matches.
#[must_use]
pub fn evaluate(node: Option<&FilterNode>, facts: &NoteFacts) -> EvalReport {
    let mut report = EvalReport::default();
    report.matched = match node {
        None => true,
        Some(n) => eval_node(n, facts, &mut report.unsupported),
    };
    report
}

fn eval_node(node: &FilterNode, facts: &NoteFacts, unsupported: &mut Vec<String>) -> bool {
    match node {
        FilterNode::And { and } => and.iter().all(|c| eval_node(c, facts, unsupported)),
        FilterNode::Or { or } => or.iter().any(|c| eval_node(c, facts, unsupported)),
        FilterNode::Not { not } => !eval_node(not, facts, unsupported),
        FilterNode::Expr(src) => match parse_expr(src) {
            Ok(expr) => eval_expr(&expr, facts),
            Err(reason) => {
                let entry = format!("{src}  ({reason})");
                if !unsupported.iter().any(|e| e == &entry) {
                    unsupported.push(entry);
                }
                false
            }
        },
    }
}

// ── Parsed expression ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Expr {
    /// `lhs op literal`
    Binary {
        lhs: Lhs,
        op: BinOp,
        rhs: Literal,
    },
    /// `lhs.method(literal)` or `!lhs.method(literal)`
    Method {
        negated: bool,
        lhs: Lhs,
        method: Method,
        rhs: Literal,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BinOp {
    Eq,
    Neq,
    Gt,
    Lt,
    Gte,
    Lte,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Method {
    Contains,
    StartsWith,
    EndsWith,
}

#[derive(Debug, Clone)]
enum Lhs {
    /// `file.name`, `file.path`, …
    File(FileIntrinsic),
    /// Bare frontmatter property name.
    Frontmatter(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileIntrinsic {
    Name,
    Path,
    Ext,
    Folder,
    Ctime,
    Mtime,
    Tags,
}

#[derive(Debug, Clone)]
enum Literal {
    String(String),
    Number(f64),
    Bool(bool),
    Null,
}

// ── Parser ──────────────────────────────────────────────────────────────────
//
// Hand-written recursive-descent. The grammar is small enough that
// pulling in a parser combinator (nom, chumsky, winnow) would be
// overkill — three productions, no precedence layers, no recursion.
//
// Effective grammar:
//
//     expr           ::= ('!' method_call) | method_call | binary
//     method_call    ::= lhs '.' method '(' literal ')'
//     binary         ::= lhs operator rhs
//
// The `!` prefix is **only** valid in front of a `method_call` —
// `! a == b` is rejected with `"'!' is only supported on
// method-call expressions"`. ADR 0019 originally documented the
// looser shape `'!' expr`; the implementation deliberately
// narrowed it to method-calls because negating a binary like
// `a == b` is better written as `a != b`. See issue #82.
fn parse_expr(src: &str) -> Result<Expr, String> {
    let trimmed = src.trim();
    let (negated, body) = match trimmed.strip_prefix('!') {
        Some(rest) => (true, rest.trim()),
        None => (false, trimmed),
    };

    // Try method-call form first because `lhs.method(literal)` would
    // otherwise be misread as a binary op on a dotted identifier.
    if let Some(method) = parse_method_call(body)? {
        return Ok(Expr::Method {
            negated,
            lhs: method.lhs,
            method: method.method,
            rhs: method.rhs,
        });
    }

    if negated {
        return Err("'!' is only supported on method-call expressions".to_string());
    }

    parse_binary(body)
}

struct ParsedMethod {
    lhs: Lhs,
    method: Method,
    rhs: Literal,
}

fn parse_method_call(src: &str) -> Result<Option<ParsedMethod>, String> {
    // Pattern: `<lhs>.<method>(<literal>)` — the closing `)` must be
    // the final non-whitespace char, and the literal must contain no
    // unescaped `)`. We split on the *last* `(` after the *last* `.`
    // before that `(`.
    let Some(open) = src.rfind('(') else {
        return Ok(None);
    };
    if !src[open + 1..].trim_end().ends_with(')') {
        return Ok(None);
    }
    let head = src[..open].trim_end();
    let Some(dot) = head.rfind('.') else {
        return Ok(None);
    };
    let method_name = head[dot + 1..].trim();
    let method = match method_name {
        "contains" => Method::Contains,
        "startsWith" => Method::StartsWith,
        "endsWith" => Method::EndsWith,
        _ => return Ok(None),
    };
    let lhs_src = head[..dot].trim();
    let lhs = parse_lhs(lhs_src)?;
    let inside_end = src.rfind(')').expect("rfind ')' after end-with check");
    let arg_src = src[open + 1..inside_end].trim();
    let rhs = parse_literal(arg_src)?;
    Ok(Some(ParsedMethod { lhs, method, rhs }))
}

fn parse_binary(src: &str) -> Result<Expr, String> {
    // Operator search must prefer 2-char ops over 1-char prefixes.
    const OPS: &[(&str, BinOp)] = &[
        ("==", BinOp::Eq),
        ("!=", BinOp::Neq),
        (">=", BinOp::Gte),
        ("<=", BinOp::Lte),
        (">", BinOp::Gt),
        ("<", BinOp::Lt),
    ];

    for (sym, op) in OPS {
        if let Some(idx) = find_op_outside_string(src, sym) {
            let lhs_src = src[..idx].trim();
            let rhs_src = src[idx + sym.len()..].trim();
            let lhs = parse_lhs(lhs_src)?;
            let rhs = parse_literal(rhs_src)?;
            return Ok(Expr::Binary { lhs, op: *op, rhs });
        }
    }

    Err(format!("no supported operator in expression: {src:?}"))
}

/// Find an operator outside any single- or double-quoted string. Used
/// so `title == "foo == bar"` doesn't split on the inner `==`.
///
/// Recognises `\\` and `\<quote>` as escape sequences inside strings
/// — `title == "she said \"hi\""` no longer closes the string at the
/// inner `\"`. Pre-#82 the loop closed at every matching quote
/// regardless of a preceding backslash, so an operator-shaped
/// substring after the false-close would split the expression at
/// the wrong position. Note: the literal parser elsewhere in this
/// module does not currently *interpret* escapes (e.g. `\"` inside
/// the literal value gets passed through verbatim) — supporting
/// them in the value is a separate fix; this function just refuses
/// to be fooled by them when looking for operator boundaries.
fn find_op_outside_string(src: &str, op: &str) -> Option<usize> {
    let bytes = src.as_bytes();
    let op_bytes = op.as_bytes();
    let mut i = 0;
    let mut quote: Option<u8> = None;
    while i + op_bytes.len() <= bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) => {
                // Inside a string. Skip the next byte after a
                // backslash so `\"` and `\\` don't close the string.
                if b == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                if b == q {
                    quote = None;
                    i += 1;
                    continue;
                }
                i += 1;
                continue;
            }
            None => {}
        }
        if b == b'"' || b == b'\'' {
            quote = Some(b);
            i += 1;
            continue;
        }
        if &bytes[i..i + op_bytes.len()] == op_bytes {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn parse_lhs(src: &str) -> Result<Lhs, String> {
    if !is_identifier_path(src) {
        return Err(format!("invalid identifier: {src:?}"));
    }
    if let Some(rest) = src.strip_prefix("file.") {
        let intrinsic = match rest {
            "name" => FileIntrinsic::Name,
            "path" => FileIntrinsic::Path,
            "ext" => FileIntrinsic::Ext,
            "folder" => FileIntrinsic::Folder,
            "ctime" => FileIntrinsic::Ctime,
            "mtime" => FileIntrinsic::Mtime,
            "tags" => FileIntrinsic::Tags,
            other => return Err(format!("unknown file intrinsic: file.{other}")),
        };
        return Ok(Lhs::File(intrinsic));
    }
    if src.contains('.') {
        return Err(format!("dotted property paths not supported: {src}"));
    }
    Ok(Lhs::Frontmatter(src.to_string()))
}

fn is_identifier_path(src: &str) -> bool {
    if src.is_empty() {
        return false;
    }
    let mut prev_dot = true;
    for c in src.chars() {
        if c == '.' {
            if prev_dot {
                return false;
            }
            prev_dot = true;
        } else if c.is_alphanumeric() || c == '_' || c == '-' {
            prev_dot = false;
        } else {
            return false;
        }
    }
    !prev_dot
}

fn parse_literal(src: &str) -> Result<Literal, String> {
    let s = src.trim();
    if s.is_empty() {
        return Err("missing literal".to_string());
    }
    if s == "null" {
        return Ok(Literal::Null);
    }
    if s == "true" {
        return Ok(Literal::Bool(true));
    }
    if s == "false" {
        return Ok(Literal::Bool(false));
    }
    let first = s.as_bytes()[0];
    if first == b'"' || first == b'\'' {
        let last = *s.as_bytes().last().unwrap();
        if last != first || s.len() < 2 {
            return Err(format!("unterminated string literal: {s}"));
        }
        return Ok(Literal::String(s[1..s.len() - 1].to_string()));
    }
    if let Ok(n) = s.parse::<f64>() {
        return Ok(Literal::Number(n));
    }
    Err(format!("unrecognized literal: {s}"))
}

// ── Evaluator ───────────────────────────────────────────────────────────────

fn eval_expr(expr: &Expr, facts: &NoteFacts) -> bool {
    match expr {
        Expr::Binary { lhs, op, rhs } => {
            let left = resolve(lhs, facts);
            apply_binop(&left, *op, rhs)
        }
        Expr::Method {
            negated,
            lhs,
            method,
            rhs,
        } => {
            let left = resolve(lhs, facts);
            let result = apply_method(&left, *method, rhs);
            if *negated {
                !result
            } else {
                result
            }
        }
    }
}

fn resolve(lhs: &Lhs, facts: &NoteFacts) -> Value {
    match lhs {
        Lhs::File(FileIntrinsic::Name) => Value::String(facts.name.clone()),
        Lhs::File(FileIntrinsic::Path) => Value::String(facts.path.clone()),
        Lhs::File(FileIntrinsic::Ext) => Value::String(facts.ext.clone()),
        Lhs::File(FileIntrinsic::Folder) => Value::String(facts.folder.clone()),
        Lhs::File(FileIntrinsic::Ctime) => Value::Number(facts.ctime.into()),
        Lhs::File(FileIntrinsic::Mtime) => Value::Number(facts.mtime.into()),
        Lhs::File(FileIntrinsic::Tags) => Value::Array(
            facts
                .tags
                .iter()
                .map(|t| Value::String(t.clone()))
                .collect(),
        ),
        Lhs::Frontmatter(key) => facts.frontmatter.get(key).cloned().unwrap_or(Value::Null),
    }
}

fn apply_binop(left: &Value, op: BinOp, right: &Literal) -> bool {
    use BinOp::{Eq, Gt, Gte, Lt, Lte, Neq};

    // Equality across mixed types follows the loose rules Obsidian
    // uses: numbers compare numerically, strings textually, null only
    // matches null, booleans match booleans.
    match op {
        Eq => values_equal(left, right),
        Neq => !values_equal(left, right),
        Gt | Lt | Gte | Lte => match compare(left, right) {
            Some(ord) => match op {
                Gt => ord.is_gt(),
                Lt => ord.is_lt(),
                Gte => !ord.is_lt(),
                Lte => !ord.is_gt(),
                _ => unreachable!(),
            },
            None => false,
        },
    }
}

fn values_equal(left: &Value, right: &Literal) -> bool {
    match (left, right) {
        (Value::Null, Literal::Null) => true,
        (Value::Bool(a), Literal::Bool(b)) => a == b,
        (Value::String(a), Literal::String(b)) => a == b,
        // Both sides come from explicit user-written literals — no
        // arithmetic, so exact comparison is the right semantic.
        #[allow(clippy::float_cmp)]
        (Value::Number(a), Literal::Number(b)) => a.as_f64().is_some_and(|n| n == *b),
        // Array equality: any element matching counts as a match,
        // mirroring how Obsidian treats `tags == "book"` against a
        // list of tags. Strict array equality is rarely what users
        // want in `.base` filters.
        (Value::Array(items), _) => items
            .iter()
            .any(|item| values_equal(item, right)),
        _ => false,
    }
}

fn compare(left: &Value, right: &Literal) -> Option<std::cmp::Ordering> {
    match (left, right) {
        (Value::Number(a), Literal::Number(b)) => a.as_f64().and_then(|n| n.partial_cmp(b)),
        (Value::String(a), Literal::String(b)) => Some(a.cmp(b)),
        _ => None,
    }
}

fn apply_method(left: &Value, method: Method, right: &Literal) -> bool {
    let needle = match right {
        Literal::String(s) => s.as_str(),
        _ => return false,
    };
    let test = |s: &str| match method {
        Method::Contains => s.contains(needle),
        Method::StartsWith => s.starts_with(needle),
        Method::EndsWith => s.ends_with(needle),
    };
    match left {
        Value::String(s) => test(s),
        Value::Array(items) => items.iter().any(|item| match item {
            Value::String(s) => test(s),
            _ => false,
        }),
        _ => false,
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod find_op_tests {
    use super::find_op_outside_string;

    #[test]
    fn finds_op_in_plain_expression() {
        assert_eq!(find_op_outside_string("a == b", "=="), Some(2));
    }

    #[test]
    fn ignores_op_inside_double_quoted_string() {
        // The inner `==` must not split the expression.
        assert_eq!(
            find_op_outside_string(r#"title == "foo == bar""#, "=="),
            Some(6),
            "outer == is at byte 6, not the inner one inside quotes"
        );
    }

    #[test]
    fn ignores_op_inside_single_quoted_string() {
        assert_eq!(
            find_op_outside_string("title == 'foo == bar'", "=="),
            Some(6)
        );
    }

    /// Issue #82. Pre-fix the loop closed at every matching quote
    /// regardless of a preceding backslash, so `\"` inside a string
    /// false-closed the string and an operator after it would be
    /// found at the wrong position. Now `\"` (and `\\`) are
    /// recognised as escapes; the string stays open until an
    /// un-escaped matching quote.
    #[test]
    fn escaped_quotes_do_not_close_strings() {
        // The whole `"she said \"hi\""` is a single string. The only
        // `==` is the outer operator at byte 6.
        assert_eq!(
            find_op_outside_string(r#"title == "she said \"hi\"""#, "=="),
            Some(6)
        );
        // Backslash-backslash also doesn't close.
        assert_eq!(
            find_op_outside_string(r#"path == "C:\\\\foo""#, "=="),
            Some(5)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn facts() -> NoteFacts {
        NoteFacts {
            name: "Dune".to_string(),
            path: "books/Dune.md".to_string(),
            ext: "md".to_string(),
            folder: "books".to_string(),
            ctime: 1_700_000_000,
            mtime: 1_710_000_000,
            tags: vec!["book".to_string(), "scifi".to_string()],
            frontmatter: BTreeMap::from([
                ("type".to_string(), json!("literature")),
                ("title".to_string(), json!("Dune")),
                ("author".to_string(), json!("Frank Herbert")),
                ("year".to_string(), json!(1965)),
                ("rating".to_string(), json!(5)),
                ("archived".to_string(), json!(false)),
            ]),
        }
    }

    fn expr_node(src: &str) -> FilterNode {
        FilterNode::Expr(src.to_string())
    }

    fn assert_match(src: &str, f: &NoteFacts) {
        let report = evaluate(Some(&expr_node(src)), f);
        assert!(report.matched, "expected `{src}` to match");
        assert!(report.unsupported.is_empty(), "{:?}", report.unsupported);
    }

    fn assert_no_match(src: &str, f: &NoteFacts) {
        let report = evaluate(Some(&expr_node(src)), f);
        assert!(!report.matched, "expected `{src}` not to match");
        assert!(report.unsupported.is_empty(), "{:?}", report.unsupported);
    }

    fn assert_unsupported(src: &str, f: &NoteFacts) {
        let report = evaluate(Some(&expr_node(src)), f);
        assert!(!report.matched);
        assert_eq!(report.unsupported.len(), 1, "{:?}", report.unsupported);
    }

    #[test]
    fn no_filter_matches_everything() {
        let report = evaluate(None, &facts());
        assert!(report.matched);
    }

    #[test]
    fn equality_on_frontmatter_string() {
        let f = facts();
        assert_match(r#"type == "literature""#, &f);
        assert_no_match(r#"type == "movie""#, &f);
    }

    #[test]
    fn inequality() {
        assert_match(r#"type != "movie""#, &facts());
    }

    #[test]
    fn numeric_comparisons() {
        let f = facts();
        assert_match("year > 1900", &f);
        assert_match("year >= 1965", &f);
        assert_match("year <= 1965", &f);
        assert_no_match("year < 1965", &f);
        assert_no_match("rating > 5", &f);
    }

    #[test]
    fn boolean_literal() {
        let f = facts();
        assert_match("archived == false", &f);
        assert_no_match("archived == true", &f);
    }

    #[test]
    fn null_compares_only_to_null() {
        let mut f = facts();
        f.frontmatter.remove("title");
        assert_match("title == null", &f);
        assert_no_match(r#"title == "Dune""#, &f);
    }

    #[test]
    fn file_intrinsics_resolve() {
        let f = facts();
        assert_match(r#"file.name == "Dune""#, &f);
        assert_match(r#"file.path == "books/Dune.md""#, &f);
        assert_match(r#"file.ext == "md""#, &f);
        assert_match(r#"file.folder == "books""#, &f);
        assert_match("file.ctime >= 1700000000", &f);
        assert_match("file.mtime > 1700000000", &f);
    }

    #[test]
    fn tags_array_equality_matches_any_element() {
        let f = facts();
        assert_match(r#"file.tags == "book""#, &f);
        assert_match(r#"file.tags == "scifi""#, &f);
        assert_no_match(r#"file.tags == "fantasy""#, &f);
    }

    #[test]
    fn method_contains_on_string() {
        assert_match(r#"title.contains("un")"#, &facts());
        assert_no_match(r#"title.contains("zzz")"#, &facts());
    }

    #[test]
    fn method_starts_and_ends_with() {
        assert_match(r#"title.startsWith("Du")"#, &facts());
        assert_match(r#"title.endsWith("ne")"#, &facts());
        assert_no_match(r#"title.startsWith("zz")"#, &facts());
    }

    #[test]
    fn method_contains_on_tags_array() {
        assert_match(r#"file.tags.contains("sci")"#, &facts());
    }

    #[test]
    fn negated_method() {
        assert_match(r#"!title.contains("xxx")"#, &facts());
        assert_no_match(r#"!title.contains("Du")"#, &facts());
    }

    #[test]
    fn quoted_operator_inside_string_does_not_split() {
        // The inner `==` must not be picked as the splitting operator.
        let f = facts();
        assert_no_match(r#"title == "x == y""#, &f);
    }

    #[test]
    fn and_or_not_combine() {
        let f = facts();
        let node = FilterNode::And {
            and: vec![
                expr_node(r#"type == "literature""#),
                FilterNode::Or {
                    or: vec![
                        expr_node("year < 1900"),
                        expr_node("year >= 1965"),
                    ],
                },
                FilterNode::Not {
                    not: Box::new(expr_node("archived == true")),
                },
            ],
        };
        let report = evaluate(Some(&node), &f);
        assert!(report.matched);
        assert!(report.unsupported.is_empty());
    }

    #[test]
    fn unsupported_expression_recorded_not_silent() {
        // Formula-style call we do not parse.
        assert_unsupported("formula(rating * 2) > 5", &facts());
    }

    #[test]
    fn unsupported_unknown_intrinsic() {
        assert_unsupported(r#"file.unknown == "x""#, &facts());
    }

    #[test]
    fn unsupported_dotted_frontmatter() {
        // `nested.path` against frontmatter is not supported in v1.
        assert_unsupported(r#"author.name == "Herbert""#, &facts());
    }

    #[test]
    fn unsupported_deduplicated_across_or_branches() {
        let node = FilterNode::Or {
            or: vec![
                expr_node("formula(x) > 1"),
                expr_node("formula(x) > 1"),
            ],
        };
        let report = evaluate(Some(&node), &facts());
        assert_eq!(report.unsupported.len(), 1);
    }

    #[test]
    fn missing_frontmatter_property_is_null() {
        let f = facts();
        assert_match("nonexistent == null", &f);
        assert_no_match(r#"nonexistent == "x""#, &f);
    }
}
