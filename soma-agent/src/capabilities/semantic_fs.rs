//! SemanticFsCapability — SQLite-backed file metadata index.
//!
//! v1.2: replaced `.soma-meta` JSON sidecars with a central SQLite database at
//! `~/.soma/index.db`.  Same capability interface (same action names + params) —
//! only the storage backend changes.

use serde_json::{json, Value};
use soma_common::{CapabilityError, ErrorReason, ActionSchema, CapabilityResult};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{param, Capability};

pub struct SemanticFsCapability;

impl Capability for SemanticFsCapability {
    fn name(&self) -> &str { "semantic_fs" }

    fn description(&self) -> &str {
        "Semantic file system — tag files, find by intent, annotate with workflow history (SQLite-backed index)"
    }

    fn actions(&self) -> Vec<ActionSchema> {
        vec![
            ActionSchema {
                name: "tag".to_string(),
                description: "Tag a file with labels and an optional description".to_string(),
                params: vec![
                    param("path", "string", true, "Absolute path to the file"),
                    param("tags", "array", true, "List of string tags to add"),
                    param("description", "string", false, "Human-readable description of the file"),
                ],
            },
            ActionSchema {
                name: "describe_file".to_string(),
                description: "Get metadata and semantic index info for a file".to_string(),
                params: vec![
                    param("path", "string", true, "Absolute path to the file"),
                ],
            },
            ActionSchema {
                name: "find_by_intent".to_string(),
                description: "Find files by natural language intent (e.g. 'the Q1 budget spreadsheet')".to_string(),
                params: vec![
                    param("intent", "string", true, "Natural language description of what you're looking for"),
                    param("search_dir", "string", false, "Directory to search under (default: ~/)"),
                ],
            },
            ActionSchema {
                name: "annotate".to_string(),
                description: "Annotate a file with a workflow name and agent note".to_string(),
                params: vec![
                    param("path", "string", true, "Absolute path to the file"),
                    param("workflow", "string", false, "Workflow name to record"),
                    param("agent_note", "string", false, "Agent note to append"),
                ],
            },
            ActionSchema {
                name: "get_history".to_string(),
                description: "Return the full index record (tags, history, timestamps) for a file".to_string(),
                params: vec![
                    param("path", "string", true, "Absolute path to the file"),
                ],
            },
            ActionSchema {
                name: "list_tagged".to_string(),
                description: "List all files that carry a given tag".to_string(),
                params: vec![
                    param("tag", "string", true, "Tag to search for"),
                    param("search_dir", "string", false, "Directory prefix to restrict results"),
                ],
            },
        ]
    }

    fn execute(&self, action: &str, params: &Value) -> CapabilityResult {
        match action {
            "tag"            => self.tag(params),
            "describe_file"  => self.describe_file(params),
            "find_by_intent" => self.find_by_intent(params),
            "annotate"       => self.annotate(params),
            "get_history"    => self.get_history(params),
            "list_tagged"    => self.list_tagged(params),
            _ => CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(
                    ErrorReason::UnknownAction,
                    format!("Unknown semantic_fs action: {}", action),
                )),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// SQLite helpers
// ---------------------------------------------------------------------------

fn db_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home).join(".soma").join("index.db")
}

fn open_db() -> Result<rusqlite::Connection, String> {
    let path = db_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let conn = rusqlite::Connection::open(&path).map_err(|e| e.to_string())?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS files (
            path        TEXT PRIMARY KEY,
            description TEXT DEFAULT '',
            tags        TEXT DEFAULT '',
            created_by  TEXT DEFAULT 'agent',
            created_at  INTEGER,
            history     TEXT DEFAULT '[]'
        );"
    ).map_err(|e| e.to_string())?;

    // Best-effort migration from old .soma-meta sidecars
    migrate_sidecars_if_needed(&conn);

    Ok(conn)
}

/// One-time migration: if the DB was just created (0 rows), scan for old
/// `.soma-meta/*.json` sidecar files and import them into the DB.
fn migrate_sidecars_if_needed(conn: &rusqlite::Connection) {
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))
        .unwrap_or(1); // if query fails, assume non-empty (safe default)
    if count > 0 {
        return; // already populated — skip migration
    }

    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let home_path = std::path::Path::new(&home);
    import_sidecars_recursive(conn, home_path, 0, 5);
}

