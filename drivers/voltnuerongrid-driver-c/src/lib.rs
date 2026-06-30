/*!
 * VoltNueronGrid C FFI binding layer.
 *
 * Exposes a thin `#[no_mangle] extern "C"` surface over a minimal in-process
 * driver handle. A `cbindgen`-generated header (`voltnuerongrid.h`) is the
 * canonical C interface; the hand-written header in the repo root is the
 * reference copy.
 *
 * Build:
 *   cargo build --release -p vng-driver-c
 *
 * Output artefacts (in target/release/):
 *   libvoltnuerongrid_driver.so   (cdylib)
 *   libvoltnuerongrid_driver.a    (staticlib)
 */

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};

// ---------------------------------------------------------------------------
// Opaque handle
// ---------------------------------------------------------------------------

/// Opaque driver handle returned to C callers via `vng_driver_create`.
///
/// C consumers must treat this as an opaque pointer and free it with
/// `vng_driver_free`.
pub struct VngDriverHandle {
    base_url: String,
    session_id: String,
    mode: String,
}

// ---------------------------------------------------------------------------
// Request structure (C-compatible layout)
// ---------------------------------------------------------------------------

/// A built HTTP request.  C callers own the heap-allocated strings; they must
/// call `vng_request_free` to release them.
#[repr(C)]
pub struct VngRequest {
    /// HTTP method: 0 = GET, 1 = POST.
    pub method: c_int,
    /// Null-terminated URL string (heap-allocated, owned by caller after return).
    pub url: *mut c_char,
    /// Null-terminated JSON object of headers (heap-allocated).
    pub headers_json: *mut c_char,
    /// Null-terminated JSON body, or NULL for GET requests (heap-allocated).
    pub body_json: *mut c_char,
}

