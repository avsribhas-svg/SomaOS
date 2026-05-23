//! soma-sheets — dual-interface spreadsheet for SomaOS v1.1
//!
//! Both the human and the agent are first-class users:
//!   Human: click cells, type values, Tab/Enter/Arrow navigation, formula bar
//!   Agent: `read_range`, `write_cell`, `apply_formula` via `NativeAppContent`
//!
//! Both share the same `cells` HashMap; edits from either side are immediately
//! visible to the other via `AppStateChanged` IPC messages.

use std::collections::HashMap;
use serde_json::{json, Value};
use soma_common::{AppState, CapabilityResult, CapabilityError, ErrorReason};
use crate::renderer::Renderer;
use crate::window_manager::NativeAppContent;
use tiny_skia::Pixmap;

// ─────────────────────────────────────────────────────────────────────────────
//  Cell model
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Cell {
    Empty,
    Text(String),
    Number(f64),
    /// Formula stored as raw expression; `value` is the last computed result.
    Formula { expr: String, value: Option<f64> },
}

impl Cell {
    /// Display string for rendering.
    pub fn display(&self) -> String {
        match self {
            Cell::Empty => String::new(),
            Cell::Text(s) => s.clone(),
            Cell::Number(n) => {
                if n.fract() == 0.0 && n.abs() < 1e12 {
                    format!("{}", *n as i64)
                } else {
                    format!("{:.2}", n)
                }
            }
            Cell::Formula { value, .. } => match value {
                Some(v) => {
                    if v.fract() == 0.0 && v.abs() < 1e12 {
                        format!("{}", *v as i64)
                    } else {
                        format!("{:.2}", v)
                    }
                }
                None => "#ERR".to_string(),
            },
        }
    }

    /// Raw content string (what the formula bar shows).
    pub fn raw(&self) -> String {
        match self {
            Cell::Empty => String::new(),
            Cell::Text(s) => s.clone(),
            Cell::Number(n) => {
                if n.fract() == 0.0 { format!("{}", *n as i64) } else { format!("{}", n) }
            }
            Cell::Formula { expr, .. } => format!("={}", expr),
        }
    }

    /// Numeric value for use in formula evaluation.
    pub fn numeric(&self) -> f64 {
        match self {
            Cell::Number(n) => *n,
            Cell::Formula { value: Some(v), .. } => *v,
            Cell::Text(s) => s.parse::<f64>().unwrap_or(0.0),
            _ => 0.0,
        }
    }

    /// True if this cell has any content.
    pub fn is_empty(&self) -> bool {
        matches!(self, Cell::Empty)
    }

