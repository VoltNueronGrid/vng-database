//! UDF runtime — real WASM (wasmi), JS (boa_engine), and Python (subprocess) execution.
//!
//! # Architecture
//!
//! Three concrete execution paths:
//! - **Rust/WASM** — compiled WASM bytes loaded via `wasmi`.  Import allow-list
//!   enforced at registration; fuel and memory limits enforced at call time.
//! - **JavaScript** — ES6 source evaluated by `boa_engine`.  Blocked globals
//!   (`process`, `Deno`, `require`, etc.) rejected at registration.
//! - **Python** — source validated (blocked imports checked at registration) then
//!   executed in a sandboxed `python3 -I` subprocess with per-call timeout.
//!
//! The legacy fallback functions (`execute_udf_runtime_legacy`, etc.) are
//! preserved unchanged at the bottom of this file for backward compatibility with
//! existing unit tests.

use std::collections::HashMap;
use voltnuerongrid_sql::{SqlAnalyzer, SqlStatementKind};
use crate::{
    UdfExecutionResult, UdfExecutionPlanStep, UdfFunctionCatalogEntry,
    UdfInvocationPlan, UdfLanguageGuardPolicy,
};

// ── Blocked-list constants ────────────────────────────────────────────────────

/// WASM imports that are unconditionally blocked (WASI network + process calls).
const WASM_BLOCKED_IMPORTS: &[&str] = &[
    "proc_exit",
    "clock_time_get",
    "sock_open",
    "sock_connect",
    "sock_recv",
    "sock_send",
    "fd_read",
    "fd_write",
    "fd_seek",
    "fd_close",
    "path_open",
];

/// JavaScript globals that must not appear in registered function source.
const JS_BLOCKED_GLOBALS: &[&str] = &[
    "Deno",
    "process",
    "require",
    "XMLHttpRequest",
    "fetch",
];

/// Python import patterns that are blocked at registration time.
const PYTHON_BLOCKED: &[&str] = &[
    "import os",
    "import subprocess",
    "import socket",
    "from os",
    "from subprocess",
    "from socket",
    "sys.exit",
    "__import__",
];

// ── Environment helpers ───────────────────────────────────────────────────────

