//! The hashline grammar: text → [`Patch`].

use crate::error::HashlineError;

/// A parsed patch: one or more file sections, applied in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch {
    /// File sections, in the order they appear in the patch text.
    pub sections: Vec<FileSection>,
}

/// A single `[PATH#TAG]` section and its operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSection {
    /// Target file path, as written in the header.
    pub path: String,
    /// The 4-uppercase-hex TAG the patch was authored against.
    pub tag: String,
    /// Operations to apply to the file, in order.
    pub ops: Vec<Op>,
}

/// One edit operation. Line numbers are 1-based and inclusive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// Replace lines `start..=end` with `body`.
    Swap {
        /// First line replaced (1-based).
        start: usize,
        /// Last line replaced (1-based, inclusive).
        end: usize,
        /// Replacement lines.
        body: Vec<String>,
    },
    /// Delete lines `start..=end`.
    Del {
        /// First line deleted (1-based).
        start: usize,
        /// Last line deleted (1-based, inclusive).
        end: usize,
    },
    /// Insert `body` before line `line`.
    InsPre {
        /// Anchor line (1-based); new lines go before it.
        line: usize,
        /// Inserted lines.
        body: Vec<String>,
    },
    /// Insert `body` after line `line`.
    InsPost {
        /// Anchor line (1-based); new lines go after it.
        line: usize,
        /// Inserted lines.
        body: Vec<String>,
    },
    /// Insert `body` at the very start of the file.
    InsHead {
        /// Inserted lines.
        body: Vec<String>,
    },
    /// Insert `body` at the very end of the file.
    InsTail {
        /// Inserted lines.
        body: Vec<String>,
    },
    /// Replace the syntactic block starting at `start` (needs tree-sitter).
    SwapBlock {
        /// Block-start line (1-based).
        start: usize,
        /// Replacement lines.
        body: Vec<String>,
    },
    /// Delete the syntactic block starting at `start` (needs tree-sitter).
    DelBlock {
        /// Block-start line (1-based).
        start: usize,
    },
    /// Insert `body` after a block's last line (needs tree-sitter).
    InsBlockPost {
        /// Block-start line (1-based).
        start: usize,
        /// Inserted lines.
        body: Vec<String>,
    },
}

impl Op {
    /// Whether this operation needs tree-sitter block resolution (Phase 5.2).
    #[must_use]
    pub fn is_block(&self) -> bool {
        matches!(
            self,
            Op::SwapBlock { .. } | Op::DelBlock { .. } | Op::InsBlockPost { .. }
        )
    }
}

/// Parse hashline patch text into a [`Patch`].
///
/// # Errors
///
/// Returns [`HashlineError::BadSectionHeader`] or [`HashlineError::BadOp`] for
/// malformed headers or operations.
pub fn parse(input: &str) -> Result<Patch, HashlineError> {
    let lines: Vec<&str> = input.split('\n').collect();
    let mut sections: Vec<FileSection> = Vec::new();
    let mut current: Option<FileSection> = None;
    let mut i = 0;

    while i < lines.len() {
        let lineno = i + 1;
        let header = lines[i].trim();

        if header.is_empty() {
            i += 1;
            continue;
        }

        if header.starts_with('[') && header.ends_with(']') {
            if let Some(done) = current.take() {
                sections.push(done);
            }
            let (path, tag) = parse_section_header(header, lineno)?;
            current = Some(FileSection {
                path,
                tag,
                ops: Vec::new(),
            });
            i += 1;
            continue;
        }

        let section = current
            .as_mut()
            .ok_or(HashlineError::BadSectionHeader { line: lineno })?;
        let op_header = parse_op_header(header, lineno)?;

        let mut body = Vec::new();
        i += 1;
        if op_header.takes_body() {
            while i < lines.len() {
                let row = lines[i].strip_suffix('\r').unwrap_or(lines[i]);
                if let Some(content) = row.strip_prefix('+') {
                    body.push(content.to_string());
                    i += 1;
                } else {
                    break;
                }
            }
        }
        section.ops.push(op_header.into_op(body));
    }

    if let Some(done) = current.take() {
        sections.push(done);
    }
    Ok(Patch { sections })
}