fn import_sidecars_recursive(
    conn: &rusqlite::Connection,
    dir: &std::path::Path,
    depth: usize,
    max_depth: usize,
) {
    if depth > max_depth { return; }
    let meta_dir = dir.join(".soma-meta");
    if meta_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&meta_dir) {
            for entry in entries.flatten() {
                let ep = entry.path();
                if ep.extension().map(|x| x == "json").unwrap_or(false) {
                    if let Ok(raw) = std::fs::read_to_string(&ep) {
                        if let Ok(v) = serde_json::from_str::<Value>(&raw) {
                            let path = v["path"].as_str().unwrap_or("").to_string();
                            if path.is_empty() { continue; }
                            let description = v["description"].as_str().unwrap_or("").to_string();
                            let tags: Vec<String> = v["tags"].as_array()
                                .map(|a| a.iter().filter_map(|t| t.as_str().map(|s| s.to_string())).collect())
                                .unwrap_or_default();
                            let tags_str = tags.join(",");
                            let now = now_unix();
                            let _ = conn.execute(
                                "INSERT OR IGNORE INTO files (path, description, tags, created_by, created_at, history) VALUES (?1, ?2, ?3, 'agent', ?4, '[]')",
                                rusqlite::params![path, description, tags_str, now],
                            );
                        }
                    }
                }
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let ep = entry.path();
            if ep.is_dir() {
                let name = ep.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if name.starts_with('.') { continue; }
                import_sidecars_recursive(conn, &ep, depth + 1, max_depth);
            }
        }
    }
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Action implementations
// ---------------------------------------------------------------------------

impl SemanticFsCapability {
    fn tag(&self, params: &Value) -> CapabilityResult {
        let path = match params.get("path").and_then(|v| v.as_str()).map(expand_tilde) {
            Some(p) => p,
            None => return err("Missing required param: path"),
        };
        if let Err(e) = guard_traversal(&path) { return e; }

        let new_tags: Vec<String> = match params.get("tags").and_then(|v| v.as_array()) {
            Some(arr) => arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect(),
            None => {
                // Also accept a single string tag or "label" param
                if let Some(label) = params.get("label").and_then(|v| v.as_str()) {
                    vec![label.to_string()]
                } else {
                    return err("Missing required param: tags (array) or label (string)");
                }
            }
        };
        let description = params.get("description").and_then(|v| v.as_str()).unwrap_or("");

        let conn = match open_db() {
            Ok(c) => c,
            Err(e) => return err(&format!("DB open error: {}", e)),
        };

        // Fetch existing tags
        let existing_tags: String = conn.query_row(
            "SELECT COALESCE(tags, '') FROM files WHERE path = ?1",
            rusqlite::params![path],
            |r| r.get(0),
        ).unwrap_or_default();

        let mut merged: Vec<String> = if existing_tags.is_empty() {
            Vec::new()
        } else {
            existing_tags.split(',').map(|s| s.to_string()).collect()
        };
        for t in &new_tags {
            if !merged.contains(t) {
                merged.push(t.clone());
            }
        }
        let tags_str = merged.join(",");
        let now = now_unix();

        match conn.execute(
            "INSERT INTO files (path, description, tags, created_by, created_at, history)
             VALUES (?1, ?2, ?3, 'agent', ?4, '[]')
             ON CONFLICT(path) DO UPDATE SET
               tags        = excluded.tags,
               description = CASE WHEN excluded.description != '' THEN excluded.description ELSE description END",
            rusqlite::params![path, description, tags_str, now],
        ) {
            Ok(_) => ok(json!({ "path": path, "tags": merged })),
            Err(e) => err(&format!("DB write error: {}", e)),
        }
    }

    fn describe_file(&self, params: &Value) -> CapabilityResult {
        let path = match params.get("path").and_then(|v| v.as_str()).map(expand_tilde) {
            Some(p) => p,
            None => return err("Missing required param: path"),
        };
        if let Err(e) = guard_traversal(&path) { return e; }

        let exists = std::path::Path::new(&path).exists();
        let (size, modified_unix) = if exists {
            match std::fs::metadata(&path) {
                Ok(m) => {
                    let modified = m.modified().ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| d.as_secs());
                    (m.len(), modified)
                }
                Err(_) => (0u64, None),
            }
        } else {
            (0u64, None)
        };

        let conn = match open_db() {
            Ok(c) => c,
            Err(e) => return err(&format!("DB open error: {}", e)),
        };

