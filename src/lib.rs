pub mod ast;
pub mod error;
pub mod from_json;
pub mod interpreter;
pub mod json;
pub mod parser;
pub mod tree;
pub mod validate;

use error::MOTLYError;
use tree::MOTLYNode;

pub use validate::{validate_references, validate_schema, SchemaError, ValidationError};

// ── Core API ───────────────────────────────────────────────────────

/// The result of parsing MOTLY source.
pub struct MOTLYResult {
    pub value: MOTLYNode,
    pub errors: Vec<MOTLYError>,
}

/// Parse MOTLY source and execute statements against the given value,
/// returning the updated value and any errors (parse errors + non-fatal execution errors).
pub fn parse_motly(input: &str, mut value: MOTLYNode) -> MOTLYResult {
    match parser::parse(input) {
        Ok(stmts) => {
            let exec_errors = interpreter::execute(&stmts, &mut value);
            MOTLYResult {
                value,
                errors: exec_errors,
            }
        }
        Err(err) => MOTLYResult {
            value,
            errors: vec![err],
        },
    }
}

// ── WASM FFI ────────────────────────────────────────────────────────

/// Allocate `len` bytes in WASM memory, returning a pointer.
/// The caller must free the returned pointer with `dealloc(ptr, len)`.
#[no_mangle]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let layout = std::alloc::Layout::from_size_align(len, 1).unwrap();
    unsafe { std::alloc::alloc(layout) }
}

/// Free a buffer previously returned by `alloc` or by any of the
/// `wasm_*` functions. For null-terminated strings returned by those
/// functions, pass `strlen(ptr) + 1` as `len`.
#[no_mangle]
pub unsafe extern "C" fn dealloc(ptr: *mut u8, len: usize) {
    let layout = std::alloc::Layout::from_size_align(len, 1).unwrap();
    unsafe { std::alloc::dealloc(ptr, layout) };
}

// ── Session-based WASM FFI ──────────────────────────────────────────

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

struct Session {
    value: MOTLYNode,
    schema: Option<MOTLYNode>,
}

// WASM is single-threaded, so thread_local is just a convenient safe wrapper.
thread_local! {
    static SESSIONS: RefCell<HashMap<u32, Session>> = RefCell::new(HashMap::new());
    static NEXT_SESSION_ID: Cell<u32> = const { Cell::new(1) };
}

fn with_sessions<R>(f: impl FnOnce(&mut HashMap<u32, Session>) -> R) -> R {
    SESSIONS.with(|s| f(&mut s.borrow_mut()))
}

fn next_id() -> u32 {
    NEXT_SESSION_ID.with(|c| {
        let id = c.get();
        c.set(id + 1);
        id
    })
}

/// Create a new session holding an empty MOTLYNode. Returns a session ID.
#[no_mangle]
pub extern "C" fn wasm_session_new() -> u32 {
    let id = next_id();
    with_sessions(|s| {
        s.insert(
            id,
            Session {
                value: MOTLYNode::new(),
                schema: None,
            },
        )
    });
    id
}

/// Parse source and apply it to the session's value in place.
/// Returns a pointer to a null-terminated JSON array of errors.
#[no_mangle]
pub unsafe extern "C" fn wasm_session_parse(
    id: u32,
    src_ptr: *const u8,
    src_len: usize,
) -> *const u8 {
    let input = unsafe {
        let slice = std::slice::from_raw_parts(src_ptr, src_len);
        std::str::from_utf8_unchecked(slice)
    };
    let value = with_sessions(|s| match s.get_mut(&id) {
        Some(session) => Some(std::mem::replace(&mut session.value, MOTLYNode::new())),
        None => None,
    });
    let value = match value {
        Some(v) => v,
        None => return string_to_c_ptr("[]".to_string()),
    };
    let result = parse_motly(input, value);
    with_sessions(|s| {
        if let Some(session) = s.get_mut(&id) {
            session.value = result.value;
        }
    });
    let json_str = json::errors_to_json(&result.errors);
    string_to_c_ptr(json_str)
}

/// Parse MOTLY source as a schema and store it in the session.
/// Returns a pointer to a null-terminated JSON array of parse errors.
#[no_mangle]
pub unsafe extern "C" fn wasm_session_parse_schema(
    id: u32,
    src_ptr: *const u8,
    src_len: usize,
) -> *const u8 {
    let input = unsafe {
        let slice = std::slice::from_raw_parts(src_ptr, src_len);
        std::str::from_utf8_unchecked(slice)
    };
    let result = parse_motly(input, MOTLYNode::new());
    with_sessions(|s| {
        if let Some(session) = s.get_mut(&id) {
            session.schema = Some(result.value);
        }
    });
    let json_str = json::errors_to_json(&result.errors);
    string_to_c_ptr(json_str)
}

/// Reset the session's value to empty, keeping the schema.
#[no_mangle]
pub extern "C" fn wasm_session_reset(id: u32) {
    with_sessions(|s| {
        if let Some(session) = s.get_mut(&id) {
            session.value = MOTLYNode::new();
        }
    });
}

/// Serialize the session's current value to wire-format JSON.
/// Returns a pointer to a null-terminated JSON string.
#[no_mangle]
pub extern "C" fn wasm_session_get_value(id: u32) -> *const u8 {
    with_sessions(|s| match s.get(&id) {
        Some(session) => string_to_c_ptr(json::to_wire(&session.value)),
        None => string_to_c_ptr("{}".to_string()),
    })
}

/// Validate references in the session's value.
/// Returns a pointer to a null-terminated JSON array of validation errors.
#[no_mangle]
pub extern "C" fn wasm_session_validate_refs(id: u32) -> *const u8 {
    with_sessions(|s| match s.get(&id) {
        Some(session) => {
            let errors = validate_references(&session.value);
            string_to_c_ptr(json::validation_errors_to_json(&errors))
        }
        None => string_to_c_ptr("[]".to_string()),
    })
}

/// Validate the session's value against its stored schema.
/// Returns `[]` if no schema has been set.
/// Returns a pointer to a null-terminated JSON array of schema errors.
#[no_mangle]
pub extern "C" fn wasm_session_validate_schema(id: u32) -> *const u8 {
    with_sessions(|s| match s.get(&id) {
        Some(session) => match &session.schema {
            Some(schema) => {
                let errors = validate_schema(&session.value, schema);
                string_to_c_ptr(json::schema_errors_to_json(&errors))
            }
            None => string_to_c_ptr("[]".to_string()),
        },
        None => string_to_c_ptr("[]".to_string()),
    })
}

/// Free a session, dropping its value and schema.
#[no_mangle]
pub extern "C" fn wasm_session_free(id: u32) {
    with_sessions(|s| s.remove(&id));
}

/// Convert a String to a null-terminated C pointer with exact allocation size.
/// The allocation size is exactly `s.len() + 1` bytes, so the caller can
/// free with `dealloc(ptr, strlen(ptr) + 1)`.
fn string_to_c_ptr(s: String) -> *const u8 {
    let mut bytes = s.into_bytes();
    bytes.push(0);
    // into_boxed_slice guarantees allocation size == bytes.len()
    let boxed = bytes.into_boxed_slice();
    Box::into_raw(boxed) as *mut u8
}

#[cfg(test)]
mod tests;
