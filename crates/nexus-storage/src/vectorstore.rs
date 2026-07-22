//! Vector store backed by `SQLite` for chunk embeddings.
//!
//! Provides CRUD over the `embeddings` table created by schema migration v4.
//! The AI plugin does **not** open its own `SQLite` connection; instead it
//! reaches these operations through storage IPC handlers
//! (`vector_insert`, `vector_query`, `vector_delete_by_file`,
//! `vectorstore_count`) so that storage remains the sole owner of the forge
//! database.
//!
//! Similarity search loads all vectors into memory and ranks them by cosine
//! similarity — appropriate for personal-knowledge-base sizes.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::StorageError;

/// A chunk together with its embedding vector, ready for storage.
///
/// `Serialize`/`Deserialize` so it can round-trip through the IPC layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkEmbedding {
    /// Path of the source file.
    pub file_path: String,
    /// Identifier of the originating block.
    pub block_id: u64,
    /// The textual content of the chunk.
    pub chunk_text: String,
    /// Dense vector representation of the chunk.
    pub embedding: Vec<f32>,
    /// C19 (#372) — hash of the source file's content as of this embed
    /// pass. Every chunk from one `upsert` call shares the same value.
    /// `None` for callers that don't opt into the skip-unchanged-files
    /// optimisation (the row is then always re-embedded, matching
    /// pre-#372 behaviour).
    #[serde(default)]
    pub content_hash: Option<String>,
}

/// A search result returned by [`search`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkMatch {
    /// Path of the source file.
    pub file_path: String,
    /// Identifier of the originating block.
    pub block_id: u64,
    /// The textual content of the chunk.
    pub chunk_text: String,
    /// Cosine similarity score (higher is more relevant).
    pub score: f32,
}

/// Replace all embeddings for `file_path` with the given chunks.
///
/// Deletes any existing rows for the file and inserts the new set inside
/// a single transaction.
///
/// # Errors
///
/// Returns [`StorageError::Database`] if the transaction, delete, or insert
/// fails.
pub fn upsert(
    conn: &Connection,
    namespace: &str,
    file_path: &str,
    chunks: &[ChunkEmbedding],
) -> Result<(), StorageError> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM embeddings WHERE namespace = ?1 AND file_path = ?2;",
        params![namespace, file_path],
    )?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO embeddings (namespace, file_path, block_id, chunk_text, embedding, content_hash, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, unixepoch());",
        )?;
        for chunk in chunks {
            let blob = embedding_to_blob(&chunk.embedding);
            #[allow(clippy::cast_possible_wrap)]
            let block_id = chunk.block_id as i64;
            stmt.execute(params![
                namespace,
                chunk.file_path,
                block_id,
                chunk.chunk_text,
                blob,
                chunk.content_hash,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// C19 (#372) — the content hash + embedding dimensionality already
/// stored for `file_path`, read from one arbitrary existing chunk row
/// (every chunk from the same `upsert` call shares the same
/// `content_hash`). `None` when nothing is stored yet, or the stored
/// rows predate this feature (`content_hash IS NULL`) — both cases mean
/// "always re-embed".
///
/// The dimension is derived from the stored blob's byte length rather
/// than tracked in a separate column, so a provider/model switch that
/// changes the embedding dimensionality is automatically treated as a
/// content mismatch by the caller (comparing against the *current*
/// provider's dimension) even though the file's own content didn't
/// change.
///
/// # Errors
///
/// Returns [`StorageError`] if the underlying query fails.
pub fn stored_signature(
    conn: &Connection,
    namespace: &str,
    file_path: &str,
) -> Result<Option<(String, usize)>, StorageError> {
    let result = conn
        .query_row(
            "SELECT content_hash, embedding FROM embeddings
             WHERE namespace = ?1 AND file_path = ?2 AND content_hash IS NOT NULL
             LIMIT 1;",
            params![namespace, file_path],
            |row| {
                let hash: String = row.get(0)?;
                let blob: Vec<u8> = row.get(1)?;
                Ok((hash, blob.len() / 4))
            },
        )
        .optional()?;
    Ok(result)
}

/// Delete all embeddings associated with `file_path` within `namespace`.
///
/// # Errors
///
/// Returns [`StorageError::Database`] if the delete statement fails.
pub fn delete_by_file(
    conn: &Connection,
    namespace: &str,
    file_path: &str,
) -> Result<(), StorageError> {
    conn.execute(
        "DELETE FROM embeddings WHERE namespace = ?1 AND file_path = ?2;",
        params![namespace, file_path],
    )?;
    Ok(())
}

/// Search for chunks most similar to `query_embedding`.
///
/// Loads all stored embeddings, computes cosine similarity against
/// `query_embedding`, and returns the top `limit` results sorted by
/// descending score.
///
/// # Errors
///
/// Returns [`StorageError::Database`] if the underlying query fails.
pub fn search(
    conn: &Connection,
    namespace: &str,
    query_embedding: &[f32],
    limit: usize,
) -> Result<Vec<ChunkMatch>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT file_path, block_id, chunk_text, embedding FROM embeddings WHERE namespace = ?1;",
    )?;

    let mut matches: Vec<ChunkMatch> = stmt
        .query_map(params![namespace], |row| {
            let file_path: String = row.get(0)?;
            let block_id: i64 = row.get(1)?;
            let chunk_text: String = row.get(2)?;
            let blob: Vec<u8> = row.get(3)?;
            Ok((file_path, block_id.cast_unsigned(), chunk_text, blob))
        })?
        .filter_map(std::result::Result::ok)
        .map(|(file_path, block_id, chunk_text, blob)| {
            let emb = blob_to_embedding(&blob);
            let score = cosine_similarity(query_embedding, &emb);
            ChunkMatch {
                file_path,
                block_id,
                chunk_text,
                score,
            }
        })
        .collect();

    matches.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    matches.truncate(limit);

    Ok(matches)
}

/// Count the total number of stored embeddings.
///
/// # Errors
///
/// Returns [`StorageError::Database`] if the count query fails.
pub fn count(conn: &Connection, namespace: &str) -> Result<usize, StorageError> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM embeddings WHERE namespace = ?1;",
        params![namespace],
        |r| r.get(0),
    )?;
    usize::try_from(n).map_err(|_| StorageError::IndexInconsistency {
        details: "embedding count overflowed usize".into(),
    })
}

