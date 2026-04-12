// Canonical PreToolUse ordering for ~/.claude/settings.json.
//
// Every installer that touches the PreToolUse array must call
// `enforce_pretooluse_order` after making its changes.  This keeps
// the invariant regardless of which features are installed first.
//
// Order:
//   1. DLP / guardian entries  (everything that isn't ctx_read or compress)
//   2. ctx_read caching hook   (llmwatcher-ctx-read)
//   3. shell compression hook  (llmwatcher-compress)  — always last

/// Checks whether a PreToolUse entry's command path contains `marker`.
fn entry_has_marker(entry: &serde_json::Value, marker: &str) -> bool {
    entry
        .get("hooks")
        .and_then(|h| h.as_array())
        .map(|arr| {
            arr.iter().any(|hook| {
                hook.get("command")
                    .and_then(|c| c.as_str())
                    .map(|s| s.contains(marker))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

/// Sort the PreToolUse array into canonical order:
///   guardian/DLP → ctx_read → compression.
///
/// Call this from any installer that adds or removes PreToolUse entries.
pub fn enforce_pretooluse_order(hooks: &mut serde_json::Map<String, serde_json::Value>) {
    let Some(arr) = hooks.get_mut("PreToolUse").and_then(|v| v.as_array_mut()) else {
        return;
    };

    let ctx_read: Vec<_> = arr
        .iter()
        .filter(|e| entry_has_marker(e, "llmwatcher-ctx-read"))
        .cloned()
        .collect();
    let compress: Vec<_> = arr
        .iter()
        .filter(|e| entry_has_marker(e, "llmwatcher-compress"))
        .cloned()
        .collect();

    // Keep everything else (DLP / guardian) in its original relative order.
    arr.retain(|e| {
        !entry_has_marker(e, "llmwatcher-ctx-read") && !entry_has_marker(e, "llmwatcher-compress")
    });

    arr.extend(ctx_read);
    arr.extend(compress);
}
