//! Content chunker for splitting markdown blocks into embeddable chunks.
//!
//! Breaks parsed markdown blocks into appropriately sized pieces for
//! embedding, preserving heading context so each chunk carries its
//! structural location within the document.

/// A single chunk of text ready for embedding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// Path of the source file.
    pub file_path: String,
    /// Identifier of the originating block.
    pub block_id: u64,
    /// The textual content of this chunk.
    pub content: String,
}

/// Split parsed blocks into embedding-ready chunks.
///
/// Each block is a tuple of `(id, block_type, content, level)`.  Heading
/// blocks update the current heading context but are **not** emitted as
/// standalone chunks.  Non-heading blocks have the most recent heading
/// prepended (if any) and are split further when they exceed
/// `max_chunk_size`.  Empty blocks are silently skipped.
#[must_use] 
pub fn chunks_from_blocks(
    file_path: &str,
    blocks: &[(u64, String, String, Option<i32>)],
    max_chunk_size: usize,
) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut current_heading: Option<String> = None;

    for (id, block_type, content, _level) in blocks {
        if content.trim().is_empty() {
            continue;
        }

        if block_type == "heading" {
            current_heading = Some(content.clone());
            continue;
        }

        let text = match &current_heading {
            Some(heading) => format!("## {heading}\n\n{content}"),
            None => content.clone(),
        };

        if text.len() <= max_chunk_size {
            chunks.push(Chunk {
                file_path: file_path.to_string(),
                block_id: *id,
                content: text,
            });
        } else {
            let parts = split_on_sentences(&text, max_chunk_size);
            for part in parts {
                chunks.push(Chunk {
                    file_path: file_path.to_string(),
                    block_id: *id,
                    content: part,
                });
            }
        }
    }

    chunks
}

/// Split text at sentence boundaries (`. `) keeping pieces under `max_size`.
///
/// Uses `split_inclusive(". ")` so that each sentence retains its trailing
/// period and space.  When a single sentence exceeds `max_size` it is
/// emitted as-is rather than being truncated.
fn split_on_sentences(text: &str, max_size: usize) -> Vec<String> {
    let sentences: Vec<&str> = text.split_inclusive(". ").collect();
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();

    for sentence in sentences {
        if current.is_empty() || current.len() + sentence.len() <= max_size {
            current.push_str(sentence);
        } else {
            parts.push(current);
            current = sentence.to_string();
        }
    }

    if !current.is_empty() {
        parts.push(current);
    }

    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_block_becomes_one_chunk() {
        let blocks = vec![(1, "paragraph".into(), "Hello world.".into(), None)];
        let chunks = chunks_from_blocks("note.md", &blocks, 1000);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, "Hello world.");
        assert_eq!(chunks[0].file_path, "note.md");
        assert_eq!(chunks[0].block_id, 1);
    }

    #[test]
    fn heading_context_prepended() {
        let blocks = vec![
            (1, "heading".into(), "My Heading".into(), Some(2)),
            (2, "paragraph".into(), "Body text here.".into(), None),
        ];
        let chunks = chunks_from_blocks("note.md", &blocks, 1000);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, "## My Heading\n\nBody text here.");
    }

    #[test]
    fn oversized_block_splits() {
        let blocks = vec![(
            1,
            "paragraph".into(),
            "First sentence. Second sentence. Third sentence.".into(),
            None,
        )];
        // Set max small enough to force a split.
        let chunks = chunks_from_blocks("note.md", &blocks, 30);
        assert!(chunks.len() > 1, "expected multiple chunks, got {}", chunks.len());
        // All chunks should reference the same block.
        for c in &chunks {
            assert_eq!(c.block_id, 1);
        }
    }

    #[test]
    fn empty_blocks_skipped() {
        let blocks = vec![
            (1, "paragraph".into(), String::new(), None),
            (2, "paragraph".into(), "   ".into(), None),
            (3, "paragraph".into(), "Real content.".into(), None),
        ];
        let chunks = chunks_from_blocks("note.md", &blocks, 1000);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].block_id, 3);
    }
}
