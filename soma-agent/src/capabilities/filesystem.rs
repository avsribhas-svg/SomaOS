use base64;
use serde_json::{json, Value};
use soma_common::{CapabilityError, ErrorReason, ActionSchema, CapabilityResult};
use std::fs;
use std::path::Path;

use super::param;
use super::Capability;

pub struct FileSystemCapability;

impl Capability for FileSystemCapability {
    fn name(&self) -> &str {
        "filesystem"
    }

    fn description(&self) -> &str {
        "File and directory operations — list, read, write, copy, move, delete, find"
    }

    fn actions(&self) -> Vec<ActionSchema> {
        vec![
            ActionSchema {
                name: "list_dir".to_string(),
                description: "List files and directories in a path".to_string(),
                params: vec![
                    param("path", "string", true, "Directory path to list"),
                    param("show_hidden", "boolean", false, "Include hidden files (default: false)"),
                ],
            },
            ActionSchema {
                name: "read_file".to_string(),
                description: "Read the contents of a text file".to_string(),
                params: vec![
                    param("path", "string", true, "File path to read"),
                    param("lines", "integer", false, "Max lines to read (default: all)"),
                ],
            },
            ActionSchema {
                name: "write_file".to_string(),
                description: "Write content to a file (creates or overwrites)".to_string(),
                params: vec![
                    param("path", "string", true, "File path to write"),
                    param("content", "string", true, "Content to write"),
                ],
            },
            ActionSchema {
                name: "create_dir".to_string(),
                description: "Create a directory (including parent directories)".to_string(),
                params: vec![param("path", "string", true, "Directory path to create")],
            },
            ActionSchema {
                name: "delete".to_string(),
                description: "Delete a file or empty directory".to_string(),
                params: vec![param("path", "string", true, "Path to delete")],
            },
            ActionSchema {
                name: "copy".to_string(),
                description: "Copy a file to a new location".to_string(),
                params: vec![
                    param("from", "string", true, "Source path"),
                    param("to", "string", true, "Destination path"),
                ],
            },
            ActionSchema {
                name: "move_item".to_string(),
                description: "Move/rename a file or directory".to_string(),
                params: vec![
                    param("from", "string", true, "Source path"),
                    param("to", "string", true, "Destination path"),
                ],
            },
            ActionSchema {
                name: "find".to_string(),
                description: "Find files matching a pattern in a directory".to_string(),
                params: vec![
                    param("path", "string", true, "Directory to search in"),
                    param("pattern", "string", true, "Filename pattern (glob)"),
                ],
            },
            ActionSchema {
                name: "file_info".to_string(),
                description: "Get metadata about a file (size, permissions, modified time)".to_string(),
                params: vec![param("path", "string", true, "File path")],
            },
        ]
    }

    fn execute(&self, action: &str, params: &Value) -> CapabilityResult {
        match action {
            "list_dir" => self.list_dir(params),
            "read_file" => self.read_file(params),
            "write_file" => self.write_file(params),
            "create_dir" => self.create_dir(params),
            "delete" => self.delete(params),
            "copy" => self.copy(params),
            "move_item" => self.move_item(params),
            "find" => self.find(params),
            "file_info" => self.file_info(params),
            _ => CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::UnknownAction, format!("Unknown filesystem action: {}", action))),
            },
        }
    }
}

