//! Tantivy full-text search index for nexus-storage.
//!
//! Provides BM25-scored search over block content.

use std::path::Path;
use std::sync::Mutex;

use tantivy::schema::{Field, Schema, Value, INDEXED, STORED, TEXT};
use tantivy::{doc, Index, IndexWriter, ReloadPolicy, TantivyDocument};
use tantivy::query::QueryParser;
use tantivy::collector::TopDocs;
use tantivy::snippet::SnippetGenerator;

use serde::{Deserialize, Serialize};

use crate::StorageError;

/// A single search result returned by [`SearchIndex::search`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// Path to the file containing the matching block.
    pub file_path: String,
    /// Unique ID of the matching block.
    pub block_id: u64,
    /// Type of the matching block (e.g. `"paragraph"`, `"heading"`).
    pub block_type: String,
    /// Excerpt of the matching content (empty string in M1).
    pub excerpt: String,
    /// BM25 relevance score.
    pub score: f32,
}

/// Full-text search index backed by Tantivy.
pub struct SearchIndex {
    index: Index,
    writer: Mutex<IndexWriter>,
    path_field: Field,
    block_id_field: Field,
    block_type_field: Field,
    content_field: Field,
    #[allow(dead_code)]
    mtime_field: Field,
}

fn build_schema() -> (Schema, Field, Field, Field, Field, Field) {
    let mut builder = Schema::builder();
    let path = builder.add_text_field("path", STORED);
    let block_id = builder.add_u64_field("block_id", STORED);
    let block_type = builder.add_text_field("block_type", STORED);
    let content = builder.add_text_field("content", TEXT | STORED);
    let mtime = builder.add_date_field("mtime", STORED | INDEXED);
    (builder.build(), path, block_id, block_type, content, mtime)
}

impl SearchIndex {
    /// Open or create an on-disk search index at `dir`.
    ///
    /// Creates the directory and index if it does not exist; opens the
    /// existing index otherwise.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Io`] if the directory cannot be created, or
    /// [`StorageError::Search`] if Tantivy fails to open/create the index or
    /// allocate the writer buffer.
    pub fn open(dir: &Path) -> Result<Self, StorageError> {
        let (schema, path_field, block_id_field, block_type_field, content_field, mtime_field) =
            build_schema();

        std::fs::create_dir_all(dir)?;
        let index = match Index::create_in_dir(dir, schema.clone()) {
            Ok(idx) => idx,
            Err(_) => Index::open_in_dir(dir)?,
        };

        let writer = index.writer(50_000_000)?;
        Ok(Self {
            index,
            writer: Mutex::new(writer),
            path_field,
            block_id_field,
            block_type_field,
            content_field,
            mtime_field,
        })
    }