    /// Serialise to a JSON Value for AppState.cells.
    pub fn to_json_value(&self) -> Value {
        match self {
            Cell::Empty => Value::Null,
            Cell::Text(s) => json!(s),
            Cell::Number(n) => json!(n),
            Cell::Formula { value: Some(v), .. } => json!(v),
            Cell::Formula { value: None, .. } => Value::Null,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Cell address helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Convert 0-indexed column number to letter(s): 0 → "A", 25 → "Z", 26 → "AA"
pub fn col_to_letter(col: u32) -> String {
    let mut n = col;
    let mut s = String::new();
    loop {
        s.insert(0, (b'A' + (n % 26) as u8) as char);
        if n < 26 { break; }
        n = n / 26 - 1;
    }
    s
}

/// Parse a cell reference like "B12" → (row=11, col=1), 0-indexed.
/// Returns None on invalid input.
pub fn parse_cell_ref(s: &str) -> Option<(u32, u32)> {
    let s = s.trim().to_uppercase();
    let col_end = s.find(|c: char| c.is_ascii_digit())?;
    if col_end == 0 { return None; }
    let col_str = &s[..col_end];
    let row_str = &s[col_end..];
    let row: u32 = row_str.parse::<u32>().ok()?.checked_sub(1)?;
    let col: u32 = col_str.bytes().fold(0u32, |acc, b| {
        acc * 26 + (b - b'A') as u32 + 1
    }).checked_sub(1)?;
    Some((row, col))
}

/// Parse a range like "A1:D5" → ((r1,c1), (r2,c2)), 0-indexed, inclusive.
pub fn parse_range(s: &str) -> Option<((u32, u32), (u32, u32))> {
    let s = s.trim().to_uppercase();
    let parts: Vec<&str> = s.splitn(2, ':').collect();
    if parts.len() != 2 { return None; }
    let a = parse_cell_ref(parts[0])?;
    let b = parse_cell_ref(parts[1])?;
    Some((
        (a.0.min(b.0), a.1.min(b.1)),
        (a.0.max(b.0), a.1.max(b.1)),
    ))
}

/// "A1" notation for a (row, col) pair.
pub fn cell_ref(row: u32, col: u32) -> String {
    format!("{}{}", col_to_letter(col), row + 1)
}

// ─────────────────────────────────────────────────────────────────────────────
//  Formula evaluator — recursive descent
// ─────────────────────────────────────────────────────────────────────────────

struct Evaluator<'a> {
    cells: &'a HashMap<(u32, u32), Cell>,
    input: Vec<char>,
    pos: usize,
}

impl<'a> Evaluator<'a> {
    fn new(expr: &str, cells: &'a HashMap<(u32, u32), Cell>) -> Self {
        Self { cells, input: expr.chars().collect(), pos: 0 }
    }

    fn peek(&self) -> Option<char> { self.input.get(self.pos).copied() }

    fn consume(&mut self) -> Option<char> {
        let c = self.input.get(self.pos).copied();
        self.pos += 1;
        c
    }

    fn skip_ws(&mut self) {
        while self.peek().map_or(false, |c| c == ' ') { self.pos += 1; }
    }

    fn parse_expr(&mut self) -> Option<f64> { self.parse_sum() }

    fn parse_sum(&mut self) -> Option<f64> {
        let mut val = self.parse_product()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('+') => { self.consume(); val += self.parse_product()?; }
                Some('-') => { self.consume(); val -= self.parse_product()?; }
                _ => break,
            }
        }
        Some(val)
    }