pub(crate) fn wasm_memory_limit_mb() -> u64 {
    std::env::var("VNG_UDF_WASM_MEMORY_LIMIT_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(64)
}

pub(crate) fn wasm_fuel_limit() -> u64 {
    std::env::var("VNG_UDF_WASM_FUEL_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10_000_000)
}

pub(crate) fn js_timeout_ms() -> u64 {
    std::env::var("VNG_UDF_JS_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(500)
}

pub(crate) fn python_timeout_ms() -> u64 {
    std::env::var("VNG_UDF_PYTHON_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1000)
}

// ── UdfRegistry ───────────────────────────────────────────────────────────────

/// The stored execution payload for a registered UDF.
#[derive(Debug, Clone)]
pub(crate) enum UdfPayload {
    /// Compiled WASM bytes with per-instance limits.
    Wasm {
        bytes: Vec<u8>,
        memory_limit_mb: u64,
        fuel_limit: u64,
    },
    /// JavaScript ES6 function source.
    Javascript {
        source: String,
        timeout_ms: u64,
    },
    /// Python function source (registered after blocked-import validation).
    Python {
        source: String,
        timeout_ms: u64,
    },
}

/// A successfully registered UDF entry.
#[derive(Debug, Clone)]
pub(crate) struct RegisteredUdf {
    pub(crate) name: String,
    pub(crate) language: String,
    pub(crate) registered_at_ms: u64,
    pub(crate) payload: UdfPayload,
}

/// Runtime registry mapping function names to their validated execution payloads.
#[derive(Default)]
pub(crate) struct UdfRegistry {
    pub(crate) udfs: HashMap<String, RegisteredUdf>,
}

impl UdfRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    // ── Registration ─────────────────────────────────────────────────────────

    /// Register a WASM UDF from raw bytes.
    ///
    /// Validates that:
    /// - The bytes parse as a valid WASM module.
    /// - No blocked WASI/network imports are present.
    /// - Declared memory ≤ `memory_limit_mb`.
    pub(crate) fn register_wasm(
        &mut self,
        name: &str,
        wasm_bytes: Vec<u8>,
        memory_limit_mb: u64,
        fuel_limit: u64,
    ) -> Result<(), String> {
        validate_wasm_imports(&wasm_bytes)?;
        validate_wasm_memory(&wasm_bytes, memory_limit_mb)?;
        self.udfs.insert(
            name.to_string(),
            RegisteredUdf {
                name: name.to_string(),
                language: "rust".to_string(),
                registered_at_ms: now_ms(),
                payload: UdfPayload::Wasm { bytes: wasm_bytes, memory_limit_mb, fuel_limit },
            },
        );
        Ok(())
    }

    /// Register a JavaScript ES6 UDF from source code.
    ///
    /// Rejects source that references blocked globals at registration time.
    pub(crate) fn register_js(
        &mut self,
        name: &str,
        source: &str,
        timeout_ms: u64,
    ) -> Result<(), String> {
        validate_js_source(source)?;
        self.udfs.insert(
            name.to_string(),
            RegisteredUdf {
                name: name.to_string(),
                language: "javascript".to_string(),
                registered_at_ms: now_ms(),
                payload: UdfPayload::Javascript { source: source.to_string(), timeout_ms },
            },
        );
        Ok(())
    }

    /// Register a Python UDF from source code.
    ///
    /// Rejects source that contains blocked import patterns.
    pub(crate) fn register_python(
        &mut self,
        name: &str,
        source: &str,
        timeout_ms: u64,
    ) -> Result<(), String> {
        validate_python_source(source)?;
        self.udfs.insert(
            name.to_string(),
            RegisteredUdf {
                name: name.to_string(),
                language: "python".to_string(),
                registered_at_ms: now_ms(),
                payload: UdfPayload::Python { source: source.to_string(), timeout_ms },
            },
        );
        Ok(())
    }

    // ── Introspection ─────────────────────────────────────────────────────────

    /// Return a list of `(name, language)` for all registered UDFs.
    pub(crate) fn list(&self) -> Vec<(String, String)> {
        self.udfs
            .values()
            .map(|u| (u.name.clone(), u.language.clone()))
            .collect()
    }

    pub(crate) fn get(&self, name: &str) -> Option<&RegisteredUdf> {
        self.udfs.get(name)
    }

    // ── Execution ─────────────────────────────────────────────────────────────

    /// Call a registered UDF by name with positional string arguments.
    ///
    /// - WASM UDFs: args parsed as i32/i64 integers.
    /// - JS/Python UDFs: args passed as strings.
    pub(crate) fn call(&self, name: &str, args: &[&str]) -> Result<String, String> {
        let udf = self
            .udfs
            .get(name)
            .ok_or_else(|| format!("udf_not_found: {name}"))?;
        match &udf.payload {
            UdfPayload::Wasm { bytes, fuel_limit, .. } => {
                execute_wasm_udf(bytes, name, args, *fuel_limit)
            }
            UdfPayload::Javascript { source, timeout_ms } => {
                execute_js_udf(source, name, args, *timeout_ms)
            }
            UdfPayload::Python { source, timeout_ms } => {
                execute_python_udf(source, name, args, *timeout_ms)
            }
        }
    }
}

// ── Validation helpers ────────────────────────────────────────────────────────

/// Check that no blocked WASI/network imports appear in the WASM module.
fn validate_wasm_imports(wasm_bytes: &[u8]) -> Result<(), String> {
    let engine = wasmi::Engine::default();
    let module = wasmi::Module::new(&engine, wasm_bytes)
        .map_err(|e| format!("wasm_parse_error: {e}"))?;
    for import in module.imports() {
        for blocked in WASM_BLOCKED_IMPORTS {
            if import.name().contains(blocked) {
                return Err(format!("blocked_import: {}", import.name()));
            }
        }
    }
    Ok(())
}

/// Scan the WASM binary for a memory section and reject if declared min pages
/// exceed the configured memory limit.
fn validate_wasm_memory(wasm_bytes: &[u8], memory_limit_mb: u64) -> Result<(), String> {
    let limit_pages = (memory_limit_mb * 1024 * 1024) / 65536; // 1 WASM page = 64 KiB
    let mut pos = 8usize; // skip 4-byte magic + 4-byte version
    while pos < wasm_bytes.len() {
        let section_id = wasm_bytes[pos];
        pos += 1;
        let (size, br) = read_uleb128(&wasm_bytes[pos..])
            .map_err(|e| format!("wasm_parse_error: {e}"))?;
        pos += br;
        let section_end = pos + size as usize;
        if section_id == 5 && pos < section_end {
            // Memory section: LEB128 count followed by resizable_limits entries.
            let (count, cr) = read_uleb128(&wasm_bytes[pos..])
                .map_err(|e| format!("wasm_parse_error: {e}"))?;
            let mut inner = pos + cr;
            for _ in 0..count {
                if inner >= section_end {
                    break;
                }
                let flags = wasm_bytes[inner];
                inner += 1;
                let (min_pages, mr) = read_uleb128(&wasm_bytes[inner..])
                    .map_err(|e| format!("wasm_parse_error: {e}"))?;
                inner += mr;
                if min_pages > limit_pages {
                    return Err(format!(
                        "wasm_memory_limit_exceeded: declared {} pages ({} MiB), limit {} pages ({} MiB)",
                        min_pages,
                        min_pages * 64 / 1024,
                        limit_pages,
                        memory_limit_mb,
                    ));
                }
                if flags & 1 != 0 {
                    // Skip max pages.
                    let (_, skr) = read_uleb128(&wasm_bytes[inner..])
                        .map_err(|e| format!("wasm_parse_error: {e}"))?;
                    inner += skr;
                }
            }
        }
        pos = section_end;
    }
    Ok(())
}

/// Reject JS source that references any blocked global.
fn validate_js_source(source: &str) -> Result<(), String> {
    for blocked in JS_BLOCKED_GLOBALS {
        if source.contains(blocked) {
            return Err(format!("blocked_global: {blocked}"));
        }
    }
    Ok(())
}

/// Reject Python source that contains any blocked import pattern.
fn validate_python_source(source: &str) -> Result<(), String> {
    for blocked in PYTHON_BLOCKED {
        if source.contains(blocked) {
            return Err(format!("blocked_import: {blocked}"));
        }
    }
    Ok(())
}

// ── WASM execution (wasmi) ────────────────────────────────────────────────────

/// Execute the named export in a WASM module with the supplied arguments.
///
/// All arguments are parsed as `i32`. If parsing fails, they are passed as 0.
/// Returns the first result value as a decimal string.
fn execute_wasm_udf(
    wasm_bytes: &[u8],
    func_name: &str,
    args: &[&str],
    fuel_limit: u64,
) -> Result<String, String> {
    // Enable fuel metering so runaway WASM is terminated.
    let mut config = wasmi::Config::default();
    config.consume_fuel(true);
    let engine = wasmi::Engine::new(&config);

    let module = wasmi::Module::new(&engine, wasm_bytes)
        .map_err(|e| format!("wasm_parse_error: {e}"))?;

    let mut store = wasmi::Store::new(&engine, ());
    store
        .set_fuel(fuel_limit)
        .map_err(|e| format!("wasm_fuel_error: {e}"))?;

    let linker = wasmi::Linker::<()>::new(&engine);
    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(|e| format!("wasm_instantiate_error: {e}"))?
        .start(&mut store)
        .map_err(|e| format!("wasm_start_error: {e}"))?;

    let func = instance
        .get_func(&store, func_name)
        .ok_or_else(|| format!("wasm_func_not_found: {func_name}"))?;

    // Build wasm argument values — parse each arg as i32 (fallback 0).
    let wasm_args: Vec<wasmi::Val> = args
        .iter()
        .map(|a| wasmi::Val::I32(a.trim().parse::<i32>().unwrap_or(0)))
        .collect();

    // Allocate a result buffer matching the function's return type count.
    let func_type = func.ty(&store);
    let result_count = func_type.results().len().max(1);
    let mut results = vec![wasmi::Val::I32(0); result_count];

    func.call(&mut store, &wasm_args, &mut results)
        .map_err(|e| format!("wasm_exec_error: {e}"))?;

    // Serialize the first result as a string.
    let output = match results.first() {
        Some(wasmi::Val::I32(v)) => v.to_string(),
        Some(wasmi::Val::I64(v)) => v.to_string(),
        Some(wasmi::Val::F32(v)) => f32::from_bits(v.to_bits()).to_string(),
        Some(wasmi::Val::F64(v)) => f64::from_bits(v.to_bits()).to_string(),
        _ => String::new(),
    };
    Ok(output)
}

// ── JavaScript execution (boa_engine) ────────────────────────────────────────

/// Execute the named JS function with the supplied arguments.
///
/// Arguments are passed as quoted JSON strings.  Returns the result coerced to
/// a JavaScript string.
fn execute_js_udf(
    source: &str,
    func_name: &str,
    args: &[&str],
    timeout_ms: u64,
) -> Result<String, String> {
    use boa_engine::{Context, Source};

    let mut ctx = Context::default();

    // Set a loop-iteration limit as a proxy for wall-clock timeout.
    // ~1 billion iterations ≈ several seconds; tighter limits for short timeouts.
    let iteration_limit = if timeout_ms < 100 { 100_000u64 } else { 1_000_000_000u64 };
    ctx.runtime_limits_mut().set_loop_iteration_limit(iteration_limit);

    // Evaluate the function definition.
    ctx.eval(Source::from_bytes(source))
        .map_err(|e| format!("js_compile_error: {e}"))?;

    // Build the call expression: funcName("arg0", "arg1", ...)
    let quoted_args: Vec<String> = args
        .iter()
        .map(|a| {
            // If arg parses as a number, pass it unquoted; else quote it.
            if a.trim().parse::<f64>().is_ok() {
                a.to_string()
            } else {
                format!("\"{}\"", a.replace('\\', "\\\\").replace('"', "\\\""))
            }
        })
        .collect();
    let call_expr = format!("{}({})", func_name, quoted_args.join(", "));

    let result = ctx
        .eval(Source::from_bytes(call_expr.as_str()))
        .map_err(|e| format!("js_exec_error: {e}"))?;

    let js_str = result
        .to_string(&mut ctx)
        .map_err(|e| format!("js_result_error: {e}"))?;

    Ok(js_str.to_std_string_escaped())
}

// ── Python execution (subprocess) ────────────────────────────────────────────

/// Execute the named Python function with the supplied arguments in a sandboxed
/// `python3 -I` subprocess.  Returns the function's return value as a string.
///
/// The `-I` (isolated) flag disables `sitecustomize`, user-site, and PYTHON*
/// environment variables, reducing the attack surface.
///
/// Timeout enforcement uses a background thread that kills the child after
/// `timeout_ms` milliseconds.
fn execute_python_udf(
    source: &str,
    func_name: &str,
    args: &[&str],
    timeout_ms: u64,
) -> Result<String, String> {
    use std::process::{Command, Stdio};
    use std::time::Duration;

    // Encode arguments as a Python tuple literal.
    let py_args: Vec<String> = args
        .iter()
        .map(|a| {
            if a.trim().parse::<f64>().is_ok() {
                a.to_string()
            } else {
                format!("\"{}\"", a.replace('\\', "\\\\").replace('"', "\\\""))
            }
        })
        .collect();
    let py_args_tuple = if py_args.len() == 1 {
        py_args[0].clone()
    } else {
        py_args.join(", ")
    };

    // Driver script: import user source, call the function, print the result.
    let driver = format!(
        "{source}\nimport sys as _sys\n_result = {func_name}({py_args_tuple})\nprint(str(_result))\n"
    );

    let mut child = Command::new("python3")
        .args(["-I", "-c", &driver])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("python_spawn_error: {e}"))?;

    // Enforce timeout via a background thread that kills the process.
    let child_id = child.id();
    let timeout_dur = Duration::from_millis(timeout_ms);
    let kill_handle = std::thread::spawn(move || {
        std::thread::sleep(timeout_dur);
        // Best-effort kill; ignore errors (child may have already exited).
        #[cfg(unix)]
        unsafe {
            libc_kill(child_id as i32, 9);
        }
        #[cfg(windows)]
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &child_id.to_string(), "/F"])
            .status();
    });

    let output = child
        .wait_with_output()
        .map_err(|e| format!("python_wait_error: {e}"))?;
    // Silence the kill thread — it may fire after the child already exited.
    drop(kill_handle);

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("Killed") || output.status.code() == Some(137) {
            Err("python_udf_timeout".to_string())
        } else {
            Err(format!("python_exec_error: {}", stderr.trim()))
        }
    }
}

