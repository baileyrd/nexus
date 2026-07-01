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

use rusqlite::{params, Connection};
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
            "INSERT INTO embeddings (namespace, file_path, block_id, chunk_text, embedding, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, unixepoch());",
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
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
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
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
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
            },
            ChunkEmbedding {
                file_path: "a.md".into(),
                block_id: 2,
                chunk_text: "Python is nice".into(),
                embedding: vec![0.0, 1.0, 0.0],
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
        }];
        upsert(&conn, "notes", "b.md", &v1).unwrap();
        assert_eq!(count(&conn, "notes").unwrap(), 1);

        let v2 = vec![ChunkEmbedding {
            file_path: "b.md".into(),
            block_id: 1,
            chunk_text: "new".into(),
            embedding: vec![0.0, 1.0],
        }];
        upsert(&conn, "notes", "b.md", &v2).unwrap();
        assert_eq!(count(&conn, "notes").unwrap(), 1);
    }

    #[test]
    fn delete_by_file_removes_embeddings() {
        let conn = setup_db();

        let chunks = vec![ChunkEmbedding {
            file_path: "c.md".into(),
            block_id: 1,
            chunk_text: "data".into(),
            embedding: vec![1.0],
        }];
        upsert(&conn, "notes", "c.md", &chunks).unwrap();
        assert_eq!(count(&conn, "notes").unwrap(), 1);

        delete_by_file(&conn, "notes", "c.md").unwrap();
        assert_eq!(count(&conn, "notes").unwrap(), 0);
    }
}
