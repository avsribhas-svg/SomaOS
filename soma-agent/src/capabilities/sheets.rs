//! SheetsCapability — agent interface for soma-sheets spreadsheet windows.
//!
//! Reads from the `AppStateCache` (populated by `CompositorMessage::AppStateChanged`),
//! and writes via the command pattern (returns `ipc_message: "AppAction"` in data
//! so that ipc.rs forwards it to the compositor as `AgentMessage::AppAction`).

use super::{param, Capability};
use crate::ipc::AppStateCache;
use serde_json::{json, Value};
use soma_common::{ActionSchema, CapabilityError, CapabilityResult, ErrorReason};

pub struct SheetsCapability {
    state_cache: AppStateCache,
}

impl SheetsCapability {
    pub fn new(state_cache: AppStateCache) -> Self {
        Self { state_cache }
    }
}

impl Capability for SheetsCapability {
    fn name(&self) -> &str {
        "sheets"
    }

    fn description(&self) -> &str {
        "Interact with soma-sheets spreadsheet windows. Read cell values, write data, and apply formulas."
    }

    fn actions(&self) -> Vec<ActionSchema> {
        vec![
            ActionSchema {
                name: "create".to_string(),
                description: "Create a new Sheets window and populate it with data. \
                              Use this to display tables to the user without them needing to open Sheets first. \
                              Pass all cell data upfront — the window appears on screen immediately.".to_string(),
                params: vec![
                    param("title", "string", true, "Window title (e.g. \"Q1 Budget\")"),
                    param("cells", "object", true,
                          "Cell data as object mapping A1 keys to values, e.g. {\"A1\": \"Month\", \"B1\": 50000}"),
                ],
            },
            ActionSchema {
                name: "describe".to_string(),
                description: "Return a summary of the spreadsheet: title, schema (column headers), \
                              row count, selection, and full cell data.".to_string(),
                params: vec![
                    param("window_id", "number", true, "ID of the Sheets window"),
                ],
            },
            ActionSchema {
                name: "read_range".to_string(),
                description: "Read a rectangular range of cells (e.g. \"A1:D5\") and return a 2D array of values.".to_string(),
                params: vec![
                    param("window_id", "number", true, "ID of the Sheets window"),
                    param("range", "string", true, "Cell range in A1:B2 notation"),
                ],
            },
            ActionSchema {
                name: "write_cell".to_string(),
                description: "Write a value to a single cell (e.g. cell \"B3\", value \"42\"). \
                              Numbers are stored as Number cells; other values as Text.".to_string(),
                params: vec![
                    param("window_id", "number", true, "ID of the Sheets window"),
                    param("cell", "string", true, "Target cell in A1 notation (e.g. \"B3\")"),
                    param("value", "string", true, "Value to write"),
                ],
            },
            ActionSchema {
                name: "apply_formula".to_string(),
                description: "Write a formula to a cell (e.g. cell \"D2\", formula \"=SUM(A2:C2)\"). \
                              The formula is evaluated immediately.".to_string(),
                params: vec![
                    param("window_id", "number", true, "ID of the Sheets window"),
                    param("cell", "string", true, "Target cell in A1 notation"),
                    param("formula", "string", true, "Formula string starting with '='"),
                ],
            },
        ]
    }

