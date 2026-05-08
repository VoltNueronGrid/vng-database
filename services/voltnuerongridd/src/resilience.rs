//! Resilience helpers for handler code.
//!
//! Production handlers must never panic on a poisoned mutex — a panic in any
//! handler poisons the global `tokio` task, which can in turn poison shared
//! state and cascade. The repo-wide cursor rule (`.cursorrules`) explicitly
//! forbids `unwrap()` / `expect()` in handler paths.
//!
//! This module provides small helpers so handlers can be concise *and* safe.
//!
//! # Migration notes
//!
//! There are 346 `.lock().expect("…")` call sites in `main.rs` as of this
//! commit. They cannot all be fixed in a single PR without a working compiler
//! (we are on Ubuntu's rustc 1.75 which is below the project's MSRV; see
//! remaining.md). The plan is:
//!
//! 1. Land this helper.
//! 2. Migrate the SQL data-plane handlers (highest QPS, highest blast radius).
//! 3. Migrate admin handlers in a follow-up PR.
//! 4. Migrate service-startup paths last (those legitimately want to fail-fast).

#![forbid(unsafe_code)]

use std::sync::{Mutex, MutexGuard};

/// Outcome of a `lock_or_503` attempt: either a guard, or the structured
/// reason for service-unavailable.
pub enum LockOutcome<'a, T> {
    Held(MutexGuard<'a, T>),
    Poisoned { resource: &'static str },
}

/// Try to take a mutex; on poisoning, return a `Poisoned` outcome instead of
/// panicking. The caller decides how to surface that to its protocol layer
/// (HTTP 503, native error frame, etc.).
///
/// # Why not just `lock().unwrap_or_else(|e| e.into_inner())`?
///
/// Recovering the inner guard from a poisoned mutex *can* be safe, but only
/// if the invariants protected by the mutex are still upheld. In practice,
/// when one of our paths panics inside a critical section, we don't know
/// what state the row store / catalog is in — so the conservative thing is
/// to refuse new work on that mutex and let the operator restart the service
/// (or, eventually, hot-swap the affected sub-component).
pub fn lock_or_unavailable<'a, T>(
    mutex: &'a Mutex<T>,
    resource: &'static str,
) -> LockOutcome<'a, T> {
    match mutex.lock() {
        Ok(guard) => LockOutcome::Held(guard),
        Err(_) => LockOutcome::Poisoned { resource },
    }
}

/// Macro used in handlers: bind a guard or return a 503 response.
///
/// ```ignore
/// let rs = handler_lock!(state.row_store, "row_store");
/// // rs is now a MutexGuard<PagedRowStore>
/// ```
#[macro_export]
macro_rules! handler_lock {
    ($mutex:expr, $resource:literal) => {
        match $crate::resilience::lock_or_unavailable(&$mutex, $resource) {
            $crate::resilience::LockOutcome::Held(g) => g,
            $crate::resilience::LockOutcome::Poisoned { resource } => {
                tracing::error!(
                    target: "vng.handler",
                    resource = resource,
                    "mutex poisoned; refusing request"
                );
                return Err((
                    axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    axum::Json(serde_json::json!({
                        "status": "error",
                        "kind": "internal_state_unavailable",
                        "resource": resource,
                    })),
                ));
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_held_for_unpoisoned_mutex() {
        let m = Mutex::new(7u32);
        match lock_or_unavailable(&m, "test") {
            LockOutcome::Held(g) => assert_eq!(*g, 7),
            LockOutcome::Poisoned { .. } => panic!("expected Held"),
        };
    }

    #[test]
    fn returns_poisoned_after_panic_in_critical_section() {
        let m = std::sync::Arc::new(Mutex::new(0u32));
        let m2 = std::sync::Arc::clone(&m);
        let _ = std::thread::spawn(move || {
            let _g = m2.lock().expect("first take");
            panic!("intentional panic to poison");
        })
        .join();
        match lock_or_unavailable(&m, "test") {
            LockOutcome::Held(_) => panic!("expected Poisoned"),
            LockOutcome::Poisoned { resource } => assert_eq!(resource, "test"),
        };
    }
}