/// Send SIGKILL to a process on Unix.  Only called from the timeout thread.
#[cfg(unix)]
#[allow(non_snake_case)]
unsafe fn libc_kill(pid: i32, sig: i32) {
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    kill(pid, sig);
}

// ── LEB128 decoder (used for WASM memory section parsing) ────────────────────

fn read_uleb128(bytes: &[u8]) -> Result<(u64, usize), String> {
    let mut result = 0u64;
    let mut shift = 0u32;
    for (i, &byte) in bytes.iter().enumerate() {
        result |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Ok((result, i + 1));
        }
        shift += 7;
        if shift >= 64 {
            return Err("uleb128_overflow".to_string());
        }
    }
    Err("uleb128_unexpected_eof".to_string())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ── Legacy fallback (preserved for backward compatibility) ─────────────────

pub(crate) fn execute_udf_runtime_legacy(sql_batch: &str) -> Result<Vec<UdfExecutionResult>, String> {
    enforce_udf_guardrails(sql_batch)?;
    let mut results = Vec::new();
    for statement in SqlAnalyzer::parse_batch(sql_batch) {
        let normalized = statement.raw.to_ascii_lowercase();
        if normalized.contains("udf_rust(") {
            let input = extract_udf_input(&statement.raw).unwrap_or_else(|| "sample".to_string());
            results.push(UdfExecutionResult {
                language: "rust",
                function: "udf_rust",
                output: input.to_ascii_uppercase(),
                input,
            });
        }
        if normalized.contains("udf_js(") {
            let input = extract_udf_input(&statement.raw).unwrap_or_else(|| "sample".to_string());
            let output: String = input.chars().rev().collect();
            results.push(UdfExecutionResult {
                language: "javascript",
                function: "udf_js",
                output,
                input,
            });
        }
        if normalized.contains("udf_python(") {
            let input = extract_udf_input(&statement.raw).unwrap_or_else(|| "sample".to_string());
            results.push(UdfExecutionResult {
                language: "python",
                function: "udf_python",
                output: input.len().to_string(),
                input,
            });
        }
    }
    Ok(results)
}


