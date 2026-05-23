//! WASM plugin host for user-defined capabilities.
//!
//! Plugins are `.wasm` files in `~/.soma/plugins/`. Each must export:
//!   name()          -> (ptr: i32, len: i32)
//!   description()   -> (ptr: i32, len: i32)
//!   actions_json()  -> (ptr: i32, len: i32)   // JSON: Vec<ActionSchema>
//!   alloc(len: i32) -> i32
//!   execute(action_ptr, action_len, params_ptr, params_len) -> (ptr: i32, len: i32)
//!
//! Host imports available to plugins:
//!   soma_log(ptr, len)
//!   soma_http_get(url_ptr, url_len, out_ptr, out_max) -> i32

use log::{info, warn};
use serde_json::Value;
use soma_common::{ActionSchema, CapabilityError, CapabilityResult, ErrorReason};
use std::sync::Mutex;
use wasmtime::{Caller, Engine, Instance, Linker, Memory, Module, Store, TypedFunc};

use super::Capability;

// ── Host state ────────────────────────────────────────────────────────────────

struct WasmHostState {
    memory: Option<Memory>,
}

// ── Plugin internals behind a mutex ──────────────────────────────────────────

struct WasmInner {
    store:    Store<WasmHostState>,
    instance: Instance,
}

pub struct WasmPlugin {
    name_cache:        String,
    description_cache: String,
    actions_cache:     Vec<ActionSchema>,
    inner:             Mutex<WasmInner>,
}

impl WasmPlugin {
    pub fn from_file(path: &std::path::Path) -> Result<Self, Box<dyn std::error::Error>> {
        let engine = Engine::default();
        let module = Module::from_file(&engine, path)?;

        let mut linker: Linker<WasmHostState> = Linker::new(&engine);
        register_host_functions(&mut linker)?;

        let mut store = Store::new(&engine, WasmHostState { memory: None });
        let instance = linker.instantiate(&mut store, &module)?;

        // Wire memory
        if let Some(mem) = instance.get_memory(&mut *(&mut store), "memory") {
            store.data_mut().memory = Some(mem);
        }

        let name         = read_str_export(&instance, &mut store, "name")?;
        let description  = read_str_export(&instance, &mut store, "description")?;
        let actions_json = read_str_export(&instance, &mut store, "actions_json")?;
        let actions: Vec<ActionSchema> = serde_json::from_str(&actions_json)
            .map_err(|e| format!("actions_json parse: {}", e))?;

        info!("Loaded WASM plugin '{}' ({} actions) from {:?}", name, actions.len(), path);

        Ok(WasmPlugin {
            name_cache:        name,
            description_cache: description,
            actions_cache:     actions,
            inner: Mutex::new(WasmInner { store, instance }),
        })
    }
}

impl Capability for WasmPlugin {
    fn name(&self)        -> &str { &self.name_cache }
    fn description(&self) -> &str { &self.description_cache }
    fn version(&self)     -> &str { "wasm-1.0" }
    fn actions(&self)     -> Vec<ActionSchema> { self.actions_cache.clone() }

    fn execute(&self, action: &str, params: &Value) -> CapabilityResult { state_delta: None,
        let mut guard = match self.inner.lock() {
            Ok(g) => g,
            Err(_) => return wasm_error("plugin lock poisoned"),
        };
        let WasmInner { ref mut store, ref instance } = *guard;

        let action_bytes = action.as_bytes().to_vec();
        let params_str   = params.to_string();
        let params_bytes = params_str.as_bytes().to_vec();

        // Allocate space in plugin memory
        let alloc: TypedFunc<i32, i32> = match instance.get_typed_func::<i32, i32>(&mut *store, "alloc") {
            Ok(f) => f,
            Err(_) => return wasm_error("plugin missing 'alloc' export"),
        };
        let action_ptr = match alloc.call(&mut *store, action_bytes.len() as i32) {
            Ok(p) => p,
            Err(e) => return wasm_error(&format!("alloc failed: {}", e)),
        };
        let params_ptr = match alloc.call(&mut *store, params_bytes.len() as i32) {
            Ok(p) => p,
            Err(e) => return wasm_error(&format!("alloc failed: {}", e)),
        };

        // Write bytes into plugin memory
        let mem = match store.data().memory {
            Some(m) => m,
            None => return wasm_error("plugin has no memory"),
        };
        {
            let data = mem.data_mut(&mut *store);
            let a = action_ptr as usize;
            data[a..a + action_bytes.len()].copy_from_slice(&action_bytes);
            let p = params_ptr as usize;
            data[p..p + params_bytes.len()].copy_from_slice(&params_bytes);
        }

        // Call execute
        let exec: TypedFunc<(i32, i32, i32, i32), (i32, i32)> =
            match instance.get_typed_func(&mut *store, "execute") {
                Ok(f) => f,
                Err(_) => return wasm_error("plugin missing 'execute' export"),
            };
        let (out_ptr, out_len) = match exec.call(
            &mut *store,
            (action_ptr, action_bytes.len() as i32, params_ptr, params_bytes.len() as i32),
        ) {
            Ok(r) => r,
            Err(e) => return wasm_error(&format!("execute trap: {}", e)),
        };

        // Read JSON result
        let json_str = {
            let data = mem.data(&*store);
            let s = out_ptr as usize;
            let e = s + out_len as usize;
            if e > data.len() {
                return wasm_error("returned out-of-bounds pointer");
            }
            match std::str::from_utf8(&data[s..e]) {
                Ok(t) => t.to_string(),
                Err(e) => return wasm_error(&format!("result not UTF-8: {}", e)),
            }
        };

        serde_json::from_str::<CapabilityResult>(&json_str)
            .unwrap_or_else(|e| wasm_error(&format!("result parse: {} (got: {})", e, &json_str[..json_str.len().min(200)])))
    }
}