impl FileSystemCapability {
    fn list_dir(&self, params: &Value) -> CapabilityResult {
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .map(expand_tilde)
            .unwrap_or_else(|| ".".to_string());
        if let Err(e) = guard_traversal(&path) { return e; }
        let show_hidden = params
            .get("show_hidden")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        match fs::read_dir(&path) {
            Ok(entries) => {
                let mut items: Vec<Value> = Vec::new();
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if !show_hidden && name.starts_with('.') {
                        continue;
                    }
                    let meta = entry.metadata().ok();
                    let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                    let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);

                    items.push(json!({
                        "name": name,
                        "is_dir": is_dir,
                        "size": size,
                    }));
                }
                ok(json!({ "path": path, "entries": items, "count": items.len() }))
            }
            Err(e) => err(&format!("Cannot read directory '{}': {}", path, e)),
        }
    }

    fn read_file(&self, params: &Value) -> CapabilityResult {
        let path = match params.get("path").and_then(|v| v.as_str()).map(expand_tilde) {
            Some(p) => p,
            None => return err("Missing required param: path"),
        };
        if let Err(e) = guard_traversal(&path) { return e; }

        // Detect image extensions — return base64-encoded bytes instead of text
        let ext = Path::new(&path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if ["png", "jpg", "jpeg", "gif", "bmp", "webp"].contains(&ext.as_str()) {
            return match fs::read(&path) {
                Ok(bytes) => {
                    use base64::Engine;
                    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
                    ok(json!({
                        "type": "image_base64",
                        "path": path,
                        "data": encoded,
                        "mime": format!("image/{}", ext),
                        "bytes": bytes.len(),
                    }))
                }
                Err(e) => err(&format!("Cannot read file '{}': {}", path, e)),
            };
        }

        let max_lines = params.get("lines").and_then(|v| v.as_u64());

        match fs::read_to_string(&path) {
            Ok(content) => {
                let output = match max_lines {
                    Some(n) => content
                        .lines()
                        .take(n as usize)
                        .collect::<Vec<_>>()
                        .join("\n"),
                    None => content,
                };
                let line_count = output.lines().count();
                let byte_size = output.len();
                ok(json!({ "path": path, "content": output, "lines": line_count, "bytes": byte_size }))
            }
            Err(e) => err(&format!("Cannot read file '{}': {}", path, e)),
        }
    }

    fn write_file(&self, params: &Value) -> CapabilityResult {
        let path = match params.get("path").and_then(|v| v.as_str()).map(expand_tilde) {
            Some(p) => p,
            None => return err("Missing required param: path"),
        };
        if let Err(e) = guard_traversal(&path) { return e; }
        let content = match params.get("content").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => return err("Missing required param: content"),
        };

        match fs::write(&path, content) {
            Ok(_) => ok(json!({ "path": path, "bytes_written": content.len() })),
            Err(e) => err(&format!("Cannot write file '{}': {}", path, e)),
        }
    }

    fn create_dir(&self, params: &Value) -> CapabilityResult {
        let path = match params.get("path").and_then(|v| v.as_str()).map(expand_tilde) {
            Some(p) => p,
            None => return err("Missing required param: path"),
        };
        if let Err(e) = guard_traversal(&path) { return e; }

        match fs::create_dir_all(&path) {
            Ok(_) => ok(json!({ "path": path, "created": true })),
            Err(e) => err(&format!("Cannot create directory '{}': {}", path, e)),
        }
    }

    fn delete(&self, params: &Value) -> CapabilityResult {
        let path = match params.get("path").and_then(|v| v.as_str()).map(expand_tilde) {
            Some(p) => p,
            None => return err("Missing required param: path"),
        };
        if let Err(e) = guard_traversal(&path) { return e; }

        let p = Path::new(&path);
        let result = if p.is_dir() {
            fs::remove_dir(&path)
        } else {
            fs::remove_file(&path)
        };

        match result {
            Ok(_) => ok(json!({ "path": path, "deleted": true })),
            Err(e) => err(&format!("Cannot delete '{}': {}", path, e)),
        }
    }

    fn copy(&self, params: &Value) -> CapabilityResult {
        let from = match params.get("from").and_then(|v| v.as_str()).map(expand_tilde) {
            Some(p) => p,
            None => return err("Missing required param: from"),
        };
        if let Err(e) = guard_traversal(&from) { return e; }
        let to = match params.get("to").and_then(|v| v.as_str()).map(expand_tilde) {
            Some(p) => p,
            None => return err("Missing required param: to"),
        };
        if let Err(e) = guard_traversal(&to) { return e; }

        match fs::copy(&from, &to) {
            Ok(bytes) => ok(json!({ "from": from, "to": to, "bytes_copied": bytes })),
            Err(e) => err(&format!("Cannot copy '{}' to '{}': {}", from, to, e)),
        }
    }

    fn move_item(&self, params: &Value) -> CapabilityResult {
        let from = match params.get("from").and_then(|v| v.as_str()).map(expand_tilde) {
            Some(p) => p,
            None => return err("Missing required param: from"),
        };
        if let Err(e) = guard_traversal(&from) { return e; }
        let to = match params.get("to").and_then(|v| v.as_str()).map(expand_tilde) {
            Some(p) => p,
            None => return err("Missing required param: to"),
        };
        if let Err(e) = guard_traversal(&to) { return e; }

        match fs::rename(&from, &to) {
            Ok(_) => ok(json!({ "from": from, "to": to, "moved": true })),
            Err(e) => err(&format!("Cannot move '{}' to '{}': {}", from, to, e)),
        }
    }

    fn find(&self, params: &Value) -> CapabilityResult {
        let path = match params.get("path").and_then(|v| v.as_str()).map(expand_tilde) {
            Some(p) => p,
            None => return err("Missing required param: path"),
        };
        if let Err(e) = guard_traversal(&path) { return e; }
        let pattern = match params.get("pattern").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return err("Missing required param: pattern"),
        };
        // Reject patterns that look like find flags or path traversal
        if pattern.starts_with('-') || pattern.contains('/') || pattern.contains("..") {
            return err("Invalid pattern: must be a plain glob (e.g. '*.rs'), not a path or flag");
        }

        let mut matches = Vec::new();
        if let Ok(output) = std::process::Command::new("find")
            .arg(&path).arg("-name").arg(pattern).arg("-maxdepth").arg("3")
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if !line.is_empty() {
                    matches.push(line.to_string());
                }
            }
        }

        ok(json!({ "path": path, "pattern": pattern, "matches": matches, "count": matches.len() }))
    }

    fn file_info(&self, params: &Value) -> CapabilityResult {
        let path = match params.get("path").and_then(|v| v.as_str()).map(expand_tilde) {
            Some(p) => p,
            None => return err("Missing required param: path"),
        };
        if let Err(e) = guard_traversal(&path) { return e; }

        match fs::metadata(&path) {
            Ok(meta) => {
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs());

                ok(json!({
                    "path": path,
                    "size": meta.len(),
                    "is_dir": meta.is_dir(),
                    "is_file": meta.is_file(),
                    "readonly": meta.permissions().readonly(),
                    "modified_unix": modified,
                }))
            }
            Err(e) => err(&format!("Cannot stat '{}': {}", path, e)),
        }
    }
}

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
    // Split on both / and \ and check each component
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