    /// Create an in-memory search index (for tests).
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Search`] if Tantivy fails to allocate the
    /// writer buffer.
    pub fn open_in_memory() -> Result<Self, StorageError> {
        let (schema, path_field, block_id_field, block_type_field, content_field, mtime_field) =
            build_schema();

        let index = Index::create_in_ram(schema);
        let writer = index.writer(50_000_000)?;
        Ok(Self {
            index,
            writer: Mutex::new(writer),
            path_field,
            block_id_field,
            block_type_field,
            content_field,
            mtime_field,
        })
    }

    /// Add a block to the index.
    ///
    /// Call [`commit`](Self::commit) to make the document visible to searches.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Search`] if Tantivy rejects the document.
    ///
    /// # Panics
    ///
    /// Panics if the internal writer mutex is poisoned.
    pub fn add_block(
        &self,
        file_path: &str,
        block_id: u64,
        block_type: &str,
        content: &str,
    ) -> Result<(), StorageError> {
        let writer = self.writer.lock().expect("writer mutex poisoned");
        writer.add_document(doc!(
            self.path_field => file_path,
            self.block_id_field => block_id,
            self.block_type_field => block_type,
            self.content_field => content,
        ))?;
        Ok(())
    }

    /// Commit all pending adds/deletes to the index.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Search`] if Tantivy fails to flush the segment.
    ///
    /// # Panics
    ///
    /// Panics if the internal writer mutex is poisoned.
    pub fn commit(&self) -> Result<(), StorageError> {
        let mut writer = self.writer.lock().expect("writer mutex poisoned");
        writer.commit()?;
        Ok(())
    }

    /// Search the index for `query_str`, returning up to `limit` results.
    ///
    /// Results are ordered by descending BM25 score.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Search`] if the query string is malformed or
    /// Tantivy encounters an internal error during search.
    pub fn search(&self, query_str: &str, limit: usize) -> Result<Vec<SearchResult>, StorageError> {
        let reader = self
            .index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;
        reader.reload()?;

        let searcher = reader.searcher();
        let query_parser = QueryParser::for_index(&self.index, vec![self.content_field]);
        let query = query_parser
            .parse_query(query_str)
            .map_err(|e| StorageError::Search(tantivy::TantivyError::InvalidArgument(e.to_string())))?;

        let top_docs = searcher.search(&query, &TopDocs::with_limit(limit).order_by_score())?;

        // Build a snippet generator for ~150-char excerpts with matched terms
        // highlighted. We fall back to an empty excerpt if this fails (e.g. the
        // content field has no tokenizer registered).
        let snippet_gen = SnippetGenerator::create(&searcher, &*query, self.content_field).ok();

        let mut results = Vec::with_capacity(top_docs.len());
        for (score, doc_address) in top_docs {
            let doc: TantivyDocument = searcher.doc(doc_address)?;

            let file_path = doc
                .get_first(self.path_field)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();

            let block_id = doc
                .get_first(self.block_id_field)
                .and_then(|v| v.as_u64())
                .unwrap_or_default();

            let block_type = doc
                .get_first(self.block_type_field)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();

            // Generate a plain-text excerpt by stripping the HTML highlight
            // tags that SnippetGenerator adds around matched terms.
            let excerpt = snippet_gen
                .as_ref()
                .map(|gen| {
                    let html = gen.snippet_from_doc(&doc).to_html();
                    html.replace("<b>", "").replace("</b>", "")
                })
                .unwrap_or_default();

            results.push(SearchResult {
                file_path,
                block_id,
                block_type,
                excerpt,
                score,
            });
        }

        Ok(results)
    }

    /// Delete all documents from the index and commit.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Search`] if Tantivy fails to delete or commit.
    ///
    /// # Panics
    ///
    /// Panics if the internal writer mutex is poisoned.
    pub fn clear(&self) -> Result<(), StorageError> {
        let mut writer = self.writer.lock().expect("writer mutex poisoned");
        writer.delete_all_documents()?;
        writer.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn open_in_memory_succeeds() {
        let idx = SearchIndex::open_in_memory();
        assert!(idx.is_ok(), "expected Ok, got {:?}", idx.err());
    }

    #[test]
    fn add_and_search_single_block() {
        let idx = SearchIndex::open_in_memory().unwrap();
        idx.add_block("notes/test.md", 42, "paragraph", "hello world of rust programming")
            .unwrap();
        idx.commit().unwrap();

        let results = idx.search("rust", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_path, "notes/test.md");
        assert_eq!(results[0].block_id, 42);
    }

    #[test]
    fn search_returns_empty_for_no_match() {
        let idx = SearchIndex::open_in_memory().unwrap();
        idx.add_block("notes/test.md", 1, "paragraph", "hello world of rust programming")
            .unwrap();
        idx.commit().unwrap();

        let results = idx.search("python", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn search_respects_limit() {
        let idx = SearchIndex::open_in_memory().unwrap();
        for i in 0..20_u64 {
            idx.add_block(
                &format!("notes/file{i}.md"),
                i,
                "paragraph",
                "common term here",
            )
            .unwrap();
        }
        idx.commit().unwrap();

        let results = idx.search("common", 5).unwrap();
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn search_phrase_query() {
        let idx = SearchIndex::open_in_memory().unwrap();
        idx.add_block("notes/a.md", 1, "paragraph", "machine learning is great")
            .unwrap();
        idx.add_block("notes/b.md", 2, "paragraph", "learning about machines today")
            .unwrap();
        idx.commit().unwrap();

        let results = idx.search("\"machine learning\"", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_path, "notes/a.md");
    }

    #[test]
    fn clear_removes_all_documents() {
        let idx = SearchIndex::open_in_memory().unwrap();
        idx.add_block("notes/test.md", 1, "paragraph", "some content here")
            .unwrap();
        idx.commit().unwrap();

        // Verify it exists first
        let before = idx.search("content", 10).unwrap();
        assert_eq!(before.len(), 1);

        idx.clear().unwrap();

        let after = idx.search("content", 10).unwrap();
        assert!(after.is_empty());
    }

    #[test]
    fn open_on_disk_creates_directory() {
        let tmp = TempDir::new().unwrap();
        let search_dir = tmp.path().join("search_index");

        // Directory should not exist yet
        assert!(!search_dir.exists());

        // open() should create it
        let _idx = SearchIndex::open(&search_dir).unwrap();
        assert!(search_dir.exists());
    }

    #[test]
    fn search_populates_excerpt() {
        let idx = SearchIndex::open_in_memory().unwrap();
        idx.add_block(
            "notes/test.md",
            1,
            "paragraph",
            "the quick brown fox jumps over the lazy dog",
        )
        .unwrap();
        idx.commit().unwrap();

        let results = idx.search("fox", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert!(
            !results[0].excerpt.is_empty(),
            "excerpt should be non-empty when SnippetGenerator is used"
        );
        assert!(
            results[0].excerpt.contains("fox"),
            "excerpt should contain the search term"
        );
    }

    #[test]
    fn search_returns_score() {
        let idx = SearchIndex::open_in_memory().unwrap();
        // Doc with high TF for "rust"
        idx.add_block(
            "notes/heavy.md",
            1,
            "paragraph",
            "rust rust rust rust rust rust rust rust",
        )
        .unwrap();
        // Doc with low TF for "rust"
        idx.add_block("notes/light.md", 2, "paragraph", "rust programming language")
            .unwrap();
        idx.commit().unwrap();

        let results = idx.search("rust", 10).unwrap();
        assert!(results.len() >= 2, "expected at least 2 results");
        assert!(
            results[0].score >= results[1].score,
            "results should be ordered by descending score"
        );
    }
}
