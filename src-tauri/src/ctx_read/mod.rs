// Context-aware file reading with session caching and token savings.
//
// Implements:
//   - Session file cache with stable file refs (F1, F2, ...)
//   - `ctx_read`: read files with mode selection (full, diff, lines:N-M)
//   - `ctx_smart_read`: auto-picks the best mode based on cache state and file type
//
// Ported from lean-ctx (reduced slice: no signatures, map, aggressive,
// entropy, TDD, or learned mode predictor).

pub mod cache;
pub mod compressor;
pub mod ctx_read;
pub mod ctx_smart_read;
pub mod protocol;

/// Per-request result with token accounting.
pub struct ReadResult {
    pub output: String,
    /// Tokens in the original file content (what would be sent without compression).
    pub original_tokens: usize,
    /// Tokens actually sent (after cache stub, auto-delta, or line extraction).
    pub sent_tokens: usize,
}

pub use ctx_read::handle as read;
pub use ctx_smart_read::handle as smart_read;
