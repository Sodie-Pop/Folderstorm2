//! The bridge's bearer token: 32 bytes from the OS random source, new each
//! time the bridge starts, compared in constant time. It is written to the
//! discovery file only: never logged, shown or copied.

use subtle::ConstantTimeEq;

/// Bytes of randomness in a token (64 hex digits on the wire).
const BYTES: usize = 32;

#[derive(Clone)]
pub struct Token(String);

impl Token {
    /// A fresh token from the OS random source.
    pub fn generate() -> std::io::Result<Token> {
        let mut bytes = [0u8; BYTES];
        getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(format!("no random source: {e}")))?;
        Ok(Token(bytes.iter().map(|b| format!("{b:02x}")).collect()))
    }

    /// A token read back from a discovery file (the sidecar's side).
    pub fn from_secret(secret: String) -> Token {
        Token(secret)
    }

    /// The secret itself, for the discovery file and the Authorization
    /// header only.
    pub fn secret(&self) -> &str {
        &self.0
    }

    /// Whether `candidate` is this token, in time independent of where the
    /// two differ (only a length mismatch returns early).
    pub fn matches(&self, candidate: &str) -> bool {
        self.0.as_bytes().ct_eq(candidate.as_bytes()).into()
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_64_hex_digits_and_new_each_time() {
        let a = Token::generate().expect("random source");
        let b = Token::generate().expect("random source");
        assert_eq!(a.secret().len(), 64);
        assert!(a.secret().bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a.secret(), b.secret());
    }

    #[test]
    fn only_the_same_secret_matches() {
        let token = Token::generate().expect("random source");
        assert!(token.matches(&token.secret().to_owned()));
        let mut other = token.secret().to_owned();
        let last = if other.ends_with('0') { "1" } else { "0" };
        other.replace_range(63.., last);
        assert!(!token.matches(&other));
        assert!(!token.matches(""));
        assert!(!token.matches(&token.secret()[..63]));
        assert!(!token.matches(&format!("{}0", token.secret())));
    }

    #[test]
    fn debug_never_prints_the_secret() {
        let token = Token::generate().expect("random source");
        let shown = format!("{token:?}");
        assert!(!shown.contains(token.secret()));
        assert_eq!(shown, "Token(<redacted>)");
    }
}