    fn execute(&self, action: &str, params: &Value) -> CapabilityResult {
        let window_id = params["window_id"].as_u64().unwrap_or(0) as u32;

        match action {
            // ── Create a new Sheets window pre-populated with data ─────────
            // Uses the command pattern: returns ipc_message so ipc.rs forwards
            // as AgentMessage::AppAction { window_id: 0 } to the compositor,
            // which creates the window and emits AppStateChanged back.
            "create" => {
                let title = params["title"].as_str().unwrap_or("Sheet 1").to_string();
                let cells = params.get("cells").cloned().unwrap_or(Value::Object(Default::default()));
                CapabilityResult {
                    success: true,
                    data: json!({
                        "ipc_message": "AppAction",
                        "window_id": 0u32,   // 0 = sentinel: create new window
                        "action": "create_and_populate",
                        "params": { "title": title, "cells": cells },
                    }),
                    error: None,
                }
            }

            "describe" => {
                let cache = self.state_cache.lock().unwrap();
                match cache.get(&window_id) {
                    Some(state) => CapabilityResult {
                        success: true,
                        data: json!({
                            "summary": state.summary,
                            "cells": state.cells,
                            "dirty": state.dirty,
                        }),
                        error: None,
                    },
                    None => CapabilityResult {
                        success: false,
                        data: Value::Null,
                        error: Some(CapabilityError::new(
                            ErrorReason::NotFound,
                            format!("No sheets window with id {} found in cache. \
                                     Open a Sheets window first.", window_id),
                        )),
                    },
                }
            }

            "read_range" => {
                let range = params["range"].as_str().unwrap_or("").to_string();
                if range.is_empty() {
                    return CapabilityResult {
                        success: false,
                        data: Value::Null,
                        error: Some(CapabilityError::new(
                            ErrorReason::InvalidParam,
                            "Missing required param: range (e.g. \"A1:D5\")".to_string(),
                        )),
                    };
                }

                let cache = self.state_cache.lock().unwrap();
                let state = match cache.get(&window_id) {
                    Some(s) => s,
                    None => return CapabilityResult {
                        success: false,
                        data: Value::Null,
                        error: Some(CapabilityError::new(
                            ErrorReason::NotFound,
                            format!("No sheets window with id {} in cache.", window_id),
                        )),
                    },
                };

                let cells = match &state.cells {
                    Some(c) => c,
                    None => return CapabilityResult {
                        success: true,
                        data: json!({ "range": range, "values": [] }),
                        error: None,
                    },
                };

                // Parse range "A1:D5" → ((col_start, row_start), (col_end, row_end))
                let (start, end) = match parse_range(&range) {
                    Some(r) => r,
                    None => return CapabilityResult {
                        success: false,
                        data: Value::Null,
                        error: Some(CapabilityError::new(
                            ErrorReason::InvalidParam,
                            format!("Invalid range: {}", range),
                        )),
                    },
                };

                let (col0, row0) = start;
                let (col1, row1) = end;
                let mut grid: Vec<Vec<Value>> = Vec::new();
                for row in row0..=row1 {
                    let mut r: Vec<Value> = Vec::new();
                    for col in col0..=col1 {
                        let key = cell_ref_to_string(row, col);
                        r.push(cells.get(&key).cloned().unwrap_or(Value::Null));
                    }
                    grid.push(r);
                }

                CapabilityResult {
                    success: true,
                    data: json!({ "range": range, "values": grid }),
                    error: None,
                }
            }

            // write_cell and apply_formula use the command pattern:
            // return ipc_message so ipc.rs forwards as AgentMessage::AppAction to the compositor
            "write_cell" => {
                let cell = params["cell"].as_str().unwrap_or("").to_string();
                let value = params["value"].as_str().unwrap_or("").to_string();
                if cell.is_empty() {
                    return CapabilityResult {
                        success: false,
                        data: Value::Null,
                        error: Some(CapabilityError::new(
                            ErrorReason::InvalidParam,
                            "Missing required param: cell".to_string(),
                        )),
                    };
                }
                CapabilityResult {
                    success: true,
                    data: json!({
                        "ipc_message": "AppAction",
                        "window_id": window_id,
                        "action": "write_cell",
                        "params": { "cell": cell, "value": value },
                    }),
                    error: None,
                }
            }

            "apply_formula" => {
                let cell = params["cell"].as_str().unwrap_or("").to_string();
                let formula = params["formula"].as_str().unwrap_or("").to_string();
                if cell.is_empty() || formula.is_empty() {
                    return CapabilityResult {
                        success: false,
                        data: Value::Null,
                        error: Some(CapabilityError::new(
                            ErrorReason::InvalidParam,
                            "Missing required params: cell and/or formula".to_string(),
                        )),
                    };
                }
                CapabilityResult {
                    success: true,
                    data: json!({
                        "ipc_message": "AppAction",
                        "window_id": window_id,
                        "action": "apply_formula",
                        "params": { "cell": cell, "formula": formula },
                    }),
                    error: None,
                }
            }

            _ => CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(
                    ErrorReason::UnknownAction,
                    format!("Unknown sheets action: {}", action),
                )),
            },
        }
    }
}

// ── Cell reference helpers ────────────────────────────────────────────────────

/// Convert (row, col) 0-indexed to "A1" notation string.
fn cell_ref_to_string(row: u32, col: u32) -> String {
    let mut col_str = String::new();
    let mut c = col;
    loop {
        col_str.insert(0, (b'A' + (c % 26) as u8) as char);
        if c < 26 {
            break;
        }
        c = c / 26 - 1;
    }
    format!("{}{}", col_str, row + 1)
}

/// Parse a cell ref like "B12" → (col=1, row=11).
fn parse_cell_ref(s: &str) -> Option<(u32, u32)> {
    let s = s.trim().to_uppercase();
    let col_end = s.find(|c: char| c.is_ascii_digit())?;
    let col_str = &s[..col_end];
    let row_str = &s[col_end..];
    if col_str.is_empty() || row_str.is_empty() {
        return None;
    }
    let col = col_str.chars().fold(0u32, |acc, c| {
        acc * 26 + (c as u32 - 'A' as u32 + 1)
    }) - 1;
    let row = row_str.parse::<u32>().ok()?.checked_sub(1)?;
    Some((col, row))
}

/// Parse a range like "A1:D5" → ((col0, row0), (col1, row1)).
fn parse_range(range: &str) -> Option<((u32, u32), (u32, u32))> {
    let parts: Vec<&str> = range.split(':').collect();
    if parts.len() != 2 {
        return None;
    }
    let (col0, row0) = parse_cell_ref(parts[0])?;
    let (col1, row1) = parse_cell_ref(parts[1])?;
    Some(((col0.min(col1), row0.min(row1)), (col0.max(col1), row0.max(row1))))
}
