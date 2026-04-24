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
