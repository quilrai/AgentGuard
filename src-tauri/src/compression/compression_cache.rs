// Content-addressed compression cache backed by SQLite.
//
// Maps content hashes to their compressed versions so repeated tool
// outputs (e.g. same grep query, same file read) skip re-compression.
// No eviction — the table grows freely; old entries can be pruned by
// a future cleanup job if needed.

use rusqlite::Connection;

/// Compute a truncated MD5 hex hash for content (16 hex chars).
pub fn content_hash(content: &str) -> String {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(content.as_bytes());
    let result = hasher.finalize();
    hex::encode(&result[..8]) // 16 hex chars
}

/// Ensure the compression_cache table exists.
fn ensure_table(conn: &Connection) {
    let _ = conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS compression_cache (
            hash TEXT PRIMARY KEY,
            compressed TEXT NOT NULL,
            tokens_saved INTEGER DEFAULT 0,
            created_at TEXT NOT NULL,
            hit_count INTEGER DEFAULT 0
        )",
    );
}

/// Look up a cached compressed version by content hash.
/// Returns the compressed text if found, and bumps hit_count.
pub fn get_compressed(db_path: &str, hash: &str) -> Option<String> {
    let conn = Connection::open(db_path).ok()?;
    ensure_table(&conn);

    let result: Option<String> = conn
        .query_row(
            "SELECT compressed FROM compression_cache WHERE hash = ?1",
            rusqlite::params![hash],
            |row| row.get(0),
        )
        .ok();

    if result.is_some() {
        let _ = conn.execute(
            "UPDATE compression_cache SET hit_count = hit_count + 1 WHERE hash = ?1",
            rusqlite::params![hash],
        );
    }

    result
}

/// Store a compressed version keyed by content hash.
/// If the hash already exists, the entry is overwritten.
pub fn store_compressed(db_path: &str, hash: &str, compressed: &str, tokens_saved: i64) {
    let conn = match Connection::open(db_path) {
        Ok(c) => c,
        Err(_) => return,
    };
    ensure_table(&conn);

    let created_at = chrono::Utc::now().to_rfc3339();
    let _ = conn.execute(
        "INSERT OR REPLACE INTO compression_cache (hash, compressed, tokens_saved, created_at, hit_count)
         VALUES (?1, ?2, ?3, ?4, 0)",
        rusqlite::params![hash, compressed, tokens_saved, created_at],
    );
}

/// Return cache statistics.
pub struct CacheStats {
    pub entries: usize,
    pub total_tokens_saved: i64,
    pub total_hits: i64,
}

pub fn get_stats(db_path: &str) -> Option<CacheStats> {
    let conn = Connection::open(db_path).ok()?;
    ensure_table(&conn);

    let (entries, total_tokens_saved, total_hits): (i64, i64, i64) = conn
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(tokens_saved), 0), COALESCE(SUM(hit_count), 0) FROM compression_cache",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .ok()?;

    Some(CacheStats {
        entries: entries as usize,
        total_tokens_saved,
        total_hits,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> String {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("test_cache_{}.db", std::process::id()));
        path.to_string_lossy().to_string()
    }

    #[test]
    fn store_and_retrieve() {
        let db = temp_db();
        let hash = content_hash("hello world");

        assert!(get_compressed(&db, &hash).is_none());

        store_compressed(&db, &hash, "hello…", 10);

        let cached = get_compressed(&db, &hash);
        assert_eq!(cached, Some("hello…".to_string()));

        let stats = get_stats(&db).unwrap();
        assert_eq!(stats.entries, 1);
        assert_eq!(stats.total_tokens_saved, 10);
        assert_eq!(stats.total_hits, 1);

        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn content_hash_deterministic() {
        let h1 = content_hash("same content");
        let h2 = content_hash("same content");
        assert_eq!(h1, h2);

        let h3 = content_hash("different content");
        assert_ne!(h1, h3);
    }
}
