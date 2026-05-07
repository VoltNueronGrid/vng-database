//! Time utilities.


// ─── S7-WS6-04: Chaos/game-day injection handlers ────────────────────────────

pub(crate) fn now_epoch_ms_chaos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}


pub(crate) fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}


pub(crate) fn now_unix_ms_u64() -> u64 {
    now_unix_ms().min(u128::from(u64::MAX)) as u64
}

