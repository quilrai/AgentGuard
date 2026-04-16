use md5::{Digest, Md5};
use std::collections::HashMap;
use std::time::Instant;

use crate::shell_compression::tokens::count_tokens;

fn max_cache_tokens() -> usize {
    std::env::var("LEAN_CTX_CACHE_MAX_TOKENS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(500_000)
}

#[derive(Clone, Debug)]
pub struct CacheEntry {
    pub content: String,
    pub hash: String,
    pub line_count: usize,
    pub original_tokens: usize,
    pub read_count: u32,
    pub last_access: Instant,
}

impl CacheEntry {
    /// Boltzmann-inspired eviction score. Higher = more valuable = keep longer.
    pub fn eviction_score(&self, now: Instant) -> f64 {
        let elapsed = now.duration_since(self.last_access).as_secs_f64();
        let recency = 1.0 / (1.0 + elapsed.sqrt());
        let frequency = (self.read_count as f64 + 1.0).ln();
        let size_value = (self.original_tokens as f64 + 1.0).ln();
        recency * 0.4 + frequency * 0.3 + size_value * 0.3
    }
}

#[derive(Debug)]
pub struct CacheStats {
    pub total_reads: u64,
    pub cache_hits: u64,
    pub total_original_tokens: u64,
    pub total_sent_tokens: u64,
    pub files_tracked: usize,
}

impl CacheStats {
    #[allow(dead_code)]
    pub fn tokens_saved(&self) -> u64 {
        self.total_original_tokens
            .saturating_sub(self.total_sent_tokens)
    }
}

pub struct SessionCache {
    entries: HashMap<String, CacheEntry>,
    file_refs: HashMap<String, String>,
    next_ref: usize,
    stats: CacheStats,
}

impl Default for SessionCache {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionCache {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            file_refs: HashMap::new(),
            next_ref: 1,
            stats: CacheStats {
                total_reads: 0,
                cache_hits: 0,
                total_original_tokens: 0,
                total_sent_tokens: 0,
                files_tracked: 0,
            },
        }
    }

    pub fn get_file_ref(&mut self, path: &str) -> String {
        if let Some(r) = self.file_refs.get(path) {
            return r.clone();
        }
        let r = format!("F{}", self.next_ref);
        self.next_ref += 1;
        self.file_refs.insert(path.to_string(), r.clone());
        r
    }

    pub fn get(&self, path: &str) -> Option<&CacheEntry> {
        self.entries.get(path)
    }

    pub fn record_cache_hit(&mut self, path: &str) -> Option<&CacheEntry> {
        if let Some(entry) = self.entries.get_mut(path) {
            entry.read_count += 1;
            entry.last_access = Instant::now();
            self.stats.total_reads += 1;
            self.stats.cache_hits += 1;
            self.stats.total_original_tokens += entry.original_tokens as u64;
            Some(entry)
        } else {
            None
        }
    }

    /// Store content in cache. Returns (entry, was_cache_hit).
    ///
    /// This updates `total_original_tokens` but NOT `total_sent_tokens` —
    /// the caller must call `record_sent_tokens()` after computing the
    /// actual output size (which depends on the read mode).
    pub fn store(&mut self, path: &str, content: String) -> (CacheEntry, bool) {
        let hash = compute_md5(&content);
        let line_count = content.lines().count();
        let original_tokens = count_tokens(&content);
        let now = Instant::now();

        self.stats.total_reads += 1;
        self.stats.total_original_tokens += original_tokens as u64;

        if let Some(existing) = self.entries.get_mut(path) {
            existing.last_access = now;
            if existing.hash == hash {
                existing.read_count += 1;
                self.stats.cache_hits += 1;
                return (existing.clone(), true);
            }
            existing.content = content;
            existing.hash = hash;
            existing.line_count = line_count;
            existing.original_tokens = original_tokens;
            existing.read_count += 1;
            return (existing.clone(), false);
        }

        self.evict_if_needed(original_tokens);
        self.get_file_ref(path);

        let entry = CacheEntry {
            content,
            hash,
            line_count,
            original_tokens,
            read_count: 1,
            last_access: now,
        };

        self.entries.insert(path.to_string(), entry.clone());
        self.stats.files_tracked = self.entries.len();
        (entry, false)
    }

    /// Record the actual number of tokens sent to the caller for this read.
    pub fn record_sent_tokens(&mut self, tokens: usize) {
        self.stats.total_sent_tokens += tokens as u64;
    }

    pub fn total_cached_tokens(&self) -> usize {
        self.entries.values().map(|e| e.original_tokens).sum()
    }

    fn evict_if_needed(&mut self, incoming_tokens: usize) {
        let max_tokens = max_cache_tokens();
        let current = self.total_cached_tokens();
        if current + incoming_tokens <= max_tokens {
            return;
        }

        let now = Instant::now();
        let mut scored: Vec<(String, f64)> = self
            .entries
            .iter()
            .map(|(path, entry)| (path.clone(), entry.eviction_score(now)))
            .collect();
        scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut freed = 0usize;
        let target = (current + incoming_tokens).saturating_sub(max_tokens);
        for (path, _score) in &scored {
            if freed >= target {
                break;
            }
            if let Some(entry) = self.entries.remove(path) {
                freed += entry.original_tokens;
                self.file_refs.remove(path);
                self.stats.files_tracked = self.entries.len();
            }
        }
    }

    pub fn invalidate(&mut self, path: &str) -> bool {
        if self.entries.remove(path).is_some() {
            self.file_refs.remove(path);
            self.stats.files_tracked = self.entries.len();
            true
        } else {
            false
        }
    }

    pub fn clear(&mut self) -> usize {
        let count = self.entries.len();
        self.entries.clear();
        self.file_refs.clear();
        self.next_ref = 1;
        self.stats = CacheStats {
            total_reads: 0,
            cache_hits: 0,
            total_original_tokens: 0,
            total_sent_tokens: 0,
            files_tracked: 0,
        };
        count
    }

    #[allow(dead_code)]
    pub fn get_stats(&self) -> &CacheStats {
        &self.stats
    }
}