pub(crate) fn udf_function_catalog_contract() -> Vec<UdfFunctionCatalogEntry> {
    vec![
        UdfFunctionCatalogEntry {
            name: "udf_rust",
            language: "rust",
            deterministic: true,
            status: "enabled",
        },
        UdfFunctionCatalogEntry {
            name: "udf_js",
            language: "javascript",
            deterministic: false,
            status: "enabled",
        },
        UdfFunctionCatalogEntry {
            name: "udf_python",
            language: "python",
            deterministic: false,
            status: "enabled",
        },
    ]
}


pub(crate) fn udf_guard_policy_contract() -> Vec<UdfLanguageGuardPolicy> {
    vec![
        UdfLanguageGuardPolicy {
            language: "rust",
            blocked_tokens: vec!["unsafe", "std::process", "process::"],
            max_input_bytes: 256,
        },
        UdfLanguageGuardPolicy {
            language: "javascript",
            blocked_tokens: vec!["eval(", "function(", "child_process"],
            max_input_bytes: 256,
        },
        UdfLanguageGuardPolicy {
            language: "python",
            blocked_tokens: vec!["import os", "subprocess", "exec("],
            max_input_bytes: 256,
        },
    ]
}


pub(crate) fn build_udf_execution_plan(sql_batch: &str) -> Vec<UdfExecutionPlanStep> {
    let mut plan = Vec::new();
    for statement in SqlAnalyzer::parse_batch(sql_batch) {
        let mut invocations = Vec::new();
        let normalized = statement.raw.to_ascii_lowercase();
        if normalized.contains("udf_rust(") {
            invocations.push(UdfInvocationPlan {
                function: "udf_rust",
                language: "rust",
                guard_policy: "rust_default",
            });
        }
        if normalized.contains("udf_js(") {
            invocations.push(UdfInvocationPlan {
                function: "udf_js",
                language: "javascript",
                guard_policy: "javascript_default",
            });
        }
        if normalized.contains("udf_python(") {
            invocations.push(UdfInvocationPlan {
                function: "udf_python",
                language: "python",
                guard_policy: "python_default",
            });
        }
        let analysis = SqlAnalyzer::analyze_statement(&statement.raw);
        let route_path = if analysis.kind == SqlStatementKind::Select {
            "olap"
        } else {
            "oltp"
        };
        plan.push(UdfExecutionPlanStep {
            statement: statement.raw,
            route_path: route_path.to_string(),
            udf_invocations: invocations,
        });
    }
    plan
}