/// Op header shape, before the body is attached.
enum OpHeader {
    Swap { start: usize, end: usize },
    Del { start: usize, end: usize },
    InsPre { line: usize },
    InsPost { line: usize },
    InsHead,
    InsTail,
    SwapBlock { start: usize },
    DelBlock { start: usize },
    InsBlockPost { start: usize },
}

impl OpHeader {
    fn takes_body(&self) -> bool {
        !matches!(self, OpHeader::Del { .. } | OpHeader::DelBlock { .. })
    }

    fn into_op(self, body: Vec<String>) -> Op {
        match self {
            OpHeader::Swap { start, end } => Op::Swap { start, end, body },
            OpHeader::Del { start, end } => Op::Del { start, end },
            OpHeader::InsPre { line } => Op::InsPre { line, body },
            OpHeader::InsPost { line } => Op::InsPost { line, body },
            OpHeader::InsHead => Op::InsHead { body },
            OpHeader::InsTail => Op::InsTail { body },
            OpHeader::SwapBlock { start } => Op::SwapBlock { start, body },
            OpHeader::DelBlock { start } => Op::DelBlock { start },
            OpHeader::InsBlockPost { start } => Op::InsBlockPost { start, body },
        }
    }
}

fn parse_section_header(s: &str, lineno: usize) -> Result<(String, String), HashlineError> {
    let inner = &s[1..s.len() - 1];
    let idx = inner
        .rfind('#')
        .ok_or(HashlineError::BadSectionHeader { line: lineno })?;
    let path = &inner[..idx];
    let tag = &inner[idx + 1..];
    if path.is_empty()
        || tag.len() != crate::tag::TAG_HEX_LEN
        || !tag.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(HashlineError::BadSectionHeader { line: lineno });
    }
    Ok((path.to_string(), tag.to_ascii_uppercase()))
}

fn parse_op_header(s: &str, lineno: usize) -> Result<OpHeader, HashlineError> {
    if s == "INS.HEAD:" {
        return Ok(OpHeader::InsHead);
    }
    if s == "INS.TAIL:" {
        return Ok(OpHeader::InsTail);
    }
    if let Some(rest) = s.strip_prefix("INS.BLK.POST ") {
        return Ok(OpHeader::InsBlockPost {
            start: parse_single(strip_colon(rest, lineno)?, lineno)?,
        });
    }
    if let Some(rest) = s.strip_prefix("INS.PRE ") {
        return Ok(OpHeader::InsPre {
            line: parse_single(strip_colon(rest, lineno)?, lineno)?,
        });
    }
    if let Some(rest) = s.strip_prefix("INS.POST ") {
        return Ok(OpHeader::InsPost {
            line: parse_single(strip_colon(rest, lineno)?, lineno)?,
        });
    }
    if let Some(rest) = s.strip_prefix("SWAP.BLK ") {
        return Ok(OpHeader::SwapBlock {
            start: parse_single(strip_colon(rest, lineno)?, lineno)?,
        });
    }
    if let Some(rest) = s.strip_prefix("DEL.BLK ") {
        return Ok(OpHeader::DelBlock {
            start: parse_single(rest, lineno)?,
        });
    }
    if let Some(rest) = s.strip_prefix("SWAP ") {
        let (start, end) = parse_range(strip_colon(rest, lineno)?, lineno)?;
        return Ok(OpHeader::Swap { start, end });
    }
    if let Some(rest) = s.strip_prefix("DEL ") {
        let (start, end) = parse_range(rest, lineno)?;
        return Ok(OpHeader::Del { start, end });
    }
    Err(HashlineError::BadOp {
        line: lineno,
        detail: "unrecognized operation".to_string(),
    })
}

fn strip_colon(s: &str, lineno: usize) -> Result<&str, HashlineError> {
    s.strip_suffix(':')
        .map(str::trim)
        .ok_or(HashlineError::BadOp {
            line: lineno,
            detail: "missing trailing ':'".to_string(),
        })
}

fn parse_single(s: &str, lineno: usize) -> Result<usize, HashlineError> {
    s.trim().parse::<usize>().map_err(|_| HashlineError::BadOp {
        line: lineno,
        detail: format!("expected a line number, found {s:?}"),
    })
}

