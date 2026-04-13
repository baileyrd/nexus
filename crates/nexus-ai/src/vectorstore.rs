//! Vector store backed by `SQLite` for storing and searching chunk embeddings.
//!
//! Provides CRUD operations over the `embeddings` table created by schema
//! migration v4.  Similarity search loads all vectors into memory and ranks
//! them by cosine similarity -- suitable for personal knowledge-base sizes.

use rusqlite::{params, Connection};

use crate::error::AiError;

/// A chunk together with its embedding vector, ready for storage.
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
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
pub fn upsert(conn: &Connection, file_path: &str, chunks: &[ChunkEmbedding]) -> Result<(), AiError> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM embeddings WHERE file_path = ?1;", params![file_path])?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO embeddings (file_path, block_id, chunk_text, embedding, created_at)
             VALUES (?1, ?2, ?3, ?4, unixepoch());",
        )?;
        for chunk in chunks {
            let blob = embedding_to_blob(&chunk.embedding);
            stmt.execute(params![
                chunk.file_path,
                chunk.block_id as i64,
                chunk.chunk_text,
                blob,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Delete all embeddings associated with `file_path`.
pub fn delete_by_file(conn: &Connection, file_path: &str) -> Result<(), AiError> {
    conn.execute("DELETE FROM embeddings WHERE file_path = ?1;", params![file_path])?;
    Ok(())
}

/// Search for chunks most similar to `query_embedding`.
///
/// Loads all stored embeddings, computes cosine similarity against
/// `query_embedding`, and returns the top `limit` results sorted by
/// descending score.
pub fn search(
    conn: &Connection,
    query_embedding: &[f32],
    limit: usize,
) -> Result<Vec<ChunkMatch>, AiError> {
    let mut stmt = conn.prepare(
        "SELECT file_path, block_id, chunk_text, embedding FROM embeddings;",
    )?;

    let mut matches: Vec<ChunkMatch> = stmt
        .query_map([], |row| {
            let file_path: String = row.get(0)?;
            let block_id: i64 = row.get(1)?;
            let chunk_text: String = row.get(2)?;
            let blob: Vec<u8> = row.get(3)?;
            Ok((file_path, block_id as u64, chunk_text, blob))
        })?
        .filter_map(|r| r.ok())
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

    matches.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    matches.truncate(limit);

    Ok(matches)
}

/// Count the total number of stored embeddings.
pub fn count(conn: &Connection) -> Result<usize, AiError> {
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM embeddings;", [], |r| r.get(0))?;
    Ok(n as usize)
}

// ---------------------------------------------------------------------------
// Serialization helpers
// ---------------------------------------------------------------------------

/// Serialize an embedding vector to a flat little-endian byte blob.
pub fn embedding_to_blob(embedding: &[f32]) -> Vec<u8> {
    embedding.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// Deserialize a flat little-endian byte blob back into an embedding vector.
pub fn blob_to_embedding(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|chunk| {
            let bytes: [u8; 4] = chunk.try_into().expect("chunks_exact guarantees 4 bytes");
            f32::from_le_bytes(bytes)
        })
        .collect()
}

/// Compute the cosine similarity between two vectors.
///
/// Returns a value in `[-1.0, 1.0]`.  If either vector has zero magnitude
/// the function returns `0.0` to avoid division by zero.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot / (norm_a * norm_b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        nexus_storage::schema::configure_pragmas(&conn).unwrap();
        nexus_storage::schema::migrate(&conn).unwrap();
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
        let original = vec![1.0_f32, -2.5, 3.14, 0.0, f32::MAX];
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

        upsert(&conn, "a.md", &chunks).unwrap();

        // Query close to the first embedding.
        let results = search(&conn, &[0.9, 0.1, 0.0], 5).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].chunk_text, "Rust is great");
        assert!(results[0].score > 0.9);
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
        upsert(&conn, "b.md", &v1).unwrap();
        assert_eq!(count(&conn).unwrap(), 1);

        let v2 = vec![ChunkEmbedding {
            file_path: "b.md".into(),
            block_id: 1,
            chunk_text: "new".into(),
            embedding: vec![0.0, 1.0],
        }];
        upsert(&conn, "b.md", &v2).unwrap();
        assert_eq!(count(&conn).unwrap(), 1);
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
        upsert(&conn, "c.md", &chunks).unwrap();
        assert_eq!(count(&conn).unwrap(), 1);

        delete_by_file(&conn, "c.md").unwrap();
        assert_eq!(count(&conn).unwrap(), 0);
    }
}