    fn parse_product(&mut self) -> Option<f64> {
        let mut val = self.parse_atom()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('*') => { self.consume(); val *= self.parse_atom()?; }
                Some('/') => {
                    self.consume();
                    let d = self.parse_atom()?;
                    if d == 0.0 { return None; }
                    val /= d;
                }
                _ => break,
            }
        }
        Some(val)
    }

    fn parse_atom(&mut self) -> Option<f64> {
        self.skip_ws();
        match self.peek()? {
            '(' => {
                self.consume();
                let v = self.parse_expr()?;
                self.skip_ws();
                if self.peek() == Some(')') { self.consume(); }
                Some(v)
            }
            '-' => { self.consume(); Some(-self.parse_atom()?) }
            c if c.is_ascii_alphabetic() => self.parse_name(),
            c if c.is_ascii_digit() || c == '.' => self.parse_number(),
            _ => None,
        }
    }

    fn parse_number(&mut self) -> Option<f64> {
        let start = self.pos;
        while self.peek().map_or(false, |c| c.is_ascii_digit() || c == '.') {
            self.consume();
        }
        let s: String = self.input[start..self.pos].iter().collect();
        s.parse().ok()
    }

    fn parse_name(&mut self) -> Option<f64> {
        let start = self.pos;
        while self.peek().map_or(false, |c| c.is_ascii_alphanumeric() || c == '_') {
            self.consume();
        }
        let name: String = self.input[start..self.pos].iter().collect();
        let name_upper = name.to_uppercase();

        // Check if it's a function call
        self.skip_ws();
        if self.peek() == Some('(') {
            self.consume(); // consume '('
            return self.parse_function(&name_upper);
        }

        // Check if it's a cell reference like B3
        if let Some(addr) = parse_cell_ref(&name_upper) {
            return Some(self.cells.get(&addr).map_or(0.0, |c| c.numeric()));
        }

        None
    }

    fn parse_function(&mut self, name: &str) -> Option<f64> {
        let mut args: Vec<f64> = Vec::new();

        loop {
            self.skip_ws();
            if self.peek() == Some(')') { self.consume(); break; }

            // Try range first (e.g. A1:C3)
            let range_start = self.pos;
            if let Some(range_vals) = self.try_parse_range() {
                args.extend(range_vals);
            } else {
                self.pos = range_start;
                if let Some(v) = self.parse_expr() {
                    args.push(v);
                }
            }

            self.skip_ws();
            if self.peek() == Some(',') { self.consume(); }
            else if self.peek() == Some(')') { self.consume(); break; }
            else { break; }
        }

        match name {
            "SUM"   => Some(args.iter().sum()),
            "AVG"   => if args.is_empty() { Some(0.0) } else { Some(args.iter().sum::<f64>() / args.len() as f64) },
            "MIN"   => args.iter().copied().reduce(f64::min),
            "MAX"   => args.iter().copied().reduce(f64::max),
            "COUNT" => Some(args.len() as f64),
            _ => None,
        }
    }

    /// Try to parse a range token like "A1:C3". Returns collected numeric values or None.
    fn try_parse_range(&mut self) -> Option<Vec<f64>> {
        let start = self.pos;
        // Collect potential cell ref chars
        while self.peek().map_or(false, |c| c.is_ascii_alphabetic()) { self.consume(); }
        while self.peek().map_or(false, |c| c.is_ascii_digit()) { self.consume(); }
        if self.peek() != Some(':') { self.pos = start; return None; }
        self.consume(); // consume ':'
        while self.peek().map_or(false, |c| c.is_ascii_alphabetic()) { self.consume(); }
        while self.peek().map_or(false, |c| c.is_ascii_digit()) { self.consume(); }

        let range_str: String = self.input[start..self.pos].iter().collect();
        let ((r1, c1), (r2, c2)) = parse_range(&range_str)?;

        let mut vals = Vec::new();
        for r in r1..=r2 {
            for c in c1..=c2 {
                vals.push(self.cells.get(&(r, c)).map_or(0.0, |cell| cell.numeric()));
            }
        }
        Some(vals)
    }
}

fn eval_formula(expr: &str, cells: &HashMap<(u32, u32), Cell>) -> Option<f64> {
    Evaluator::new(expr, cells).parse_expr()
}

// ─────────────────────────────────────────────────────────────────────────────
//  SheetsApp
// ─────────────────────────────────────────────────────────────────────────────

pub struct SheetsApp {
    pub title: String,
    cells: HashMap<(u32, u32), Cell>,
    rows: u32,  // visible rows
    cols: u32,  // visible cols
    selection: (u32, u32),   // (row, col) 0-indexed
    scroll_row: u32,
    scroll_col: u32,
    /// Text being typed into the selected cell (None = not editing)
    edit_buffer: Option<String>,
    pub dirty: bool,
}