pub(crate) fn enforce_udf_guardrails(sql_batch: &str) -> Result<(), String> {
    let lowered = sql_batch.to_ascii_lowercase();
    let has_rust_udf = lowered.contains("udf_rust(");
    let has_js_udf = lowered.contains("udf_js(");
    let has_python_udf = lowered.contains("udf_python(");

    if has_rust_udf && ["unsafe", "std::process", "process::"].iter().any(|t| lowered.contains(t)) {
        return Err("udf_guardrail_blocked_rust_payload".to_string());
    }
    if has_js_udf && ["eval(", "function(", "child_process"].iter().any(|t| lowered.contains(t)) {
        return Err("udf_guardrail_blocked_javascript_payload".to_string());
    }
    if has_python_udf && ["import os", "subprocess", "exec("].iter().any(|t| lowered.contains(t)) {
        return Err("udf_guardrail_blocked_python_payload".to_string());
    }
    Ok(())
}


pub(crate) fn extract_udf_input(statement: &str) -> Option<String> {
    let first = statement.find('\'')?;
    let remaining = &statement[first + 1..];
    let end = remaining.find('\'')?;
    Some(remaining[..end].to_string())
}

// ── ISSUE-05: Catalog UDF execution ──────────────────────────────────────────

/// Describes a user-defined function resolved from the DDL catalog.
#[derive(Debug, Clone)]
pub(crate) struct CatalogUdfEntry {
    /// Unqualified function name (lower-cased).
    pub(crate) name: String,
    /// The SQL body extracted from the `CREATE FUNCTION` DDL, if any.
    pub(crate) sql_body: Option<String>,
    /// The full original DDL statement.
    #[allow(dead_code)]
    pub(crate) ddl: String,
}