impl Default for VngRequest {
    fn default() -> Self {
        VngRequest {
            method: 0,
            url: std::ptr::null_mut(),
            headers_json: std::ptr::null_mut(),
            body_json: std::ptr::null_mut(),
        }
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Safely converts a C string pointer to an owned `String`.
/// Returns `None` if the pointer is null or the bytes are not valid UTF-8.
unsafe fn c_str_to_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    CStr::from_ptr(ptr).to_str().ok().map(|s| s.to_owned())
}

/// Allocates a `CString` from a `&str` and returns the raw pointer.
/// The caller becomes responsible for freeing the memory.
fn string_to_c(s: &str) -> *mut c_char {
    match CString::new(s) {
        Ok(cs) => cs.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

fn build_headers_json(handle: &VngDriverHandle) -> String {
    let pairs = vec![
        format!(r#""content-type":"application/json""#),
        format!(r#""x-vng-session-id":"{}""#, handle.session_id),
    ];
    if handle.mode == "admin" || handle.mode == "operator" {
        // In a real implementation the API key would be stored in the handle.
        // Placeholder: callers set credentials via extended create functions.
    }
    format!("{{{}}}", pairs.join(","))
}

// ---------------------------------------------------------------------------
// Public C API
// ---------------------------------------------------------------------------

/// Creates a new driver handle.
///
/// # Safety
/// All pointer arguments must be valid null-terminated C strings or NULL.
/// The returned pointer must be freed with `vng_driver_free`.
/// Returns NULL on allocation failure or if `base_url`/`session_id` are NULL/empty.
#[no_mangle]
pub unsafe extern "C" fn vng_driver_create(
    base_url: *const c_char,
    session_id: *const c_char,
    mode: *const c_char,
) -> *mut VngDriverHandle {
    let base_url = match c_str_to_string(base_url) {
        Some(s) if !s.trim().is_empty() => s,
        _ => return std::ptr::null_mut(),
    };
    let session_id = match c_str_to_string(session_id) {
        Some(s) if !s.trim().is_empty() => s,
        _ => return std::ptr::null_mut(),
    };
    let mode = c_str_to_string(mode).unwrap_or_else(|| "admin".to_owned());

    let handle = Box::new(VngDriverHandle {
        base_url,
        session_id,
        mode,
    });
    Box::into_raw(handle)
}

/// Frees a driver handle previously created by `vng_driver_create`.
///
/// # Safety
/// `handle` must have been returned by `vng_driver_create` and must not be
/// used after this call.  Passing NULL is a no-op.
#[no_mangle]
pub unsafe extern "C" fn vng_driver_free(handle: *mut VngDriverHandle) {
    if !handle.is_null() {
        drop(Box::from_raw(handle));
    }
}

/// Builds a GET /health request into `*out`.
///
/// Returns 0 on success, non-zero on error.
///
/// # Safety
/// `handle` and `out` must be valid non-null pointers.
/// The caller must call `vng_request_free(out)` when done.
#[no_mangle]
pub unsafe extern "C" fn vng_driver_build_health_request(
    handle: *const VngDriverHandle,
    out: *mut VngRequest,
) -> c_int {
    if handle.is_null() || out.is_null() {
        return -1;
    }
    let h = &*handle;
    let base = h.base_url.trim_end_matches('/');
    let url = format!("{}/health", base);

    (*out).method = 0; // GET
    (*out).url = string_to_c(&url);
    (*out).headers_json = string_to_c(&build_headers_json(h));
    (*out).body_json = std::ptr::null_mut();

    if (*out).url.is_null() || (*out).headers_json.is_null() {
        return -2;
    }
    0
}

/// Builds a POST /api/v1/sql/execute request into `*out`.
///
/// Returns 0 on success, non-zero on error.
///
/// # Safety
/// `handle`, `sql_batch`, and `out` must be valid non-null pointers.
/// The caller must call `vng_request_free(out)` when done.
#[no_mangle]
pub unsafe extern "C" fn vng_driver_build_sql_execute_request(
    handle: *const VngDriverHandle,
    sql_batch: *const c_char,
    out: *mut VngRequest,
) -> c_int {
    if handle.is_null() || sql_batch.is_null() || out.is_null() {
        return -1;
    }
    let h = &*handle;
    let sql = match c_str_to_string(sql_batch) {
        Some(s) if !s.trim().is_empty() => s,
        _ => return -1,
    };
    let base = h.base_url.trim_end_matches('/');
    let url = format!("{}/api/v1/sql/execute", base);
    // Minimal JSON encoding — only escapes double-quote and backslash.
    let escaped = sql.replace('\\', "\\\\").replace('"', "\\\"");
    let body = format!(r#"{{"sql_batch":"{}"}}"#, escaped);

    (*out).method = 1; // POST
    (*out).url = string_to_c(&url);
    (*out).headers_json = string_to_c(&build_headers_json(h));
    (*out).body_json = string_to_c(&body);

    if (*out).url.is_null() || (*out).headers_json.is_null() || (*out).body_json.is_null() {
        return -2;
    }
    0
}

/// Frees heap-allocated strings inside a `VngRequest`.
///
/// Does NOT free the `VngRequest` struct itself (if it was stack-allocated by
/// the caller, it doesn't need to be freed separately).
///
/// # Safety
/// `req` must be a valid non-null pointer.  Each non-null string field must
/// have been allocated by this library.  After this call, all string fields
/// are set to NULL.
#[no_mangle]
pub unsafe extern "C" fn vng_request_free(req: *mut VngRequest) {
    if req.is_null() {
        return;
    }
    let r = &mut *req;
    if !r.url.is_null() {
        drop(CString::from_raw(r.url));
        r.url = std::ptr::null_mut();
    }
    if !r.headers_json.is_null() {
        drop(CString::from_raw(r.headers_json));
        r.headers_json = std::ptr::null_mut();
    }
    if !r.body_json.is_null() {
        drop(CString::from_raw(r.body_json));
        r.body_json = std::ptr::null_mut();
    }
}

// ===========================================================================
// D-2: End-to-end SQL execution FFI (connect → execute → iterate → free)
// ===========================================================================
//
// These functions perform real HTTP I/O (blocking, via `ureq`) so C callers
// can run SQL without assembling requests by hand.  A `VngConn` is a per-thread
// connection handle; a `VngResult` owns a fully-materialised result set.

/// Opaque connection handle returned by `vng_connect`.
pub struct VngConn {
    base_url: String,
    admin_key: String,
}

/// Opaque result set returned by `vng_execute`. Owns all row data.
pub struct VngResult {
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
    /// Cursor: `usize::MAX` means "before first row" (call `vng_result_next`).
    cursor: usize,
    /// Cache of the current row's column values as C strings (kept alive until
    /// the next `vng_result_next` call or `vng_result_free`).
    cell_cache: Vec<CString>,
}

/// Parse a `/api/v1/sql/execute` JSON response body into `(columns, rows)`.
///
/// Handles the canonical VNG shapes:
/// * `columns: ["a","b"]`, `rows: [["1","x"], ["2","y"]]`
/// * `rows: [{"a":"1","b":"x"}, ...]` (object rows — columns inferred from keys)
/// Scalar cells are stringified; missing cells become empty strings.
fn parse_execute_response(body: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return (Vec::new(), Vec::new()),
    };
    // Column list, if present.
    let mut columns: Vec<String> = v
        .get("columns")
        .and_then(|c| c.as_array())
        .map(|arr| arr.iter().map(json_scalar_to_string).collect())
        .unwrap_or_default();

    let rows_val = v.get("rows").and_then(|r| r.as_array());
    let mut rows: Vec<Vec<String>> = Vec::new();
    if let Some(arr) = rows_val {
        for row in arr {
            match row {
                serde_json::Value::Array(cells) => {
                    rows.push(cells.iter().map(json_scalar_to_string).collect());
                }
                serde_json::Value::Object(map) => {
                    // Infer/extend the column order from object keys on first sight.
                    if columns.is_empty() {
                        columns = map.keys().cloned().collect();
                    }
                    let cells = columns
                        .iter()
                        .map(|c| map.get(c).map(json_scalar_to_string).unwrap_or_default())
                        .collect();
                    rows.push(cells);
                }
                other => rows.push(vec![json_scalar_to_string(other)]),
            }
        }
    }
    (columns, rows)
}

fn json_scalar_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Connect to a VoltNueronGrid server.
///
/// `host` e.g. `"127.0.0.1"`, `port` e.g. `8080`, `admin_key` is the admin API
/// key (may be NULL/empty for unauthenticated/health-only use).
///
/// Returns an opaque `*mut VngConn` (free with `vng_disconnect`) or NULL on
/// invalid arguments.
///
/// # Safety
/// `host` and `admin_key` must be valid null-terminated C strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn vng_connect(
    host: *const c_char,
    port: c_int,
    admin_key: *const c_char,
) -> *mut VngConn {
    let host = match c_str_to_string(host) {
        Some(s) if !s.trim().is_empty() => s,
        _ => return std::ptr::null_mut(),
    };
    if port <= 0 || port > 65535 {
        return std::ptr::null_mut();
    }
    let admin_key = c_str_to_string(admin_key).unwrap_or_default();
    let base_url = format!("http://{}:{}", host.trim_end_matches('/'), port);
    Box::into_raw(Box::new(VngConn { base_url, admin_key }))
}

/// Execute a SQL batch and return a fully-materialised result set.
///
/// Returns NULL on transport error, non-2xx HTTP status, or invalid arguments.
/// The returned `*mut VngResult` must be freed with `vng_result_free`.
///
/// # Safety
/// `conn` must be a live handle from `vng_connect`; `sql` a valid C string.
#[no_mangle]
pub unsafe extern "C" fn vng_execute(
    conn: *const VngConn,
    sql: *const c_char,
) -> *mut VngResult {
    if conn.is_null() {
        return std::ptr::null_mut();
    }
    let c = &*conn;
    let sql = match c_str_to_string(sql) {
        Some(s) if !s.trim().is_empty() => s,
        _ => return std::ptr::null_mut(),
    };
    let url = format!("{}/api/v1/sql/execute", c.base_url);
    let body = serde_json::json!({ "sql_batch": sql }).to_string();

    let mut req = ureq::post(&url).set("content-type", "application/json");
    if !c.admin_key.is_empty() {
        req = req.set("x-vng-admin-key", &c.admin_key);
    }
    let resp_body = match req.send_string(&body) {
        Ok(resp) => resp.into_string().unwrap_or_default(),
        // ureq returns Err for non-2xx; surface the body so callers can parse it
        // if they want, but here we treat it as an execution failure → NULL.
        Err(ureq::Error::Status(_, resp)) => {
            let _ = resp.into_string();
            return std::ptr::null_mut();
        }
        Err(_) => return std::ptr::null_mut(),
    };
    let (columns, rows) = parse_execute_response(&resp_body);
    Box::into_raw(Box::new(VngResult {
        columns,
        rows,
        cursor: usize::MAX,
        cell_cache: Vec::new(),
    }))
}

/// Number of rows in the result set.
///
/// # Safety
/// `result` must be a live handle from `vng_execute`.
#[no_mangle]
pub unsafe extern "C" fn vng_result_row_count(result: *const VngResult) -> c_int {
    if result.is_null() {
        return -1;
    }
    (*result).rows.len() as c_int
}

/// Number of columns in the result set.
///
/// # Safety
/// `result` must be a live handle from `vng_execute`.
#[no_mangle]
pub unsafe extern "C" fn vng_result_column_count(result: *const VngResult) -> c_int {
    if result.is_null() {
        return -1;
    }
    (*result).columns.len() as c_int
}

/// Advance the row cursor. Returns 1 if a row is now current, 0 if there are no
/// more rows, -1 on invalid argument. Must be called before the first
/// `vng_result_get_str`.
///
/// # Safety
/// `result` must be a live handle from `vng_execute`.
#[no_mangle]
pub unsafe extern "C" fn vng_result_next(result: *mut VngResult) -> c_int {
    if result.is_null() {
        return -1;
    }
    let r = &mut *result;
    let next = if r.cursor == usize::MAX { 0 } else { r.cursor + 1 };
    if next >= r.rows.len() {
        r.cursor = r.rows.len();
        r.cell_cache.clear();
        return 0;
    }
    r.cursor = next;
    // Refresh the per-row C-string cache.
    r.cell_cache = r.rows[next]
        .iter()
        .map(|s| CString::new(s.as_str()).unwrap_or_default())
        .collect();
    1
}

/// Return the value of column `col` in the current row as a null-terminated C
/// string. The pointer is valid until the next `vng_result_next` or
/// `vng_result_free`. Returns NULL if there is no current row or `col` is out
/// of range.
///
/// # Safety
/// `result` must be a live handle from `vng_execute`.
#[no_mangle]
pub unsafe extern "C" fn vng_result_get_str(
    result: *const VngResult,
    col: c_int,
) -> *const c_char {
    if result.is_null() || col < 0 {
        return std::ptr::null();
    }
    let r = &*result;
    match r.cell_cache.get(col as usize) {
        Some(cs) => cs.as_ptr(),
        None => std::ptr::null(),
    }
}

/// Free a result set returned by `vng_execute`.
///
/// # Safety
/// `result` must have been returned by `vng_execute` and not used afterwards.
/// NULL is a no-op.
#[no_mangle]
pub unsafe extern "C" fn vng_result_free(result: *mut VngResult) {
    if !result.is_null() {
        drop(Box::from_raw(result));
    }
}

/// Disconnect and free a connection handle from `vng_connect`.
///
/// # Safety
/// `conn` must have been returned by `vng_connect` and not used afterwards.
/// NULL is a no-op.
#[no_mangle]
pub unsafe extern "C" fn vng_disconnect(conn: *mut VngConn) {
    if !conn.is_null() {
        drop(Box::from_raw(conn));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_columnar_response() {
        let body = r#"{"status":"ok","columns":["id","name"],"rows":[["1","alice"],["2","bob"]]}"#;
        let (cols, rows) = parse_execute_response(body);
        assert_eq!(cols, vec!["id", "name"]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec!["1", "alice"]);
        assert_eq!(rows[1], vec!["2", "bob"]);
    }

    #[test]
    fn parses_object_rows_inferring_columns() {
        let body = r#"{"rows":[{"id":"1","name":"alice"},{"id":"2","name":"bob"}]}"#;
        let (cols, rows) = parse_execute_response(body);
        assert!(cols.contains(&"id".to_string()) && cols.contains(&"name".to_string()));
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn parses_numeric_scalars_to_strings() {
        let body = r#"{"columns":["n"],"rows":[[1],[2.5],[null]]}"#;
        let (_cols, rows) = parse_execute_response(body);
        assert_eq!(rows[0], vec!["1"]);
        assert_eq!(rows[1], vec!["2.5"]);
        assert_eq!(rows[2], vec![""]);
    }

    #[test]
    fn result_cursor_iteration_via_ffi() {
        // Build a VngResult by hand (no server needed) and iterate it via the FFI.
        let result = Box::into_raw(Box::new(VngResult {
            columns: vec!["id".into(), "name".into()],
            rows: vec![vec!["1".into(), "alice".into()], vec!["2".into(), "bob".into()]],
            cursor: usize::MAX,
            cell_cache: Vec::new(),
        }));
        unsafe {
            assert_eq!(vng_result_row_count(result), 2);
            assert_eq!(vng_result_column_count(result), 2);
            // First row.
            assert_eq!(vng_result_next(result), 1);
            let name = std::ffi::CStr::from_ptr(vng_result_get_str(result, 1))
                .to_str().unwrap().to_string();
            assert_eq!(name, "alice");
            // Second row.
            assert_eq!(vng_result_next(result), 1);
            let name2 = std::ffi::CStr::from_ptr(vng_result_get_str(result, 1))
                .to_str().unwrap().to_string();
            assert_eq!(name2, "bob");
            // Exhausted.
            assert_eq!(vng_result_next(result), 0);
            assert!(vng_result_get_str(result, 0).is_null());
            vng_result_free(result);
        }
    }

    #[test]
    fn connect_validates_arguments() {
        unsafe {
            // Invalid port → NULL.
            let host = CString::new("127.0.0.1").unwrap();
            assert!(vng_connect(host.as_ptr(), 0, std::ptr::null()).is_null());
            assert!(vng_connect(host.as_ptr(), 70000, std::ptr::null()).is_null());
            // Valid → non-null; then disconnect.
            let conn = vng_connect(host.as_ptr(), 8080, std::ptr::null());
            assert!(!conn.is_null());
            vng_disconnect(conn);
        }
    }
}
