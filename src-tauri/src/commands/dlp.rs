// DLP Settings Tauri Commands

use crate::database::open_connection;
use crate::pattern_utils::{
    collect_matches_with_negative_context, compile_pattern_set, filter_by_min_occurrences,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct DlpPattern {
    pub id: i64,
    pub name: String,
    pub pattern_type: String,
    pub patterns: Vec<String>,
    pub negative_pattern_type: Option<String>,
    pub negative_patterns: Option<Vec<String>>,
    pub enabled: bool,
    pub min_occurrences: i32,
    pub min_unique_chars: i32,
    pub is_builtin: bool,
}

#[derive(Serialize)]
pub struct DlpSettings {
    pub patterns: Vec<DlpPattern>,
}

#[tauri::command]
pub fn get_dlp_settings() -> Result<DlpSettings, String> {
    let conn = open_connection().map_err(|e| e.to_string())?;

    // Get all patterns (builtin and custom)
    let mut stmt = conn
        .prepare(
            "SELECT id, name, pattern_type, patterns, negative_pattern_type, negative_patterns,
                    enabled, min_occurrences, min_unique_chars, is_builtin
             FROM dlp_patterns ORDER BY is_builtin DESC, id",
        )
        .map_err(|e| e.to_string())?;

    let patterns: Vec<DlpPattern> = stmt
        .query_map([], |row| {
            let patterns_json: String = row.get(3)?;
            let patterns: Vec<String> = serde_json::from_str(&patterns_json).unwrap_or_default();

            let negative_patterns_json: Option<String> = row.get(5)?;
            let negative_patterns: Option<Vec<String>> = negative_patterns_json
                .and_then(|json| serde_json::from_str(&json).ok());

            Ok(DlpPattern {
                id: row.get(0)?,
                name: row.get(1)?,
                pattern_type: row.get(2)?,
                patterns,
                negative_pattern_type: row.get(4)?,
                negative_patterns,
                enabled: row.get::<_, i32>(6)? == 1,
                min_occurrences: row.get(7)?,
                min_unique_chars: row.get(8)?,
                is_builtin: row.get::<_, i32>(9)? == 1,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(DlpSettings { patterns })
}

#[tauri::command]
pub fn add_dlp_pattern(
    name: String,
    pattern_type: String,
    patterns: Vec<String>,
    negative_pattern_type: Option<String>,
    negative_patterns: Option<Vec<String>>,
    min_occurrences: Option<i32>,
    min_unique_chars: Option<i32>,
) -> Result<i64, String> {
    if name.trim().is_empty() {
        return Err("Name is required".to_string());
    }
    if patterns.is_empty() {
        return Err("At least one pattern is required".to_string());
    }

    let conn = open_connection().map_err(|e| e.to_string())?;
    let patterns_json = serde_json::to_string(&patterns).map_err(|e| e.to_string())?;
    let negative_patterns_json = negative_patterns
        .as_ref()
        .map(|np| serde_json::to_string(np).unwrap_or_else(|_| "[]".to_string()));
    let created_at = chrono::Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO dlp_patterns (name, pattern_type, patterns, negative_pattern_type, negative_patterns, enabled, min_occurrences, min_unique_chars, is_builtin, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?7, 0, ?8)",
        rusqlite::params![
            name.trim(),
            pattern_type,
            patterns_json,
            negative_pattern_type,
            negative_patterns_json,
            min_occurrences.unwrap_or(1),
            min_unique_chars.unwrap_or(0),
            created_at
        ],
    )
    .map_err(|e| e.to_string())?;

    Ok(conn.last_insert_rowid())
}

#[tauri::command]
pub fn update_dlp_pattern(
    id: i64,
    name: Option<String>,
    pattern_type: Option<String>,
    patterns: Option<Vec<String>>,
    negative_pattern_type: Option<String>,
    negative_patterns: Option<Vec<String>>,
    enabled: Option<bool>,
    min_occurrences: Option<i32>,
    min_unique_chars: Option<i32>,
) -> Result<(), String> {
    let conn = open_connection().map_err(|e| e.to_string())?;

    // Build dynamic update query based on provided fields
    let mut updates: Vec<String> = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(ref n) = name {
        if n.trim().is_empty() {
            return Err("Name cannot be empty".to_string());
        }
        updates.push("name = ?".to_string());
        params.push(Box::new(n.trim().to_string()));
    }

    if let Some(ref pt) = pattern_type {
        updates.push("pattern_type = ?".to_string());
        params.push(Box::new(pt.clone()));
    }

    if let Some(ref p) = patterns {
        if p.is_empty() {
            return Err("At least one pattern is required".to_string());
        }
        let patterns_json = serde_json::to_string(p).map_err(|e| e.to_string())?;
        updates.push("patterns = ?".to_string());
        params.push(Box::new(patterns_json));
    }

    // Handle negative_pattern_type - allow setting to null by passing empty string
    if negative_pattern_type.is_some() {
        let npt = negative_pattern_type.as_ref().unwrap();
        if npt.is_empty() {
            updates.push("negative_pattern_type = NULL".to_string());
        } else {
            updates.push("negative_pattern_type = ?".to_string());
            params.push(Box::new(npt.clone()));
        }
    }

    // Handle negative_patterns - allow setting to null by passing empty array
    if negative_patterns.is_some() {
        let np = negative_patterns.as_ref().unwrap();
        if np.is_empty() {
            updates.push("negative_patterns = NULL".to_string());
        } else {
            let np_json = serde_json::to_string(np).map_err(|e| e.to_string())?;
            updates.push("negative_patterns = ?".to_string());
            params.push(Box::new(np_json));
        }
    }

    if let Some(e) = enabled {
        updates.push("enabled = ?".to_string());
        params.push(Box::new(e as i32));
    }

    if let Some(mo) = min_occurrences {
        updates.push("min_occurrences = ?".to_string());
        params.push(Box::new(mo));
    }

    if let Some(muc) = min_unique_chars {
        updates.push("min_unique_chars = ?".to_string());
        params.push(Box::new(muc));
    }

    if updates.is_empty() {
        return Ok(()); // Nothing to update
    }

    params.push(Box::new(id));

    let sql = format!(
        "UPDATE dlp_patterns SET {} WHERE id = ?",
        updates.join(", ")
    );

    let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    conn.execute(&sql, params_refs.as_slice())
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub fn toggle_dlp_pattern(id: i64, enabled: bool) -> Result<(), String> {
    let conn = open_connection().map_err(|e| e.to_string())?;

    conn.execute(
        "UPDATE dlp_patterns SET enabled = ?1 WHERE id = ?2",
        rusqlite::params![enabled as i32, id],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub fn delete_dlp_pattern(id: i64) -> Result<(), String> {
    let conn = open_connection().map_err(|e| e.to_string())?;

    // Prevent deleting builtin patterns
    let is_builtin: bool = conn
        .query_row(
            "SELECT is_builtin FROM dlp_patterns WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get::<_, i32>(0).map(|v| v == 1),
        )
        .unwrap_or(false);

    if is_builtin {
        return Err("Cannot delete builtin patterns. You can disable them instead.".to_string());
    }

    conn.execute(
        "DELETE FROM dlp_patterns WHERE id = ?1",
        rusqlite::params![id],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[derive(Serialize)]
pub struct DlpDetectionRecord {
    id: i64,
    request_id: i64,
    timestamp: String,
    pattern_name: String,
    pattern_type: String,
    original_value: String,
    placeholder: String,
    message_index: Option<i32>,
}

#[derive(Serialize)]
pub struct DlpStats {
    total_detections: i64,
    detections_by_pattern: Vec<PatternCount>,
    recent_detections: Vec<DlpDetectionRecord>,
}

#[derive(Serialize)]
pub struct PatternCount {
    pattern_name: String,
    count: i64,
}

#[tauri::command]
pub fn get_dlp_detection_stats(time_range: String, backend: String) -> Result<DlpStats, String> {
    let conn = open_connection().map_err(|e| e.to_string())?;

    // Ensure table exists
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS dlp_detections (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            request_id INTEGER,
            timestamp TEXT NOT NULL,
            pattern_name TEXT NOT NULL,
            pattern_type TEXT NOT NULL,
            original_value TEXT NOT NULL,
            placeholder TEXT NOT NULL,
            message_index INTEGER,
            FOREIGN KEY (request_id) REFERENCES requests(id)
        )",
        [],
    );

    let hours = match time_range.as_str() {
        "1h" => 1,
        "6h" => 6,
        "1d" => 24,
        "7d" => 24 * 7,
        _ => 24,
    };
    let cutoff = chrono::Utc::now() - chrono::Duration::hours(hours);
    let cutoff_ts = cutoff.to_rfc3339();

    // Build backend filter
    let backend_filter = if backend == "all" {
        String::new()
    } else {
        format!(" AND r.backend = '{}'", backend)
    };

    // Get total detections count (with backend filter)
    let total_detections: i64 = conn
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM dlp_detections d
                 JOIN requests r ON d.request_id = r.id
                 WHERE d.timestamp >= ?1{}",
                backend_filter
            ),
            [&cutoff_ts],
            |row| row.get(0),
        )
        .unwrap_or(0);

    // Get detections by pattern (with backend filter)
    let mut stmt = conn
        .prepare(&format!(
            "SELECT d.pattern_name, COUNT(*) as count FROM dlp_detections d
             JOIN requests r ON d.request_id = r.id
             WHERE d.timestamp >= ?1{} GROUP BY d.pattern_name ORDER BY count DESC",
            backend_filter
        ))
        .map_err(|e| e.to_string())?;

    let detections_by_pattern: Vec<PatternCount> = stmt
        .query_map([&cutoff_ts], |row| {
            Ok(PatternCount {
                pattern_name: row.get(0)?,
                count: row.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    // Get recent detections (with backend filter)
    let mut stmt = conn
        .prepare(&format!(
            "SELECT d.id, d.request_id, d.timestamp, d.pattern_name, d.pattern_type, d.original_value, d.placeholder, d.message_index
             FROM dlp_detections d
             JOIN requests r ON d.request_id = r.id
             WHERE d.timestamp >= ?1{} ORDER BY d.id DESC LIMIT 50",
            backend_filter
        ))
        .map_err(|e| e.to_string())?;

    let recent_detections: Vec<DlpDetectionRecord> = stmt
        .query_map([&cutoff_ts], |row| {
            Ok(DlpDetectionRecord {
                id: row.get(0)?,
                request_id: row.get(1)?,
                timestamp: row.get(2)?,
                pattern_name: row.get(3)?,
                pattern_type: row.get(4)?,
                original_value: row.get(5)?,
                placeholder: row.get(6)?,
                message_index: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(DlpStats {
        total_detections,
        detections_by_pattern,
        recent_detections,
    })
}

#[tauri::command]
pub fn get_dlp_detections_for_request(request_id: i64) -> Result<Vec<DlpDetectionRecord>, String> {
    let conn = open_connection().map_err(|e| e.to_string())?;

    let mut stmt = conn
        .prepare(
            "SELECT id, request_id, timestamp, pattern_name, pattern_type, original_value, placeholder, message_index
             FROM dlp_detections WHERE request_id = ?1 ORDER BY id ASC",
        )
        .map_err(|e| e.to_string())?;

    let detections: Vec<DlpDetectionRecord> = stmt
        .query_map([request_id], |row| {
            Ok(DlpDetectionRecord {
                id: row.get(0)?,
                request_id: row.get(1)?,
                timestamp: row.get(2)?,
                pattern_name: row.get(3)?,
                pattern_type: row.get(4)?,
                original_value: row.get(5)?,
                placeholder: row.get(6)?,
                message_index: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(detections)
}

#[derive(Serialize)]
pub struct TestPatternResult {
    pub matches: Vec<String>,
    pub excluded: bool,
}

/// Test a pattern configuration against sample text without saving
#[tauri::command]
pub fn test_dlp_pattern(
    pattern_type: String,
    patterns: Vec<String>,
    negative_pattern_type: Option<String>,
    negative_patterns: Option<Vec<String>>,
    min_occurrences: i32,
    min_unique_chars: i32,
    test_text: String,
) -> Result<TestPatternResult, String> {
    // Compile patterns using shared utility
    let compiled = compile_pattern_set(
        &patterns,
        &pattern_type,
        negative_patterns.as_ref(),
        negative_pattern_type.as_deref(),
    )?;

    // Collect matches with context-aware negative pattern filtering
    // Each match is checked against negative patterns within its 30-char context window
    let match_result = collect_matches_with_negative_context(
        &test_text,
        &compiled.regexes,
        &compiled.negative_regexes,
        min_unique_chars,
        None, // No validator for manual test — tests raw regex matches
    );

    // Filter by min_occurrences threshold
    let matches = filter_by_min_occurrences(match_result, min_occurrences);

    // If all matches were excluded by negative patterns, indicate exclusion
    let excluded = matches.is_empty() && !compiled.negative_regexes.is_empty();

    Ok(TestPatternResult {
        matches,
        excluded,
    })
}
