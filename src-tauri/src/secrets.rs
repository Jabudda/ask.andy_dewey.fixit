//! API keys live in the macOS Keychain (service "com.projectnextchapter.app"), never in SQLite,
//! logs, exports, backups or the repository. The UI only ever learns whether a key is present.

use crate::error::{AppError, AppResult};

pub const SERVICE: &str = "com.projectnextchapter.app";
pub const NAMES: [&str; 2] = ["usajobs", "anthropic"];

fn entry(name: &str) -> AppResult<keyring::Entry> {
    if !NAMES.contains(&name) {
        return Err(AppError::invalid("Unknown credential"));
    }
    keyring::Entry::new(SERVICE, name).map_err(|e| AppError::other(format!("Keychain unavailable: {e}")))
}

/// Development override (examples / CI): `NEXT_CHAPTER_<NAME>_KEY` environment variable.
fn env_override(name: &str) -> Option<String> {
    std::env::var(format!("NEXT_CHAPTER_{}_KEY", name.to_uppercase())).ok().filter(|v| !v.trim().is_empty())
}

pub fn set(name: &str, value: &str) -> AppResult<()> {
    let v = value.trim();
    if v.len() < 8 || v.contains(char::is_whitespace) {
        return Err(AppError::invalid("That doesn't look like an API key"));
    }
    entry(name)?.set_password(v).map_err(|e| AppError::other(format!("Could not save to Keychain: {e}")))
}

pub fn get(name: &str) -> AppResult<Option<String>> {
    if let Some(v) = env_override(name) {
        return Ok(Some(v));
    }
    match entry(name)?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::other(format!("Could not read from Keychain: {e}"))),
    }
}

pub fn delete(name: &str) -> AppResult<()> {
    match entry(name)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::other(format!("Could not remove from Keychain: {e}"))),
    }
}

pub fn has(name: &str) -> bool {
    get(name).map(|v| v.is_some()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_names_and_bad_values() {
        assert!(entry("github").is_err());
        assert!(set("usajobs", "short").is_err());
        assert!(set("usajobs", "has spaces in it").is_err());
    }
}