        let row = conn.query_row(
            "SELECT description, tags, created_by, created_at, history FROM files WHERE path = ?1",
            rusqlite::params![path],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, u64>(3)?,
                    r.get::<_, String>(4)?,
                ))
            },
        );

        match row {
            Ok((description, tags_str, created_by, created_at, history_str)) => {
                let tags: Vec<&str> = tags_str.split(',').filter(|s| !s.is_empty()).collect();
                let history: Value = serde_json::from_str(&history_str).unwrap_or(json!([]));
                ok(json!({
                    "path": path,
                    "exists": exists,
                    "size": size,
                    "modified_unix": modified_unix,
                    "description": description,
                    "tags": tags,
                    "created_by": created_by,
                    "created_at": created_at,
                    "history": history,
                }))
            }
            Err(_) => {
                // Not in DB — return file metadata only
                ok(json!({
                    "path": path,
                    "exists": exists,
                    "size": size,
                    "modified_unix": modified_unix,
                    "description": "",
                    "tags": [],
                    "created_by": null,
                    "created_at": null,
                    "history": [],
                }))
            }
        }
    }

    fn find_by_intent(&self, params: &Value) -> CapabilityResult {
        let intent = match params.get("intent").and_then(|v| v.as_str()) {
            Some(i) => i.to_lowercase(),
            None => return err("Missing required param: intent"),
        };
        let search_dir = params
            .get("search_dir")
            .and_then(|v| v.as_str())
            .map(expand_tilde)
            .unwrap_or_else(|| std::env::var("HOME").unwrap_or_else(|_| "/".to_string()));
        if let Err(e) = guard_traversal(&search_dir) { return e; }

        let conn = match open_db() {
            Ok(c) => c,
            Err(e) => return err(&format!("DB open error: {}", e)),
        };

        let pattern = format!("%{}%", intent);
        let mut stmt = match conn.prepare(
            "SELECT path, description, tags FROM files
             WHERE (description LIKE ?1 OR tags LIKE ?2)
             LIMIT 50"
        ) {
            Ok(s) => s,
            Err(e) => return err(&format!("DB query error: {}", e)),
        };

        let rows: Vec<(String, String, String)> = match stmt.query_map(
            rusqlite::params![pattern, pattern],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)),
        ) {
            Ok(mapped) => mapped.flatten().collect(),
            Err(_) => Vec::new(),
        };

        let mut results: Vec<(i64, Value)> = rows.into_iter()
        .filter(|(path, _, _)| search_dir == "/" || path.starts_with(&search_dir))
        .map(|(path, description, tags_str)| {
            let tags: Vec<String> = tags_str.split(',').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect();
            let mut score: i64 = 0;
            for tag in &tags {
                if intent.contains(&tag.to_lowercase()) || tag.to_lowercase().contains(&intent) {
                    score += 3;
                }
            }
            for word in intent.split_whitespace() {
                if description.to_lowercase().contains(word) { score += 1; }
            }
            let fname = std::path::Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            for word in intent.split_whitespace() {
                if fname.contains(word) { score += 2; }
            }
            (score, json!({
                "path": path,
                "score": score,
                "tags": tags,
                "description": description,
            }))
        })
        .collect();

        results.sort_by(|a, b| b.0.cmp(&a.0));
        let top5: Vec<Value> = results.into_iter().take(5).map(|(_, v)| v).collect();
        ok(json!({ "intent": intent, "matches": top5 }))
    }

    fn annotate(&self, params: &Value) -> CapabilityResult {
        let path = match params.get("path").and_then(|v| v.as_str()).map(expand_tilde) {
            Some(p) => p,
            None => return err("Missing required param: path"),
        };
        if let Err(e) = guard_traversal(&path) { return e; }

        let workflow   = params.get("workflow").and_then(|v| v.as_str()).map(|s| s.to_string());
        let agent_note = params.get("agent_note").and_then(|v| v.as_str()).map(|s| s.to_string());

        let conn = match open_db() {
            Ok(c) => c,
            Err(e) => return err(&format!("DB open error: {}", e)),
        };

        // Ensure row exists
        let now = now_unix();
        conn.execute(
            "INSERT OR IGNORE INTO files (path, description, tags, created_by, created_at, history) VALUES (?1, '', '', 'agent', ?2, '[]')",
            rusqlite::params![path, now],
        ).ok();

        // Fetch current history
        let history_str: String = conn.query_row(
            "SELECT COALESCE(history, '[]') FROM files WHERE path = ?1",
            rusqlite::params![path],
            |r| r.get(0),
        ).unwrap_or_else(|_| "[]".to_string());

        let mut history: Vec<Value> = serde_json::from_str(&history_str).unwrap_or_default();

        let entry = json!({
            "timestamp": now,
            "workflow": workflow,
            "agent_note": agent_note,
        });
        history.push(entry);
        // Ring buffer — keep last 20 entries
        if history.len() > 20 {
            history.drain(0..history.len() - 20);
        }

        let new_history = serde_json::to_string(&history).unwrap_or_else(|_| "[]".to_string());

        match conn.execute(
            "UPDATE files SET history = ?1 WHERE path = ?2",
            rusqlite::params![new_history, path],
        ) {
            Ok(_) => ok(json!({ "path": path, "history_entries": history.len() })),
            Err(e) => err(&format!("DB update error: {}", e)),
        }
    }

    fn get_history(&self, params: &Value) -> CapabilityResult {
        let path = match params.get("path").and_then(|v| v.as_str()).map(expand_tilde) {
            Some(p) => p,
            None => return err("Missing required param: path"),
        };
        if let Err(e) = guard_traversal(&path) { return e; }

        let conn = match open_db() {
            Ok(c) => c,
            Err(e) => return err(&format!("DB open error: {}", e)),
        };

        match conn.query_row(
            "SELECT path, description, tags, created_by, created_at, history FROM files WHERE path = ?1",
            rusqlite::params![path],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, u64>(4)?,
                    r.get::<_, String>(5)?,
                ))
            },
        ) {
            Ok((p, description, tags_str, created_by, created_at, history_str)) => {
                let tags: Vec<&str> = tags_str.split(',').filter(|s| !s.is_empty()).collect();
                let history: Value = serde_json::from_str(&history_str).unwrap_or(json!([]));
                ok(json!({
                    "path": p,
                    "description": description,
                    "tags": tags,
                    "created_by": created_by,
                    "created_at": created_at,
                    "history": history,
                }))
            }
            Err(_) => CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(
                    ErrorReason::NotFound,
                    format!("No index entry found for '{}'", path),
                )),
            },
        }
    }

    fn list_tagged(&self, params: &Value) -> CapabilityResult {
        let tag = match params.get("tag").and_then(|v| v.as_str()) {
            Some(t) => t.to_string(),
            None => return err("Missing required param: tag"),
        };
        let search_dir = params
            .get("search_dir")
            .and_then(|v| v.as_str())
            .map(expand_tilde)
            .unwrap_or_else(|| "/".to_string());

        let conn = match open_db() {
            Ok(c) => c,
            Err(e) => return err(&format!("DB open error: {}", e)),
        };

        let pattern = format!("%{}%", tag.to_lowercase());
        let mut stmt = match conn.prepare(
            "SELECT path, description, tags FROM files WHERE LOWER(tags) LIKE ?1"
        ) {
            Ok(s) => s,
            Err(e) => return err(&format!("DB query error: {}", e)),
        };

        let rows: Vec<(String, String, String)> = match stmt.query_map(
            rusqlite::params![pattern],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)),
        ) {
            Ok(mapped) => mapped.flatten().collect(),
            Err(_) => Vec::new(),
        };

        let matches: Vec<Value> = rows.into_iter()
        .filter(|(path, _, _)| search_dir == "/" || path.starts_with(&search_dir))
        .map(|(path, description, tags_str)| {
            let tags: Vec<String> = tags_str.split(',').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect();
            json!({ "path": path, "description": description, "tags": tags })
        })
        .collect();

        let count = matches.len();
        ok(json!({ "tag": tag, "matches": matches, "count": count }))
    }
}

// ---------------------------------------------------------------------------
// Security helpers
// ---------------------------------------------------------------------------

fn expand_tilde(path: &str) -> String {
    if path == "~" || path.starts_with("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{}{}", home, &path[1..]);
        }
    }
    path.to_string()
}

fn guard_traversal(path: &str) -> Result<(), CapabilityResult> {
    if path.split(['/', '\\']).any(|c| c == "..") {
        Err(CapabilityResult {
            success: false,
            data: Value::Null,
            error: Some(CapabilityError::new(
                ErrorReason::PermissionDenied,
                "Path traversal ('..') not allowed",
            )),
        })
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Result helpers
// ---------------------------------------------------------------------------

fn ok(data: Value) -> CapabilityResult {
    CapabilityResult { success: true, data, error: None }
}

fn err(msg: &str) -> CapabilityResult {
    CapabilityResult {
        success: false,
        data: Value::Null,
        error: Some(CapabilityError::new(ErrorReason::InternalError, msg)),
    }
}
