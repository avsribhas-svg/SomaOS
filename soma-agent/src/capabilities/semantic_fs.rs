use serde_json::{json, Value};
use soma_common::{CapabilityError, ErrorReason, ActionSchema, CapabilityResult};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::param;
use super::Capability;

pub struct SemanticFsCapability;

impl Capability for SemanticFsCapability {
    fn name(&self) -> &str {
        "semantic_fs"
    }

    fn description(&self) -> &str {
        "Semantic file system — tag files, find by intent, annotate with workflow history"
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
                description: "Get metadata and semantic sidecar info for a file".to_string(),
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
                description: "Annotate a file with a workflow name and agent note; increments access_count".to_string(),
                params: vec![
                    param("path", "string", true, "Absolute path to the file"),
                    param("workflow", "string", false, "Workflow name to record"),
                    param("agent_note", "string", false, "Agent note to append"),
                ],
            },
            ActionSchema {
                name: "get_history".to_string(),
                description: "Return the full sidecar (tags, workflows, notes, timestamps) for a file".to_string(),
                params: vec![
                    param("path", "string", true, "Absolute path to the file"),
                ],
            },
            ActionSchema {
                name: "list_tagged".to_string(),
                description: "List all files that carry a given tag".to_string(),
                params: vec![
                    param("tag", "string", true, "Tag to search for"),
                    param("search_dir", "string", false, "Directory to search under (default: ~/)"),
                ],
            },
        ]
    }

    fn execute(&self, action: &str, params: &Value) -> CapabilityResult {
        match action {
            "tag"             => self.tag(params),
            "describe_file"   => self.describe_file(params),
            "find_by_intent"  => self.find_by_intent(params),
            "annotate"        => self.annotate(params),
            "get_history"     => self.get_history(params),
            "list_tagged"     => self.list_tagged(params),
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
            None => return err("Missing required param: tags"),
        };
        let description = params.get("description").and_then(|v| v.as_str()).map(|s| s.to_string());

        let sp = sidecar_path(&path);
        let mut sidecar = load_sidecar(&sp).unwrap_or_else(|| new_sidecar(&path));

        // Merge tags (deduplicate)
        let existing: Vec<String> = sidecar["tags"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();
        let mut merged = existing;
        for t in &new_tags {
            if !merged.contains(t) {
                merged.push(t.clone());
            }
        }
        sidecar["tags"] = json!(merged);

        if let Some(desc) = description {
            sidecar["description"] = json!(desc);
        }

        let now = now_unix();
        sidecar["last_tagged_at"] = json!(now);
        sidecar["tagged_by"] = json!("agent");

        if let Err(e) = save_sidecar(&sp, &sidecar) {
            return err(&format!("Failed to save sidecar: {}", e));
        }

        ok(json!({
            "path": path,
            "tags": sidecar["tags"],
            "sidecar": sp.to_string_lossy(),
        }))
    }

    fn describe_file(&self, params: &Value) -> CapabilityResult {
        let path = match params.get("path").and_then(|v| v.as_str()).map(expand_tilde) {
            Some(p) => p,
            None => return err("Missing required param: path"),
        };
        if let Err(e) = guard_traversal(&path) { return e; }

        let exists = Path::new(&path).exists();
        let (size, modified_unix) = if exists {
            match fs::metadata(&path) {
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

        let sp = sidecar_path(&path);
        let sidecar = load_sidecar(&sp).unwrap_or_else(|| new_sidecar(&path));

        ok(json!({
            "path": path,
            "exists": exists,
            "size": size,
            "modified_unix": modified_unix,
            "tags": sidecar["tags"],
            "description": sidecar["description"],
            "created_by": sidecar["created_by"],
            "workflows": sidecar["workflows"],
            "access_count": sidecar["access_count"],
        }))
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
            .unwrap_or_else(|| {
                std::env::var("HOME").unwrap_or_else(|_| "/".to_string())
            });
        if let Err(e) = guard_traversal(&search_dir) { return e; }

        let intent_words: Vec<&str> = intent.split_whitespace().collect();
        let now = now_unix();
        let seven_days: u64 = 7 * 24 * 3600;

        let mut results: Vec<(i64, Value)> = Vec::new();

        collect_sidecars(Path::new(&search_dir), 0, 4, &mut |sidecar_file, sidecar| {
            let file_path = sidecar["path"].as_str().unwrap_or("").to_string();
            let filename = Path::new(&file_path)
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let tags: Vec<String> = sidecar["tags"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_lowercase())).collect())
                .unwrap_or_default();
            let description = sidecar["description"].as_str().unwrap_or("").to_lowercase();
            let access_count = sidecar["access_count"].as_u64().unwrap_or(0);
            let last_accessed = sidecar["last_accessed"].as_u64().unwrap_or(0);

            let mut score: i64 = 0;

            // Tag match: +3 per tag word found in intent
            for tag in &tags {
                let tag_words: Vec<&str> = tag.split_whitespace().collect();
                for tw in &tag_words {
                    if intent.contains(*tw) {
                        score += 3;
                    }
                }
            }

            // Description match: +1 per intent word found in description
            for iw in &intent_words {
                if description.contains(iw) {
                    score += 1;
                }
            }

            // Filename match: +2 per intent word found in filename
            for iw in &intent_words {
                if filename.contains(iw) {
                    score += 2;
                }
            }

            // Recent access bonus
            if access_count > 0 {
                score += 1;
            }
            if last_accessed > 0 && now.saturating_sub(last_accessed) <= seven_days {
                score += 2;
            }

            if score > 0 {
                results.push((score, json!({
                    "path": file_path,
                    "score": score,
                    "tags": sidecar["tags"],
                    "description": sidecar["description"],
                })));
            }

            let _ = sidecar_file; // used by the closure signature
        });

        // Sort descending by score, take top 5
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

        let sp = sidecar_path(&path);
        let mut sidecar = load_sidecar(&sp).unwrap_or_else(|| new_sidecar(&path));

        let now = now_unix();

        // Append workflow (ring-buffer max 20)
        if let Some(wf) = &workflow {
            let mut workflows: Vec<String> = sidecar["workflows"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default();
            workflows.push(wf.clone());
            if workflows.len() > 20 {
                workflows.drain(0..workflows.len() - 20);
            }
            sidecar["workflows"] = json!(workflows);
        }

        // Append agent note (ring-buffer max 10)
        if let Some(note) = &agent_note {
            let mut notes: Vec<String> = sidecar["agent_notes"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default();
            notes.push(note.clone());
            if notes.len() > 10 {
                notes.drain(0..notes.len() - 10);
            }
            sidecar["agent_notes"] = json!(notes);
        }

        // Increment access_count, update last_accessed
        let access_count = sidecar["access_count"].as_u64().unwrap_or(0) + 1;
        sidecar["access_count"] = json!(access_count);
        sidecar["last_accessed"] = json!(now);

        if let Err(e) = save_sidecar(&sp, &sidecar) {
            return err(&format!("Failed to save sidecar: {}", e));
        }

        ok(json!({
            "path": path,
            "workflows": sidecar["workflows"],
            "access_count": access_count,
        }))
    }

    fn get_history(&self, params: &Value) -> CapabilityResult {
        let path = match params.get("path").and_then(|v| v.as_str()).map(expand_tilde) {
            Some(p) => p,
            None => return err("Missing required param: path"),
        };
        if let Err(e) = guard_traversal(&path) { return e; }

        let sp = sidecar_path(&path);
        match load_sidecar(&sp) {
            Some(sidecar) => ok(sidecar),
            None => CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(
                    ErrorReason::NotFound,
                    format!("No sidecar found for '{}'", path),
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
            .unwrap_or_else(|| {
                std::env::var("HOME").unwrap_or_else(|_| "/".to_string())
            });
        if let Err(e) = guard_traversal(&search_dir) { return e; }

        let tag_lc = tag.to_lowercase();
        let mut matches: Vec<Value> = Vec::new();

        collect_sidecars(Path::new(&search_dir), 0, 4, &mut |_sidecar_file, sidecar| {
            let tags: Vec<String> = sidecar["tags"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_lowercase())).collect())
                .unwrap_or_default();
            if tags.iter().any(|t| t == &tag_lc) {
                matches.push(json!({
                    "path": sidecar["path"],
                    "tags": sidecar["tags"],
                    "description": sidecar["description"],
                }));
            }
        });

        ok(json!({ "tag": tag, "matches": matches, "count": matches.len() }))
    }
}

// ---------------------------------------------------------------------------
// Sidecar helpers
// ---------------------------------------------------------------------------

/// Compute the sidecar path for a given file path.
/// The sidecar lives at `<parent_dir>/.soma-meta/<filename>.json`.
fn sidecar_path(file_path: &str) -> PathBuf {
    let p = Path::new(file_path);
    let dir = p.parent().unwrap_or(Path::new("."));
    let name = p.file_name().unwrap_or_default();
    dir.join(".soma-meta").join(format!("{}.json", name.to_string_lossy()))
}

/// Load a sidecar JSON file. Returns None if the file does not exist or is invalid.
fn load_sidecar(sp: &Path) -> Option<Value> {
    let raw = fs::read_to_string(sp).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Persist a sidecar JSON to disk, creating the `.soma-meta` directory if necessary.
fn save_sidecar(sp: &Path, sidecar: &Value) -> Result<(), String> {
    if let Some(parent) = sp.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(sidecar).map_err(|e| e.to_string())?;
    fs::write(sp, raw).map_err(|e| e.to_string())
}

/// Create a default (empty) sidecar for a file path.
fn new_sidecar(file_path: &str) -> Value {
    json!({
        "path": file_path,
        "created_by": "agent",
        "created_at": now_unix(),
        "tags": [],
        "description": "",
        "workflows": [],
        "agent_notes": [],
        "access_count": 0,
        "last_accessed": 0,
        "last_tagged_at": 0,
        "tagged_by": "agent",
    })
}

/// Return the current time as seconds since UNIX_EPOCH.
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Recursively scan directories for `.soma-meta/*.json` sidecar files, up to `max_depth`.
/// Calls `callback` for each valid sidecar with (sidecar_file_path, sidecar_value).
fn collect_sidecars<F>(dir: &Path, depth: usize, max_depth: usize, callback: &mut F)
where
    F: FnMut(&Path, &Value),
{
    if depth > max_depth {
        return;
    }

    // Check for a .soma-meta directory at the current level
    let meta_dir = dir.join(".soma-meta");
    if meta_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&meta_dir) {
            for entry in entries.flatten() {
                let ep = entry.path();
                if ep.extension().map(|x| x == "json").unwrap_or(false) {
                    if let Some(sidecar) = load_sidecar(&ep) {
                        callback(&ep, &sidecar);
                    }
                }
            }
        }
    }

    // Recurse into subdirectories (skip hidden dirs and .soma-meta itself)
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let ep = entry.path();
            if ep.is_dir() {
                let name = ep.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if name.starts_with('.') {
                    continue; // skip hidden dirs
                }
                collect_sidecars(&ep, depth + 1, max_depth, callback);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Security helpers (copied from filesystem.rs — private in sibling module)
// ---------------------------------------------------------------------------

/// Expand a leading `~` to the HOME directory.
fn expand_tilde(path: &str) -> String {
    if path == "~" || path.starts_with("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{}{}", home, &path[1..]);
        }
    }
    path.to_string()
}

/// Reject paths containing `..` components to prevent path traversal attacks.
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
    CapabilityResult {
        success: true,
        data,
        error: None,
    }
}

fn err(msg: &str) -> CapabilityResult {
    CapabilityResult {
        success: false,
        data: Value::Null,
        error: Some(CapabilityError::new(ErrorReason::InternalError, msg)),
    }
}