/// Mean-pool every chunk embedding for each file in `namespace` into a
/// single per-file vector — one row per file, suitable for whole-note
/// similarity comparisons (C23 / #376 near-duplicate note detection).
///
/// Chunks whose dimensionality doesn't match the running mean for their
/// file (e.g. leftover rows from a prior embedding model) are skipped
/// rather than erroring, so a stale row can't poison a whole file's
/// vector. Files with zero eligible chunks are omitted from the result.
///
/// # Errors
///
/// Returns [`StorageError`] if the underlying query fails.
pub fn mean_embeddings_by_file(
    conn: &Connection,
    namespace: &str,
) -> Result<Vec<(String, Vec<f32>)>, StorageError> {
    let mut stmt =
        conn.prepare("SELECT file_path, embedding FROM embeddings WHERE namespace = ?1;")?;
    let rows: Vec<(String, Vec<f32>)> = stmt
        .query_map(params![namespace], |row| {
            let file_path: String = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            Ok((file_path, blob_to_embedding(&blob)))
        })?
        .filter_map(std::result::Result::ok)
        .collect();

    let mut by_file: std::collections::BTreeMap<String, (Vec<f32>, usize)> =
        std::collections::BTreeMap::new();
    for (file_path, emb) in rows {
        let entry = by_file
            .entry(file_path)
            .or_insert_with(|| (vec![0.0; emb.len()], 0));
        if entry.0.len() != emb.len() {
            continue;
        }
        for (acc, v) in entry.0.iter_mut().zip(emb.iter()) {
            *acc += v;
        }
        entry.1 += 1;
    }

    Ok(by_file
        .into_iter()
        .filter(|(_, (_, count))| *count > 0)
        .map(|(path, (sum, count))| {
            #[allow(clippy::cast_precision_loss)]
            let divisor = count as f32;
            let mean: Vec<f32> = sum.iter().map(|v| v / divisor).collect();
            (path, mean)
        })
        .collect())
}

// ─── Serialization helpers ───────────────────────────────────────────────────

/// Serialize an embedding vector to a flat little-endian byte blob.
fn embedding_to_blob(embedding: &[f32]) -> Vec<u8> {
    embedding.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// Deserialize a flat little-endian byte blob back into an embedding vector.
fn blob_to_embedding(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|chunk| {
            let bytes: [u8; 4] = chunk.try_into().expect("chunks_exact guarantees 4 bytes");
            f32::from_le_bytes(bytes)
        })
        .collect()
}

