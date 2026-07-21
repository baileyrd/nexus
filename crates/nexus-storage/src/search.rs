//! Tantivy full-text search index for nexus-storage.
//!
//! Provides BM25-scored search over block content.

use std::path::Path;
use std::sync::Mutex;

use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, Schema, Value, FAST, INDEXED, STORED, TEXT};
use tantivy::snippet::SnippetGenerator;
use tantivy::{
    doc, DateTime as TantivyDateTime, DocAddress, Index, IndexReader, IndexWriter, Order,
    ReloadPolicy, TantivyDocument,
};

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
    /// BM25 relevance score. `0.0` when [`SearchOptions::sort`] is
    /// [`SearchSort::MtimeDesc`]/[`SearchSort::MtimeAsc`] — those modes
    /// don't rank by score, so the value would be meaningless.
    pub score: f32,
    /// #375 — the block's file mtime, Unix seconds (same clock as
    /// `files.modified_at`; see [`SearchIndex::add_block`]'s doc
    /// comment for what "mtime" means here). `0` only if the document
    /// somehow predates mtime being written (shouldn't happen post
    /// reindex).
    pub mtime: i64,
}

/// Sort order for [`SearchIndex::search`]. #375.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchSort {
    /// BM25 relevance, descending score (the historical/only behavior).
    #[default]
    Relevance,
    /// Most-recently-modified block first.
    MtimeDesc,
    /// Least-recently-modified block first.
    MtimeAsc,
}

/// Optional paging / sort / date-range knobs for [`SearchIndex::search`].
/// #375 — bundled into one struct rather than growing `search`'s
/// positional argument list every time a new knob is added.
#[derive(Debug, Clone, Copy, Default)]
pub struct SearchOptions {
    /// Skip this many ranked hits before taking the page of `limit`.
    pub offset: usize,
    /// How to rank/order hits.
    pub sort: SearchSort,
    /// Only include blocks whose file mtime is on or after this
    /// Unix-seconds timestamp.
    pub mtime_after: Option<i64>,
    /// Only include blocks whose file mtime is on or before this
    /// Unix-seconds timestamp.
    pub mtime_before: Option<i64>,
}

impl SearchOptions {
    fn has_date_filter(&self) -> bool {
        self.mtime_after.is_some() || self.mtime_before.is_some()
    }

    fn passes_date_filter(&self, mtime: i64) -> bool {
        if self.mtime_after.is_some_and(|after| mtime < after) {
            return false;
        }
        if self.mtime_before.is_some_and(|before| mtime > before) {
            return false;
        }
        true
    }
}

/// Full-text search index backed by Tantivy.
pub struct SearchIndex {
    index: Index,
    writer: Mutex<IndexWriter>,
    /// #192 / R9 — long-lived reader. Tantivy's `IndexReader` is the
    /// expensive handle (it holds the segment cache); a `Searcher` is
    /// cheap to derive per query via `reader.searcher()`. We build the
    /// reader once at `open()` and call `reload()` after each writer
    /// commit so newly-added blocks become visible. Previously the
    /// reader was rebuilt inside `search()` on every call.
    reader: IndexReader,
    path_field: Field,
    block_id_field: Field,
    block_type_field: Field,
    content_field: Field,
    /// #375 — `STORED | INDEXED | FAST`: `FAST` is required for
    /// `TopDocs::order_by_fast_field` (sort-by-mtime); `INDEXED` backs
    /// the `mtime_after`/`mtime_before` range filter; `STORED` lets
    /// `search()` read the value back for [`SearchResult::mtime`].
    mtime_field: Field,
}

