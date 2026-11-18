//! The guard that keeps what must never leave the PC out of a backup:
//! passwords, tokens, saved logins, session and capability URLs, e-mail
//! addresses. A backup file is carried around and shared.
//!
//! The sections are written not to hold any of these (the tests check each
//! one with a profile full of them, `super::tests`); this guard is the
//! second line: any key whose name says it holds one, and any capability
//! URL, is removed from a section's value when it is written and again
//! when one is read.

use serde_json::Value;

/// Parts of key names that hold a secret or a login, in lower case.
const DENIED: &[&str] = &[
    "password",
    "passwd",
    "token",
    "secret",
    "api_key",
    "apikey",
    "mfa",
    "credential",
    "cookie",
    "session_id",
    "secure_session",
    "seed_capability",
    "capability",
    "cap_url",
    "email",
    "e_mail",
    "login_uri",
    "username",
    "last_login",
    "authorization",
];

/// Whether a key's name says it holds a secret or a login.
pub fn denied(key: &str) -> bool {
    let key = key.to_ascii_lowercase().replace('-', "_");
    DENIED.iter().any(|part| key.contains(part))
}

/// Whether a text is a capability URL (`…/cap/<id>`), or an address with
/// a user and password in it.
pub fn private_url(text: &str) -> bool {
    let lower = text.trim().to_ascii_lowercase();
    let Some(rest) = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://")) else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    host.contains('@') || rest.contains("/cap/")
}

/// Removes from `value` every key [`denied`] names and every value that is
/// a [`private_url`]; how many were removed.
pub fn scrub(value: &mut Value) -> usize {
    match value {
        Value::Object(map) => {
            let before = map.len();
            map.retain(|key, inner| !denied(key) && !inner.as_str().is_some_and(private_url));
            let mut removed = before - map.len();
            for inner in map.values_mut() {
                removed += scrub(inner);
            }
            removed
        }
        Value::Array(items) => {
            let before = items.len();
            items.retain(|inner| !inner.as_str().is_some_and(private_url));
            let mut removed = before - items.len();
            for inner in items.iter_mut() {
                removed += scrub(inner);
            }
            removed
        }
        _ => 0,
    }
}

/// The paths of the keys [`scrub`] would remove (for the tests and the log).
pub fn findings(value: &Value) -> Vec<String> {
    let mut out = Vec::new();
    walk(value, "", &mut out);
    out
}

fn walk(value: &Value, path: &str, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map {
                let here = format!("{path}.{key}");
                if denied(key) || inner.as_str().is_some_and(private_url) {
                    out.push(here);
                } else {
                    walk(inner, &here, out);
                }
            }
        }
        Value::Array(items) => {
            for (index, inner) in items.iter().enumerate() {
                let here = format!("{path}[{index}]");
                if inner.as_str().is_some_and(private_url) {
                    out.push(here);
                } else {
                    walk(inner, &here, out);
                }
            }
        }
        _ => {}
    }
}