fn parse_range(s: &str, lineno: usize) -> Result<(usize, usize), HashlineError> {
    let (a, b) = s.split_once(".=").ok_or(HashlineError::BadOp {
        line: lineno,
        detail: "expected an `A.=B` range".to_string(),
    })?;
    Ok((parse_single(a, lineno)?, parse_single(b, lineno)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_swap_with_body() {
        let p = parse("[a.rs#ABCD]\nSWAP 2.=3:\n+x\n+y\n").unwrap();
        assert_eq!(p.sections.len(), 1);
        assert_eq!(p.sections[0].path, "a.rs");
        assert_eq!(p.sections[0].tag, "ABCD");
        assert_eq!(
            p.sections[0].ops,
            vec![Op::Swap {
                start: 2,
                end: 3,
                body: vec!["x".to_string(), "y".to_string()],
            }]
        );
    }

    #[test]
    fn parses_del_without_body() {
        let p = parse("[a#0001]\nDEL 5.=7\n").unwrap();
        assert_eq!(p.sections[0].ops, vec![Op::Del { start: 5, end: 7 }]);
    }

    #[test]
    fn parses_all_insert_forms() {
        let p = parse(
            "[a#00ff]\nINS.HEAD:\n+top\nINS.TAIL:\n+bottom\nINS.PRE 3:\n+before\nINS.POST 3:\n+after\n",
        )
        .unwrap();
        assert_eq!(
            p.sections[0].ops,
            vec![
                Op::InsHead {
                    body: vec!["top".to_string()]
                },
                Op::InsTail {
                    body: vec!["bottom".to_string()]
                },
                Op::InsPre {
                    line: 3,
                    body: vec!["before".to_string()]
                },
                Op::InsPost {
                    line: 3,
                    body: vec!["after".to_string()]
                },
            ]
        );
    }

    #[test]
    fn blank_body_row_and_escapes() {
        // `+` alone = blank line; `++x` decodes to `+x`; `+-y` decodes to `-y`.
        let p = parse("[a#ABCD]\nSWAP 1.=1:\n+\n++x\n+-y\n").unwrap();
        let Op::Swap { body, .. } = &p.sections[0].ops[0] else {
            panic!("expected swap");
        };
        assert_eq!(body, &[String::new(), "+x".to_string(), "-y".to_string()]);
    }

    #[test]
    fn multi_file_patch() {
        let p = parse("[a#AAAA]\nDEL 1.=1\n\n[b#BBBB]\nDEL 2.=2\n").unwrap();
        assert_eq!(p.sections.len(), 2);
        assert_eq!(p.sections[0].path, "a");
        assert_eq!(p.sections[1].path, "b");
    }

    #[test]
    fn block_ops_parse_but_flag_as_block() {
        let p = parse("[a#ABCD]\nSWAP.BLK 4:\n+repl\nDEL.BLK 9\nINS.BLK.POST 2:\n+tail\n").unwrap();
        assert!(p.sections[0].ops.iter().all(Op::is_block));
        assert_eq!(p.sections[0].ops.len(), 3);
    }

    #[test]
    fn tag_is_uppercased_and_validated() {
        assert_eq!(
            parse("[a#abcd]\nDEL 1.=1\n").unwrap().sections[0].tag,
            "ABCD"
        );
        assert!(matches!(
            parse("[a#XYZ]\nDEL 1.=1\n"),
            Err(HashlineError::BadSectionHeader { line: 1 })
        ));
        assert!(matches!(
            parse("[a#AB]\nDEL 1.=1\n"),
            Err(HashlineError::BadSectionHeader { line: 1 })
        ));
    }

    #[test]
    fn rejects_unknown_op_and_orphan_body() {
        assert!(matches!(
            parse("[a#ABCD]\nFROBNICATE 1\n"),
            Err(HashlineError::BadOp { line: 2, .. })
        ));
        assert!(matches!(
            parse("SWAP 1.=1:\n+x\n"),
            Err(HashlineError::BadSectionHeader { line: 1 })
        ));
    }

    #[test]
    fn crlf_patch_text_is_tolerated() {
        let p = parse("[a#ABCD]\r\nSWAP 1.=1:\r\n+x\r\n").unwrap();
        let Op::Swap { body, .. } = &p.sections[0].ops[0] else {
            panic!("expected swap");
        };
        assert_eq!(body, &["x".to_string()]);
    }
}