pub fn compute_md5(content: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_stores_and_retrieves() {
        let mut cache = SessionCache::new();
        let (entry, was_hit) = cache.store("/test/file.rs", "fn main() {}".to_string());
        assert!(!was_hit);
        assert_eq!(entry.line_count, 1);
        assert!(cache.get("/test/file.rs").is_some());
    }

    #[test]
    fn cache_hit_on_same_content() {
        let mut cache = SessionCache::new();
        cache.store("/test/file.rs", "content".to_string());
        let (_, was_hit) = cache.store("/test/file.rs", "content".to_string());
        assert!(was_hit);
    }

    #[test]
    fn cache_miss_on_changed_content() {
        let mut cache = SessionCache::new();
        cache.store("/test/file.rs", "old content".to_string());
        let (_, was_hit) = cache.store("/test/file.rs", "new content".to_string());
        assert!(!was_hit);
    }

    #[test]
    fn file_refs_are_sequential() {
        let mut cache = SessionCache::new();
        assert_eq!(cache.get_file_ref("/a.rs"), "F1");
        assert_eq!(cache.get_file_ref("/b.rs"), "F2");
        assert_eq!(cache.get_file_ref("/a.rs"), "F1"); // stable
    }

    #[test]
    fn cache_clear_resets_everything() {
        let mut cache = SessionCache::new();
        cache.store("/a.rs", "a".to_string());
        cache.store("/b.rs", "b".to_string());
        let count = cache.clear();
        assert_eq!(count, 2);
        assert!(cache.get("/a.rs").is_none());
        assert_eq!(cache.get_file_ref("/c.rs"), "F1");
    }

    #[test]
    fn cache_invalidate_removes_entry() {
        let mut cache = SessionCache::new();
        cache.store("/test.rs", "test".to_string());
        let old_ref = cache.get_file_ref("/test.rs");
        assert!(cache.invalidate("/test.rs"));
        assert!(cache.get("/test.rs").is_none());
        assert_ne!(cache.get_file_ref("/test.rs"), old_ref);
        assert!(!cache.invalidate("/nonexistent.rs"));
    }

    #[test]
    fn md5_is_deterministic() {
        let h1 = compute_md5("test content");
        let h2 = compute_md5("test content");
        assert_eq!(h1, h2);
        assert_ne!(h1, compute_md5("different"));
    }
}
