pub mod ast;
pub mod error;
pub mod from_json;
pub mod interpreter;
pub mod json;
pub mod parser;
pub mod tree;
pub mod validate;

use error::MOTLYError;
use tree::MOTLYDataNode;

pub use interpreter::{
    ExecContext, SessionOptions, Transformer,
    flatten, chunk, topo_sort, execute_chunked,
    ChunkResult, TopoSortResult,
};
pub use validate::{validate_references, validate_schema, SchemaError, ValidationError};

// ── Core API ───────────────────────────────────────────────────────

/// The result of parsing MOTLY source.
pub struct MOTLYResult {
    pub value: MOTLYDataNode,
    pub errors: Vec<MOTLYError>,
}

/// Parse MOTLY source and execute against the given value using the four-phase
/// interpreter. Returns the updated value and any errors.
pub fn parse_motly(input: &str, mut value: MOTLYDataNode, ctx: &ExecContext) -> MOTLYResult {
    match parser::parse(input) {
        Ok(stmts) => {
            let transformers = flatten(&stmts, ctx);
            let chunk_result = chunk(&transformers);
            let sort_result = topo_sort(&chunk_result.deps);
            let errors = execute_chunked(
                &transformers,
                &chunk_result.splits,
                &sort_result.order,
                &mut value,
                &ctx.options,
            );
            MOTLYResult { value, errors }
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
///
/// # Safety
/// `ptr` must have been returned by `alloc` or a `wasm_*` function,
/// and `len` must match the original allocation size.
#[no_mangle]
pub unsafe extern "C" fn dealloc(ptr: *mut u8, len: usize) {
    let layout = std::alloc::Layout::from_size_align(len, 1).unwrap();
    unsafe { std::alloc::dealloc(ptr, layout) };
}

// ── Session-based WASM FFI ──────────────────────────────────────────

use crate::ast::Statement;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

struct AccumulatedParse {
    stmts: Vec<Statement>,
    parse_id: u32,
}

struct Session {
    accumulated: Vec<AccumulatedParse>,
    value: MOTLYDataNode,
    schema: Option<MOTLYDataNode>,
    next_parse_id: u32,
    options: SessionOptions,
    finished: bool,
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

/// Create a new session holding an empty MOTLYDataNode. Returns a session ID.
#[no_mangle]
pub extern "C" fn wasm_session_new() -> u32 {
    wasm_session_new_with_options(0)
}

/// Create a new session with options encoded as bit flags.
/// Bit 0: disable_references (1 = true, 0 = false).
/// Pass `0` for default behavior.
#[no_mangle]
pub extern "C" fn wasm_session_new_with_options(flags: u32) -> u32 {
    let options = SessionOptions {
        disable_references: flags & 1 != 0,
    };
    let id = next_id();
    with_sessions(|s| {
        s.insert(
            id,
            Session {
                accumulated: Vec::new(),
                value: MOTLYDataNode::new(),
                schema: None,
                next_parse_id: 0,
                options,
                finished: false,
            },
        )
    });
    id
}

/// Parse source and accumulate statements. Returns only syntax errors.
/// Returns a pointer to a null-terminated JSON object: `{"parseId":N,"errors":[...]}`.
///
/// # Safety
/// `src_ptr` must point to a valid UTF-8 byte sequence of length `src_len`.
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
    let parse_id = with_sessions(|s| match s.get_mut(&id) {
        Some(session) if session.finished => None,
        Some(session) => {
            let pid = session.next_parse_id;
            session.next_parse_id += 1;
            Some(pid)
        }
        None => None,
    });
    let parse_id = match parse_id {
        Some(pid) => pid,
        None => {
            return string_to_c_ptr(json::parse_result_to_json(0, &[MOTLYError {
                code: "session-error".to_string(),
                message: "Session is spent after finish() — create a new session".to_string(),
                begin: error::Position { line: 0, column: 0, offset: 0 },
                end: error::Position { line: 0, column: 0, offset: 0 },
            }]));
        }
    };
    match parser::parse(input) {
        Ok(stmts) => {
            with_sessions(|s| {
                if let Some(session) = s.get_mut(&id) {
                    session.accumulated.push(AccumulatedParse { stmts, parse_id });
                }
            });
            let json_str = json::parse_result_to_json(parse_id, &[]);
            string_to_c_ptr(json_str)
        }
        Err(err) => {
            let json_str = json::parse_result_to_json(parse_id, &[err]);
            string_to_c_ptr(json_str)
        }
    }
}

/// Interpret all accumulated statements using the four-phase engine,
/// validate references, and store the result.
/// Returns a pointer to a null-terminated JSON object: `{"errors":[...]}`.
#[no_mangle]
pub extern "C" fn wasm_session_finish(id: u32) -> *const u8 {
    let session_data = with_sessions(|s| {
        match s.get_mut(&id) {
            Some(session) if session.finished => None,
            Some(session) => {
                session.finished = true;
                let accumulated = std::mem::take(&mut session.accumulated);
                let options = session.options.clone();
                Some((accumulated, options))
            }
            None => None,
        }
    });
    let (accumulated, options) = match session_data {
        Some(data) => data,
        None => return string_to_c_ptr("{\"errors\":[]}".to_string()),
    };

    let mut root = MOTLYDataNode::new();
    let mut all_errors: Vec<MOTLYError> = Vec::new();

    // Phase 1: Flatten all accumulated statements into transformers
    let mut all_transformers: Vec<Transformer> = Vec::new();
    for ap in &accumulated {
        let ctx = ExecContext { parse_id: ap.parse_id, options: options.clone() };
        all_transformers.extend(flatten(&ap.stmts, &ctx));
    }

    // Phase 2: Chunk
    let chunk_result = chunk(&all_transformers);

    // Phase 3: Topo-sort
    let sort_result = topo_sort(&chunk_result.deps);

    // Phase 4: Execute
    let exec_errors = execute_chunked(
        &all_transformers,
        &chunk_result.splits,
        &sort_result.order,
        &mut root,
        &options,
    );
    all_errors.extend(exec_errors);

    // Validate references (unless disabled)
    if !options.disable_references {
        let ref_errors = validate_references(&root);
        for re in ref_errors {
            all_errors.push(MOTLYError {
                code: re.code.to_string(),
                message: re.message,
                begin: error::Position { line: 0, column: 0, offset: 0 },
                end: error::Position { line: 0, column: 0, offset: 0 },
            });
        }
    }

    with_sessions(|s| {
        if let Some(session) = s.get_mut(&id) {
            session.value = root;
        }
    });

    let json_str = json::errors_to_json(&all_errors);
    string_to_c_ptr(json_str)
}

/// Parse MOTLY source as a schema and store it in the session.
/// Returns a pointer to a null-terminated JSON object: `{"parseId":N,"errors":[...]}`.
///
/// # Safety
/// `src_ptr` must point to a valid UTF-8 byte sequence of length `src_len`.
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
    let ctx = with_sessions(|s| match s.get_mut(&id) {
        Some(session) => {
            let pid = session.next_parse_id;
            session.next_parse_id += 1;
            ExecContext { parse_id: pid, options: SessionOptions { disable_references: true } }
        }
        None => ExecContext { parse_id: 0, options: SessionOptions::default() },
    });
    let result = parse_motly(input, MOTLYDataNode::new(), &ctx);
    with_sessions(|s| {
        if let Some(session) = s.get_mut(&id) {
            session.schema = Some(result.value);
        }
    });
    let json_str = json::parse_result_to_json(ctx.parse_id, &result.errors);
    string_to_c_ptr(json_str)
}

/// Reset the session's value to empty, keeping the schema.
#[no_mangle]
pub extern "C" fn wasm_session_reset(id: u32) {
    with_sessions(|s| {
        if let Some(session) = s.get_mut(&id) {
            session.value = MOTLYDataNode::new();
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
fn string_to_c_ptr(s: String) -> *const u8 {
    let mut bytes = s.into_bytes();
    bytes.push(0);
    let boxed = bytes.into_boxed_slice();
    Box::into_raw(boxed) as *mut u8
}

/// Convenience wrapper for tests: parse_motly with default options.
#[cfg(test)]
fn parse_motly_n(input: &str, value: MOTLYDataNode, parse_id: u32) -> MOTLYResult {
    parse_motly(input, value, &ExecContext { parse_id, options: SessionOptions::default() })
}

#[cfg(test)]
fn parse_motly_0(input: &str, value: MOTLYDataNode) -> MOTLYResult {
    parse_motly_n(input, value, 0)
}

/// Full session lifecycle for tests: parse all inputs, run four-phase interpreter,
/// optionally validate refs. Returns (value, all_errors).
#[cfg(test)]
fn session_finish_ex(inputs: &[&str], options: SessionOptions, validate_refs: bool) -> (MOTLYDataNode, Vec<MOTLYError>) {
    let mut root = MOTLYDataNode::new();
    let mut all_errors: Vec<MOTLYError> = Vec::new();
    let mut all_transformers: Vec<Transformer> = Vec::new();

    for (i, input) in inputs.iter().enumerate() {
        match parser::parse(input) {
            Ok(stmts) => {
                let ctx = ExecContext { parse_id: i as u32, options: options.clone() };
                all_transformers.extend(flatten(&stmts, &ctx));
            }
            Err(err) => {
                all_errors.push(err);
            }
        }
    }

    let chunk_result = chunk(&all_transformers);
    let sort_result = topo_sort(&chunk_result.deps);
    let exec_errors = execute_chunked(
        &all_transformers,
        &chunk_result.splits,
        &sort_result.order,
        &mut root,
        &options,
    );
    all_errors.extend(exec_errors);

    if validate_refs && !options.disable_references {
        let ref_errors = validate_references(&root);
        for re in ref_errors {
            all_errors.push(MOTLYError {
                code: re.code.to_string(),
                message: re.message,
                begin: error::Position { line: 0, column: 0, offset: 0 },
                end: error::Position { line: 0, column: 0, offset: 0 },
            });
        }
    }

    (root, all_errors)
}

#[cfg(test)]
fn session_finish(inputs: &[&str], options: SessionOptions) -> (MOTLYDataNode, Vec<MOTLYError>) {
    session_finish_ex(inputs, options, true)
}

#[cfg(test)]
mod tests;
