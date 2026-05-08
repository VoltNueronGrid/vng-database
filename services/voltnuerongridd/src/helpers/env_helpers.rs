//! Environment variable helpers.
use std::env;


pub(crate) fn read_env_bool(name: &str, default: bool) -> bool {
    match env::var(name) {
        Ok(value) => value.trim().eq_ignore_ascii_case("true"),
        Err(_) => default,
    }
}


pub(crate) fn read_env_usize(name: &str, default: usize) -> usize {
    match env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    {
        Some(value) => match value.parse::<usize>() {
            Ok(parsed) => parsed,
            Err(_) => {
                eprintln!("Invalid {name}={value}; using default {default}");
                default
            }
        },
        None => default,
    }
}


pub(crate) fn read_env_u64(name: &str, default: u64) -> u64 {
    match env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    {
        Some(value) => match value.parse::<u64>() {
            Ok(parsed) => parsed,
            Err(_) => {
                eprintln!("Invalid {name}={value}; using default {default}");
                default
            }
        },
        None => default,
    }
}