fn build_schema() -> (Schema, Field, Field, Field, Field, Field) {
    let mut builder = Schema::builder();
    let path = builder.add_text_field("path", STORED);
    let block_id = builder.add_u64_field("block_id", STORED);
    let block_type = builder.add_text_field("block_type", STORED);
    let content = builder.add_text_field("content", TEXT | STORED);
    let mtime = builder.add_date_field("mtime", STORED | INDEXED | FAST);
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
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;
        Ok(Self {
            index,
            writer: Mutex::new(writer),
            reader,
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
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;
        Ok(Self {
            index,
            writer: Mutex::new(writer),
            reader,
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
    /// `mtime` is Unix seconds — the same value stored in
    /// `files.modified_at`, which today is the time the file was last
    /// (re)indexed rather than a true filesystem mtime stat (`reconcile.rs`
    /// doesn't read `fs::Metadata::modified()`). That's an acceptable proxy
    /// for sort/filter purposes since it lags real mtime by at most one
    /// reconcile cycle, but callers relying on this for anything more
    /// precise should be aware.
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
        mtime: i64,
    ) -> Result<(), StorageError> {
        let writer = self.writer.lock().expect("writer mutex poisoned");
        writer.add_document(doc!(
            self.path_field => file_path,
            self.block_id_field => block_id,
            self.block_type_field => block_type,
            self.content_field => content,
            self.mtime_field => TantivyDateTime::from_timestamp_secs(mtime),
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
        // #192 — reload the cached reader so subsequent `search()`
        // calls see the new segments without rebuilding the reader on
        // every query.
        self.reader.reload()?;
        Ok(())
    }

    /// Search the index for `query_str`, returning up to `limit` results
    /// ordered by descending BM25 score. Shorthand for
    /// [`Self::search_with_options`] with [`SearchOptions::default`].
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Search`] if the query string is malformed or
    /// Tantivy encounters an internal error during search.
    pub fn search(&self, query_str: &str, limit: usize) -> Result<Vec<SearchResult>, StorageError> {
        self.search_with_options(query_str, limit, SearchOptions::default())
    }

    /// Search the index for `query_str`, with paging (`options.offset`),
    /// an alternate sort (`options.sort`), and/or an mtime range filter
    /// (`options.mtime_after` / `options.mtime_before`). #375.
    ///
    /// The date filter is applied *after* ranking, on the same
    /// already-ranked candidate window Tantivy returns — a hit outside
    /// the mtime window doesn't get backfilled from beyond that window.
    /// This mirrors the existing trade-off in `search_scope.rs`'s
    /// scoped-query post-filter (documented there as "Scoped BM25
    /// search + `SQLite` post-filter"). To keep a filtered page from
    /// coming back smaller than `limit` in the common case, the
    /// candidate window is widened 4x when a date filter is active.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Search`] if the query string is malformed or
    /// Tantivy encounters an internal error during search.
    pub fn search_with_options(
        &self,
        query_str: &str,
        limit: usize,
        options: SearchOptions,
    ) -> Result<Vec<SearchResult>, StorageError> {
        // #192 — use the long-lived reader cached on `self`. `commit()`
        // calls `reload()` so newly-written segments are visible
        // without rebuilding the reader on every query.
        let searcher = self.reader.searcher();
        let query_parser = QueryParser::for_index(&self.index, vec![self.content_field]);
        let query = query_parser.parse_query(query_str).map_err(|e| {
            StorageError::Search(tantivy::TantivyError::InvalidArgument(e.to_string()))
        })?;

        let window = options.offset.saturating_add(limit);
        let fetch_limit = if options.has_date_filter() {
            window.saturating_mul(4).max(window)
        } else {
            window
        };

        // Both branches produce the same shape — (score, DocAddress) —
        // but only the relevance branch has a real score; the
        // mtime-sort branches report 0.0 (see `SearchResult::score`'s
        // doc comment) since ranking there isn't by score at all.
        let ranked: Vec<(f32, DocAddress)> = match options.sort {
            SearchSort::Relevance => {
                searcher.search(&query, &TopDocs::with_limit(fetch_limit).order_by_score())?
            }
            SearchSort::MtimeDesc | SearchSort::MtimeAsc => {
                let order = if options.sort == SearchSort::MtimeAsc {
                    Order::Asc
                } else {
                    Order::Desc
                };
                searcher
                    .search(
                        &query,
                        &TopDocs::with_limit(fetch_limit)
                            .order_by_fast_field::<TantivyDateTime>("mtime", order),
                    )?
                    .into_iter()
                    .map(|(_mtime, addr)| (0.0, addr))
                    .collect()
            }
        };

        // Build a snippet generator for ~150-char excerpts with matched terms
        // highlighted. We fall back to an empty excerpt if this fails (e.g. the
        // content field has no tokenizer registered).
        let snippet_gen = SnippetGenerator::create(&searcher, &*query, self.content_field).ok();

        let mut results = Vec::with_capacity(limit);
        for (score, doc_address) in ranked {
            let doc: TantivyDocument = searcher.doc(doc_address)?;

            let mtime = doc
                .get_first(self.mtime_field)
                .and_then(|v| v.as_datetime())
                .map(TantivyDateTime::into_timestamp_secs)
                .unwrap_or_default();

            if !options.passes_date_filter(mtime) {
                continue;
            }

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
                mtime,
            });
        }

        // `ranked` is already in final rank order (relevance or mtime);
        // apply the page window last so offset/limit behave the same
        // whether or not a date filter dropped candidates along the way.
        let paged = results
            .into_iter()
            .skip(options.offset)
            .take(limit)
            .collect();
        Ok(paged)
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
        // #192 — reload so subsequent searches see the empty segment
        // set. Mirrors the reload in `commit()`.
        self.reader.reload()?;
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
        idx.add_block(
            "notes/test.md",
            42,
            "paragraph",
            "hello world of rust programming",
            1_700_000_000,
        )
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
        idx.add_block(
            "notes/test.md",
            1,
            "paragraph",
            "hello world of rust programming",
            1_700_000_000,
        )
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
                1_700_000_000,
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
        idx.add_block(
            "notes/a.md",
            1,
            "paragraph",
            "machine learning is great",
            1_700_000_000,
        )
        .unwrap();
        idx.add_block(
            "notes/b.md",
            2,
            "paragraph",
            "learning about machines today",
            1_700_000_000,
        )
        .unwrap();
        idx.commit().unwrap();

        let results = idx.search("\"machine learning\"", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_path, "notes/a.md");
    }

    #[test]
    fn clear_removes_all_documents() {
        let idx = SearchIndex::open_in_memory().unwrap();
        idx.add_block(
            "notes/test.md",
            1,
            "paragraph",
            "some content here",
            1_700_000_000,
        )
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
            1_700_000_000,
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
            1_700_000_000,
        )
        .unwrap();
        // Doc with low TF for "rust"
        idx.add_block(
            "notes/light.md",
            2,
            "paragraph",
            "rust programming language",
            1_700_000_000,
        )
        .unwrap();
        idx.commit().unwrap();

        let results = idx.search("rust", 10).unwrap();
        assert!(results.len() >= 2, "expected at least 2 results");
        assert!(
            results[0].score >= results[1].score,
            "results should be ordered by descending score"
        );
    }

    // ── #375 — mtime population, offset, sort, date filter ──────────────────

    #[test]
    fn add_block_populates_mtime_on_search_results() {
        let idx = SearchIndex::open_in_memory().unwrap();
        idx.add_block("notes/a.md", 1, "paragraph", "unique_term_alpha", 1_650_000_000)
            .unwrap();
        idx.commit().unwrap();

        let results = idx.search("unique_term_alpha", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].mtime, 1_650_000_000);
    }

    #[test]
    fn search_with_options_offset_pages_through_relevance_order() {
        let idx = SearchIndex::open_in_memory().unwrap();
        for i in 0..5_u64 {
            idx.add_block(
                &format!("notes/file{i}.md"),
                i,
                "paragraph",
                "common_term_beta",
                1_700_000_000 + i.cast_signed(),
            )
            .unwrap();
        }
        idx.commit().unwrap();

        let page1 = idx
            .search_with_options("common_term_beta", 2, SearchOptions::default())
            .unwrap();
        let page2 = idx
            .search_with_options(
                "common_term_beta",
                2,
                SearchOptions {
                    offset: 2,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(page1.len(), 2);
        assert_eq!(page2.len(), 2);
        // Pages shouldn't overlap.
        let page1_ids: Vec<u64> = page1.iter().map(|r| r.block_id).collect();
        let page2_ids: Vec<u64> = page2.iter().map(|r| r.block_id).collect();
        assert!(
            page1_ids.iter().all(|id| !page2_ids.contains(id)),
            "page1 {page1_ids:?} and page2 {page2_ids:?} must not overlap"
        );
    }

    #[test]
    fn search_with_options_sorts_by_mtime_descending() {
        let idx = SearchIndex::open_in_memory().unwrap();
        idx.add_block("notes/old.md", 1, "paragraph", "sort_term_gamma", 1_000)
            .unwrap();
        idx.add_block("notes/new.md", 2, "paragraph", "sort_term_gamma", 2_000)
            .unwrap();
        idx.add_block("notes/mid.md", 3, "paragraph", "sort_term_gamma", 1_500)
            .unwrap();
        idx.commit().unwrap();

        let results = idx
            .search_with_options(
                "sort_term_gamma",
                10,
                SearchOptions {
                    sort: SearchSort::MtimeDesc,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(
            results.iter().map(|r| r.file_path.clone()).collect::<Vec<_>>(),
            vec!["notes/new.md", "notes/mid.md", "notes/old.md"],
        );
    }

    #[test]
    fn search_with_options_sorts_by_mtime_ascending() {
        let idx = SearchIndex::open_in_memory().unwrap();
        idx.add_block("notes/old.md", 1, "paragraph", "sort_term_delta", 1_000)
            .unwrap();
        idx.add_block("notes/new.md", 2, "paragraph", "sort_term_delta", 2_000)
            .unwrap();
        idx.commit().unwrap();

        let results = idx
            .search_with_options(
                "sort_term_delta",
                10,
                SearchOptions {
                    sort: SearchSort::MtimeAsc,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(
            results.iter().map(|r| r.file_path.clone()).collect::<Vec<_>>(),
            vec!["notes/old.md", "notes/new.md"],
        );
    }

    #[test]
    fn search_with_options_filters_by_mtime_range() {
        let idx = SearchIndex::open_in_memory().unwrap();
        idx.add_block("notes/early.md", 1, "paragraph", "range_term_epsilon", 1_000)
            .unwrap();
        idx.add_block("notes/mid.md", 2, "paragraph", "range_term_epsilon", 2_000)
            .unwrap();
        idx.add_block("notes/late.md", 3, "paragraph", "range_term_epsilon", 3_000)
            .unwrap();
        idx.commit().unwrap();

        let results = idx
            .search_with_options(
                "range_term_epsilon",
                10,
                SearchOptions {
                    mtime_after: Some(1_500),
                    mtime_before: Some(2_500),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_path, "notes/mid.md");
    }

    #[test]
    fn search_with_options_mtime_after_alone_is_inclusive() {
        let idx = SearchIndex::open_in_memory().unwrap();
        idx.add_block("notes/exact.md", 1, "paragraph", "range_term_zeta", 5_000)
            .unwrap();
        idx.commit().unwrap();

        let results = idx
            .search_with_options(
                "range_term_zeta",
                10,
                SearchOptions {
                    mtime_after: Some(5_000),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(results.len(), 1, "mtime_after should be inclusive of an exact match");
    }
}