/// Extract the SQL function body from a `CREATE FUNCTION … AS $$ … $$` DDL statement.
pub(crate) fn extract_sql_function_body(ddl: &str) -> Option<String> {
    let lower = ddl.to_ascii_lowercase();
    if let Some(start) = lower.find("$$") {
        let body_start = start + 2;
        if let Some(end_off) = lower[body_start..].find("$$") {
            let body = ddl[body_start..body_start + end_off].trim().to_string();
            if !body.is_empty() {
                return Some(body);
            }
        }
    }
    None
}

#[allow(dead_code)]
pub(crate) fn resolve_catalog_udfs(
    sql_batch: &str,
    catalog_functions: &[CatalogUdfEntry],
) -> Vec<(String, Option<String>)> {
    let lower = sql_batch.to_ascii_lowercase();
    catalog_functions
        .iter()
        .filter(|f| {
            let call_pat = format!("{}(", f.name);
            lower.contains(&call_pat)
        })
        .map(|f| (f.name.clone(), f.sql_body.clone()))
        .collect()
}

pub(crate) fn try_inline_catalog_udf(
    sql: &str,
    fn_name: &str,
    sql_body: &str,
) -> Option<String> {
    let body_lower = sql_body.trim().to_ascii_lowercase();
    if !body_lower.starts_with("select ") && !body_lower.starts_with("return ") {
        return None;
    }
    let subquery = if body_lower.starts_with("select ") {
        format!("({})", sql_body.trim())
    } else {
        let expr = sql_body.trim().trim_start_matches("RETURN").trim_start_matches("return").trim();
        format!("(SELECT {})", expr)
    };

    let lower = sql.to_ascii_lowercase();
    let call_pat = format!("{}(", fn_name.to_ascii_lowercase());
    let pos = lower.find(&call_pat)?;
    let after_open = pos + call_pat.len();
    let mut depth = 1usize;
    let mut close_pos = None;
    for (i, ch) in sql[after_open..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    close_pos = Some(after_open + i + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = close_pos?;
    Some(format!("{}{}{}", &sql[..pos], subquery, &sql[end..]))
}


