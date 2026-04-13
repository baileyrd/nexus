//! Task extraction, storage, and file-writeback operations.

/// A task item parsed from a markdown checkbox list.
#[derive(Debug, Clone)]
pub struct ParsedTask {
    /// Task text without the checkbox prefix.
    pub content: String,
    /// Whether the checkbox is checked (`[x]`).
    pub completed: bool,
    /// 1-indexed line number in the source file.
    pub line_number: u32,
}