impl SheetsApp {
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_string(),
            cells: HashMap::new(),
            rows: 20,
            cols: 8,
            selection: (0, 0),
            scroll_row: 0,
            scroll_col: 0,
            edit_buffer: None,
            dirty: false,
        }
    }

    // ── Internal helpers ───────────────────────────────────────────────────

    fn get_cell(&self, row: u32, col: u32) -> &Cell {
        self.cells.get(&(row, col)).unwrap_or(&Cell::Empty)
    }

    fn set_cell_raw(&mut self, row: u32, col: u32, raw: &str) {
        let cell = if raw.starts_with('=') {
            let expr = raw[1..].to_string();
            let value = eval_formula(&expr, &self.cells);
            Cell::Formula { expr, value }
        } else if let Ok(n) = raw.parse::<f64>() {
            Cell::Number(n)
        } else if raw.is_empty() {
            Cell::Empty
        } else {
            Cell::Text(raw.to_string())
        };
        if matches!(cell, Cell::Empty) {
            self.cells.remove(&(row, col));
        } else {
            self.cells.insert((row, col), cell);
        }
        self.recompute_all_formulas();
        self.dirty = true;
    }

    fn recompute_all_formulas(&mut self) {
        let keys: Vec<(u32, u32)> = self.cells.keys().cloned().collect();
        for key in keys {
            if let Some(Cell::Formula { expr, value }) = self.cells.get(&key).cloned() {
                let new_val = eval_formula(&expr, &self.cells);
                if let Some(Cell::Formula { value: ref mut v, .. }) = self.cells.get_mut(&key) {
                    *v = new_val;
                }
                let _ = (expr, value); // suppress unused warning
            }
        }
    }

    fn commit_edit(&mut self) {
        if let Some(buf) = self.edit_buffer.take() {
            let (row, col) = self.selection;
            self.set_cell_raw(row, col, &buf);
        }
    }

    fn move_selection(&mut self, dr: i32, dc: i32) {
        self.commit_edit();
        let new_row = (self.selection.0 as i32 + dr).max(0) as u32;
        let new_col = (self.selection.1 as i32 + dc).max(0) as u32;
        self.selection = (new_row, new_col);

        // Scroll if needed
        if new_row < self.scroll_row { self.scroll_row = new_row; }
        if new_row >= self.scroll_row + self.rows { self.scroll_row = new_row.saturating_sub(self.rows - 1); }
        if new_col < self.scroll_col { self.scroll_col = new_col; }
        if new_col >= self.scroll_col + self.cols { self.scroll_col = new_col.saturating_sub(self.cols - 1); }
    }

    /// Max non-empty row index (0-indexed), or 0 if no cells.
    fn max_row(&self) -> u32 {
        self.cells.keys().map(|k| k.0).max().unwrap_or(0)
    }

    /// Max non-empty col index (0-indexed), or 0 if no cells.
    fn max_col(&self) -> u32 {
        self.cells.keys().map(|k| k.1).max().unwrap_or(0)
    }

    /// Schema: text values from row 0 (column headers).
    fn schema(&self) -> Vec<String> {
        let mc = self.max_col();
        (0..=mc)
            .map(|c| match self.get_cell(0, c) {
                Cell::Text(s) => s.clone(),
                Cell::Empty => String::new(),
                other => other.display(),
            })
            .collect()
    }

    /// Serialise all non-empty cells to a JSON object keyed by "A1" notation.
    fn cells_json(&self) -> Value {
        let mut obj = serde_json::Map::new();
        for ((r, c), cell) in &self.cells {
            if !cell.is_empty() {
                obj.insert(cell_ref(*r, *c), cell.to_json_value());
            }
        }
        Value::Object(obj)
    }

    fn build_app_state(&self) -> AppState {
        let mr = self.max_row();
        let mc = self.max_col();
        let summary = json!({
            "title": self.title,
            "schema": self.schema(),
            "row_count": mr + 1,
            "col_count": mc + 1,
            "selection": cell_ref(self.selection.0, self.selection.1),
            "dimensions": format!("A1:{}", cell_ref(mr, mc)),
        });
        AppState {
            app_type: "spreadsheet".to_string(),
            summary,
            cells: Some(self.cells_json()),
            dirty: self.dirty,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Layout constants
// ─────────────────────────────────────────────────────────────────────────────

const FORMULA_BAR_H: f32 = 26.0;
const COL_HEADER_H: f32  = 20.0;
const ROW_HEADER_W: f32  = 38.0;
const ROW_H: f32         = 22.0;
const COL_W: f32         = 90.0;

// ─────────────────────────────────────────────────────────────────────────────
//  NativeAppContent impl
// ─────────────────────────────────────────────────────────────────────────────

impl NativeAppContent for SheetsApp {
    fn describe_state(&self) -> AppState {
        self.build_app_state()
    }

    fn execute_action(&mut self, action: &str, params: &Value) -> CapabilityResult {
        match action {
            "read_range" => {
                let range_str = params["range"].as_str().unwrap_or("A1:A1");
                let ((r1, c1), (r2, c2)) = match parse_range(range_str) {
                    Some(r) => r,
                    None => return CapabilityResult {
                        success: false,
                        data: Value::Null,
                        error: Some(CapabilityError::new(ErrorReason::InvalidParam,
                            format!("Invalid range: {}", range_str))),
                        state_delta: None,
                    },
                };
                let mut rows_data: Vec<Vec<Value>> = Vec::new();
                for r in r1..=r2 {
                    let mut row_data = Vec::new();
                    for c in c1..=c2 {
                        row_data.push(self.get_cell(r, c).to_json_value());
                    }
                    rows_data.push(row_data);
                }
                CapabilityResult {
                    success: true,
                    data: json!({ "range": range_str, "values": rows_data }),
                    error: None,
                    state_delta: None,
                }
            }

            "write_cell" => {
                let cell_str = params["cell"].as_str().unwrap_or("");
                let (row, col) = match parse_cell_ref(cell_str) {
                    Some(addr) => addr,
                    None => return CapabilityResult {
                        success: false,
                        data: Value::Null,
                        error: Some(CapabilityError::new(ErrorReason::InvalidParam,
                            format!("Invalid cell reference: {}", cell_str))),
                        state_delta: None,
                    },
                };
                let value = match &params["value"] {
                    Value::Number(n) => n.to_string(),
                    Value::String(s) => s.clone(),
                    Value::Bool(b)   => b.to_string(),
                    Value::Null      => String::new(),
                    other            => other.to_string(),
                };
                self.set_cell_raw(row, col, &value);
                CapabilityResult {
                    success: true,
                    data: json!({ "cell": cell_str, "value": value, "state": self.build_app_state() }),
                    error: None,
                    state_delta: None,
                }
            }

            "apply_formula" => {
                let cell_str = params["cell"].as_str().unwrap_or("");
                let formula = params["formula"].as_str().unwrap_or("");
                let (row, col) = match parse_cell_ref(cell_str) {
                    Some(addr) => addr,
                    None => return CapabilityResult {
                        success: false,
                        data: Value::Null,
                        error: Some(CapabilityError::new(ErrorReason::InvalidParam,
                            format!("Invalid cell reference: {}", cell_str))),
                        state_delta: None,
                    },
                };
                // Normalize: strip leading '=' if present
                let raw = if formula.starts_with('=') { formula.to_string() } else { format!("={}", formula) };
                self.set_cell_raw(row, col, &raw);
                let result = self.get_cell(row, col).display();
                CapabilityResult {
                    success: true,
                    data: json!({ "cell": cell_str, "formula": formula, "result": result, "state": self.build_app_state() }),
                    error: None,
                    state_delta: None,
                }
            }

            _ => CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::UnknownAction,
                    format!("sheets: unknown action '{}'", action))
                    .with_alt("read_range")
                    .with_alt("write_cell")
                    .with_alt("apply_formula")),
                state_delta: None,
            },
        }
    }

    fn render(&self, renderer: &mut Renderer, pixmap: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, cursor_visible: bool) {
        let t = renderer.theme.clone();

        // ── Background ────────────────────────────────────────────────────
        renderer.fill_rect(pixmap, x, y, w, h, t.bg_window_chrome);

        // ── Formula bar ───────────────────────────────────────────────────
        let fb_y = y;
        renderer.fill_rect(pixmap, x, fb_y, w, FORMULA_BAR_H, [28, 30, 38, 255]);
        renderer.fill_rect(pixmap, x, fb_y + FORMULA_BAR_H - 1.0, w, 1.0, [255, 255, 255, 15]);

        // Cell ref label (e.g. "B3")
        let sel_ref = cell_ref(self.selection.0, self.selection.1);
        renderer.draw_text(pixmap, &sel_ref, x + 6.0, fb_y + 7.0, 36.0, 9.0, t.accent);

        // Separator
        renderer.fill_rect(pixmap, x + 42.0, fb_y + 4.0, 1.0, FORMULA_BAR_H - 8.0, [255, 255, 255, 20]);

        // Content: edit buffer or raw cell value
        let formula_content = if let Some(ref buf) = self.edit_buffer {
            if cursor_visible { format!("{}|", buf) } else { buf.clone() }
        } else {
            self.get_cell(self.selection.0, self.selection.1).raw()
        };
        renderer.draw_text(pixmap, &formula_content, x + 48.0, fb_y + 7.0, w - 56.0, 9.5, t.text_primary);

        // ── Grid area ─────────────────────────────────────────────────────
        let grid_y = y + FORMULA_BAR_H;
        let grid_h = h - FORMULA_BAR_H;

        // Column header background
        renderer.fill_rect(pixmap, x, grid_y, w, COL_HEADER_H, [32, 34, 42, 255]);
        renderer.fill_rect(pixmap, x, grid_y + COL_HEADER_H - 1.0, w, 1.0, [255, 255, 255, 20]);

        // Corner cell (top-left of headers)
        renderer.fill_rect(pixmap, x, grid_y, ROW_HEADER_W, COL_HEADER_H, [28, 30, 38, 255]);
        renderer.fill_rect(pixmap, x + ROW_HEADER_W - 1.0, grid_y, 1.0, grid_h, [255, 255, 255, 15]);

        // Column headers: A, B, C...
        let visible_cols = ((w - ROW_HEADER_W) / COL_W).ceil() as u32 + 1;
        for ci in 0..visible_cols {
            let col = self.scroll_col + ci;
            let cx = x + ROW_HEADER_W + ci as f32 * COL_W;
            if cx + COL_W > x + w { break; }

            let is_sel_col = col == self.selection.1;
            let header_bg = if is_sel_col { [45, 48, 60, 255] } else { [32, 34, 42, 255] };
            renderer.fill_rect(pixmap, cx, grid_y, COL_W, COL_HEADER_H, header_bg);
            renderer.fill_rect(pixmap, cx + COL_W - 1.0, grid_y, 1.0, grid_h, [255, 255, 255, 10]);

            let label = col_to_letter(col);
            let lw = label.len() as f32 * 6.5;
            renderer.draw_text(pixmap, &label, cx + (COL_W - lw) / 2.0, grid_y + 5.0, lw + 2.0, 8.5, t.text_muted);
        }

        // Rows
        let cell_area_y = grid_y + COL_HEADER_H;
        let visible_rows = ((grid_h - COL_HEADER_H) / ROW_H).ceil() as u32 + 1;

        for ri in 0..visible_rows {
            let row = self.scroll_row + ri;
            let ry = cell_area_y + ri as f32 * ROW_H;
            if ry > y + h { break; }

            // Row number background
            let is_sel_row = row == self.selection.0;
            let rh_bg = if is_sel_row { [45, 48, 60, 255] } else { [28, 30, 38, 255] };
            renderer.fill_rect(pixmap, x, ry, ROW_HEADER_W, ROW_H, rh_bg);

            let row_label = format!("{}", row + 1);
            let rlw = row_label.len() as f32 * 5.5;
            renderer.draw_text(pixmap, &row_label, x + ROW_HEADER_W - rlw - 6.0, ry + 5.0, rlw + 4.0, 8.5, t.text_muted);

            // Row separator
            renderer.fill_rect(pixmap, x, ry + ROW_H - 1.0, w, 1.0, [255, 255, 255, 8]);

            // Cells
            for ci in 0..visible_cols {
                let col = self.scroll_col + ci;
                let cx = x + ROW_HEADER_W + ci as f32 * COL_W;
                if cx + COL_W > x + w { break; }

                let is_selected = row == self.selection.0 && col == self.selection.1;

                // Cell background
                if is_selected {
                    renderer.fill_rect(pixmap, cx + 1.0, ry + 1.0, COL_W - 2.0, ROW_H - 2.0, [40, 80, 140, 120]);
                    // Selection border
                    renderer.stroke_rect(pixmap, cx, ry, COL_W, ROW_H, t.accent);
                }

                // Cell content
                let display_text = if is_selected {
                    if let Some(ref buf) = self.edit_buffer {
                        if cursor_visible { format!("{}|", buf) } else { buf.clone() }
                    } else {
                        self.get_cell(row, col).display()
                    }
                } else {
                    self.get_cell(row, col).display()
                };

                if !display_text.is_empty() {
                    let cell_val = self.get_cell(row, col);
                    let is_number = matches!(cell_val, Cell::Number(_))
                        || matches!(cell_val, Cell::Formula { .. });
                    let text_color = if is_selected { t.text_primary } else { t.text_secondary };

                    if is_number && self.edit_buffer.is_none() {
                        // Right-align numbers
                        let tw = display_text.len() as f32 * 6.0;
                        let tx = cx + COL_W - tw - 6.0;
                        renderer.draw_text(pixmap, &display_text, tx, ry + 5.0, tw + 4.0, 9.0, text_color);
                    } else {
                        // Left-align text
                        renderer.draw_text(pixmap, &display_text, cx + 4.0, ry + 5.0, COL_W - 8.0, 9.0, text_color);
                    }
                }
            }
        }
    }

    fn on_char(&mut self, c: char) {
        let buf = self.edit_buffer.get_or_insert_with(String::new);
        buf.push(c);
    }

    fn on_backspace(&mut self) {
        if let Some(ref mut buf) = self.edit_buffer {
            buf.pop();
        } else {
            // Clear the selected cell
            let (row, col) = self.selection;
            self.cells.remove(&(row, col));
            self.recompute_all_formulas();
            self.dirty = true;
        }
    }

    fn on_key(&mut self, key: &str) -> Option<AppState> {
        match key {
            "Enter" => {
                self.move_selection(1, 0);
                Some(self.build_app_state())
            }
            "Tab" => {
                self.move_selection(0, 1);
                Some(self.build_app_state())
            }
            "Escape" => {
                self.edit_buffer = None;
                None
            }
            "ArrowUp"    => { self.move_selection(-1, 0); Some(self.build_app_state()) }
            "ArrowDown"  => { self.move_selection(1, 0);  Some(self.build_app_state()) }
            "ArrowLeft"  => { self.move_selection(0, -1); Some(self.build_app_state()) }
            "ArrowRight" => { self.move_selection(0, 1);  Some(self.build_app_state()) }
            _ => None,
        }
    }

    fn on_click(&mut self, rel_x: f32, rel_y: f32) -> Option<AppState> {
        // rel_x/rel_y are relative to content area top-left (below title bar)
        let grid_y_offset = FORMULA_BAR_H + COL_HEADER_H;

        if rel_y < FORMULA_BAR_H || rel_x < ROW_HEADER_W {
            return None; // clicked in formula bar or row header
        }

        let cell_x = rel_x - ROW_HEADER_W;
        let cell_y = rel_y - grid_y_offset;

        if cell_y < 0.0 { return None; }

        let col = self.scroll_col + (cell_x / COL_W) as u32;
        let row = self.scroll_row + (cell_y / ROW_H) as u32;

        self.commit_edit();
        self.selection = (row, col);
        Some(self.build_app_state())
    }

    fn content_type_id(&self) -> crate::window_manager::WindowContentType {
        crate::window_manager::WindowContentType::Sheets
    }
}