// ── Host functions ────────────────────────────────────────────────────────────

fn register_host_functions(linker: &mut Linker<WasmHostState>) -> Result<(), Box<dyn std::error::Error>> {
    linker.func_wrap(
        "env",
        "soma_log",
        |caller: Caller<'_, WasmHostState>, ptr: i32, len: i32| {
            if let Some(mem) = caller.data().memory {
                let data = mem.data(&caller);
                let s = ptr as usize;
                let e = s + len as usize;
                if e <= data.len() {
                    if let Ok(msg) = std::str::from_utf8(&data[s..e]) {
                        info!("[wasm] {}", msg);
                    }
                }
            }
        },
    )?;

    linker.func_wrap(
        "env",
        "soma_http_get",
        |mut caller: Caller<'_, WasmHostState>,
         url_ptr: i32,
         url_len: i32,
         out_ptr: i32,
         out_max: i32|
         -> i32 {
            let mem = match caller.data().memory {
                Some(m) => m,
                None => return 0,
            };
            let url = {
                let data = mem.data(&caller);
                let s = url_ptr as usize;
                let e = s + url_len as usize;
                if e > data.len() {
                    return 0;
                }
                match std::str::from_utf8(&data[s..e]) {
                    Ok(u) => u.to_string(),
                    Err(_) => return 0,
                }
            };

            let body = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    reqwest::get(&url).await?.text().await
                })
            });

            match body {
                Ok(text) => {
                    let bytes = text.as_bytes();
                    let to_write = bytes.len().min(out_max as usize);
                    let data = mem.data_mut(&mut caller);
                    let s = out_ptr as usize;
                    if s + to_write <= data.len() {
                        data[s..s + to_write].copy_from_slice(&bytes[..to_write]);
                        to_write as i32
                    } else {
                        0
                    }
                }
                Err(_) => 0,
            }
        },
    )?;

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn read_str_export(
    instance: &Instance,
    store: &mut Store<WasmHostState>,
    name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let f: TypedFunc<(), (i32, i32)> = instance
        .get_typed_func(&mut *store, name)
        .map_err(|_| format!("plugin missing '{}' export", name))?;
    let (ptr, len) = f.call(&mut *store, ())?;
    let mem = store.data().memory.ok_or("plugin has no memory")?;
    let data = mem.data(&*store);
    let s = ptr as usize;
    let e = s + len as usize;
    if e > data.len() {
        return Err("out-of-bounds pointer from export".into());
    }
    Ok(std::str::from_utf8(&data[s..e])?.to_string())
}

fn wasm_error(msg: &str) -> CapabilityResult { state_delta: None,
    CapabilityResult {
        success: false,
        data: Value::Null,
        error: Some(CapabilityError::new(ErrorReason::InternalError, msg.to_string())),
        state_delta: None,
    }
}

// ── Plugin scanner ────────────────────────────────────────────────────────────

pub fn load_wasm_plugins() -> Vec<Box<dyn Capability>> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let plugin_dir = std::path::PathBuf::from(home).join(".soma").join("plugins");

    if !plugin_dir.exists() {
        return Vec::new();
    }

    let entries = match std::fs::read_dir(&plugin_dir) {
        Ok(e) => e,
        Err(e) => {
            warn!("Cannot read plugin dir {:?}: {}", plugin_dir, e);
            return Vec::new();
        }
    };

    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|ext| ext == "wasm").unwrap_or(false))
        .filter_map(|e| {
            let path = e.path();
            match WasmPlugin::from_file(&path) {
                Ok(p) => Some(Box::new(p) as Box<dyn Capability>),
                Err(err) => {
                    warn!("Failed to load {:?}: {}", path, err);
                    None
                }
            }
        })
        .collect()
}