/// Cosine similarity between two vectors, clamped to `[-1.0, 1.0]`.
///
/// Returns `0.0` when either vector has zero magnitude.
pub(crate) fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot / (norm_a * norm_b)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::schema::configure_pragmas(&conn).unwrap();
        crate::schema::migrate(&conn).unwrap();
        conn
    }

    #[test]
    fn cosine_similarity_identical_vectors() {
        let v = vec![1.0, 2.0, 3.0];
        let score = cosine_similarity(&v, &v);
        assert!((score - 1.0).abs() < 1e-6, "expected ~1.0, got {score}");
    }

    #[test]
    fn cosine_similarity_orthogonal_vectors() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let score = cosine_similarity(&a, &b);
        assert!(score.abs() < 1e-6, "expected ~0.0, got {score}");
    }

    #[test]
    fn embedding_blob_round_trip() {
        let original = vec![1.0_f32, -2.5, 3.15, 0.0, f32::MAX];
        let blob = embedding_to_blob(&original);
        let restored = blob_to_embedding(&blob);
        assert_eq!(original, restored);
    }

    #[test]
    fn upsert_and_search() {
        let conn = setup_db();

        let chunks = vec![
            ChunkEmbedding {
                file_path: "a.md".into(),
                block_id: 1,
                chunk_text: "Rust is great".into(),
                embedding: vec![1.0, 0.0, 0.0],
                content_hash: None,
            },
            ChunkEmbedding {
                file_path: "a.md".into(),
                block_id: 2,
                chunk_text: "Python is nice".into(),
                embedding: vec![0.0, 1.0, 0.0],
                content_hash: None,
            },
        ];

        upsert(&conn, "notes", "a.md", &chunks).unwrap();

        let results = search(&conn, "notes", &[0.9, 0.1, 0.0], 5).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].chunk_text, "Rust is great");
        assert!(results[0].score > 0.9);
    }

    #[test]
    fn namespaces_are_isolated() {
        let conn = setup_db();
        upsert(
            &conn,
            "notes",
            "a.md",
            &[ChunkEmbedding {
                file_path: "a.md".into(),
                block_id: 1,
                chunk_text: "a note".into(),
                embedding: vec![1.0, 0.0],
                content_hash: None,
            }],
        )
        .unwrap();
        upsert(
            &conn,
            "memory",
            "memory://x",
            &[ChunkEmbedding {
                file_path: "memory://x".into(),
                block_id: 0,
                chunk_text: "a memory".into(),
                embedding: vec![1.0, 0.0],
                content_hash: None,
            }],
        )
        .unwrap();

        // Each namespace sees only its own rows.
        assert_eq!(count(&conn, "notes").unwrap(), 1);
        assert_eq!(count(&conn, "memory").unwrap(), 1);
        let notes = search(&conn, "notes", &[1.0, 0.0], 5).unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].chunk_text, "a note");
        let mem = search(&conn, "memory", &[1.0, 0.0], 5).unwrap();
        assert_eq!(mem.len(), 1);
        assert_eq!(mem[0].chunk_text, "a memory");

        // Deleting in one namespace leaves the other intact.
        delete_by_file(&conn, "memory", "memory://x").unwrap();
        assert_eq!(count(&conn, "memory").unwrap(), 0);
        assert_eq!(count(&conn, "notes").unwrap(), 1);
    }

    #[test]
    fn upsert_replaces_existing() {
        let conn = setup_db();

        let v1 = vec![ChunkEmbedding {
            file_path: "b.md".into(),
            block_id: 1,
            chunk_text: "old".into(),
            embedding: vec![1.0, 0.0],
            content_hash: None,
        }];
        upsert(&conn, "notes", "b.md", &v1).unwrap();
        assert_eq!(count(&conn, "notes").unwrap(), 1);

        let v2 = vec![ChunkEmbedding {
            file_path: "b.md".into(),
            block_id: 1,
            chunk_text: "new".into(),
            embedding: vec![0.0, 1.0],
            content_hash: None,
        }];
        upsert(&conn, "notes", "b.md", &v2).unwrap();
        assert_eq!(count(&conn, "notes").unwrap(), 1);
    }

    #[test]
    fn mean_embeddings_by_file_pools_chunks_per_file() {
        let conn = setup_db();
        upsert(
            &conn,
            "notes",
            "a.md",
            &[
                ChunkEmbedding {
                    file_path: "a.md".into(),
                    block_id: 1,
                    chunk_text: "one".into(),
                    embedding: vec![1.0, 0.0],
                    content_hash: None,
                },
                ChunkEmbedding {
                    file_path: "a.md".into(),
                    block_id: 2,
                    chunk_text: "two".into(),
                    embedding: vec![0.0, 1.0],
                    content_hash: None,
                },
            ],
        )
        .unwrap();
        upsert(
            &conn,
            "notes",
            "b.md",
            &[ChunkEmbedding {
                file_path: "b.md".into(),
                block_id: 1,
                chunk_text: "solo".into(),
                embedding: vec![2.0, 2.0],
                content_hash: None,
            }],
        )
        .unwrap();

        let means = mean_embeddings_by_file(&conn, "notes").unwrap();
        assert_eq!(means.len(), 2);
        let a = means.iter().find(|(p, _)| p == "a.md").unwrap();
        assert!((a.1[0] - 0.5).abs() < 1e-6);
        assert!((a.1[1] - 0.5).abs() < 1e-6);
        let b = means.iter().find(|(p, _)| p == "b.md").unwrap();
        assert!((b.1[0] - 2.0).abs() < 1e-6);
        assert!((b.1[1] - 2.0).abs() < 1e-6);
    }

    #[test]
    fn mean_embeddings_by_file_skips_dimension_mismatch_chunks() {
        let conn = setup_db();
        // Simulate a stale row from a different embedding model by
        // inserting directly with a mismatched dimensionality.
        upsert(
            &conn,
            "notes",
            "c.md",
            &[ChunkEmbedding {
                file_path: "c.md".into(),
                block_id: 1,
                chunk_text: "first".into(),
                embedding: vec![1.0, 0.0, 0.0],
                content_hash: None,
            }],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO embeddings (namespace, file_path, block_id, chunk_text, embedding, created_at)
             VALUES ('notes', 'c.md', 2, 'mismatched', ?1, unixepoch());",
            params![embedding_to_blob(&[1.0, 0.0])],
        )
        .unwrap();

        let means = mean_embeddings_by_file(&conn, "notes").unwrap();
        assert_eq!(means.len(), 1);
        assert_eq!(means[0].0, "c.md");
        assert_eq!(means[0].1, vec![1.0, 0.0, 0.0]);
    }

    #[test]
    fn delete_by_file_removes_embeddings() {
        let conn = setup_db();

        let chunks = vec![ChunkEmbedding {
            file_path: "c.md".into(),
            block_id: 1,
            chunk_text: "data".into(),
            embedding: vec![1.0],
            content_hash: None,
        }];
        upsert(&conn, "notes", "c.md", &chunks).unwrap();
        assert_eq!(count(&conn, "notes").unwrap(), 1);

        delete_by_file(&conn, "notes", "c.md").unwrap();
        assert_eq!(count(&conn, "notes").unwrap(), 0);
    }

    // ── C19 (#372) — stored_signature ────────────────────────────────

    #[test]
    fn stored_signature_returns_none_when_nothing_stored() {
        let conn = setup_db();
        assert_eq!(stored_signature(&conn, "notes", "missing.md").unwrap(), None);
    }

    #[test]
    fn stored_signature_returns_hash_and_dimension_after_upsert() {
        let conn = setup_db();
        upsert(
            &conn,
            "notes",
            "d.md",
            &[ChunkEmbedding {
                file_path: "d.md".into(),
                block_id: 1,
                chunk_text: "hello".into(),
                embedding: vec![1.0, 2.0, 3.0],
                content_hash: Some("abc123".into()),
            }],
        )
        .unwrap();

        let sig = stored_signature(&conn, "notes", "d.md").unwrap();
        assert_eq!(sig, Some(("abc123".to_string(), 3)));
    }

    #[test]
    fn stored_signature_is_none_for_rows_predating_the_feature() {
        // A row with content_hash left NULL (e.g. from before migration
        // 011, or a caller that opted out) must read back as "unknown",
        // not as an empty-string match.
        let conn = setup_db();
        upsert(
            &conn,
            "notes",
            "e.md",
            &[ChunkEmbedding {
                file_path: "e.md".into(),
                block_id: 1,
                chunk_text: "hello".into(),
                embedding: vec![1.0],
                content_hash: None,
            }],
        )
        .unwrap();

        assert_eq!(stored_signature(&conn, "notes", "e.md").unwrap(), None);
    }

    #[test]
    fn stored_signature_is_scoped_to_namespace() {
        let conn = setup_db();
        upsert(
            &conn,
            "notes",
            "f.md",
            &[ChunkEmbedding {
                file_path: "f.md".into(),
                block_id: 1,
                chunk_text: "hello".into(),
                embedding: vec![1.0, 2.0],
                content_hash: Some("notes-hash".into()),
            }],
        )
        .unwrap();

        assert_eq!(stored_signature(&conn, "memory", "f.md").unwrap(), None);
    }
}
