//! Feature-gated entry points for focused parser fuzzing.

/// Exercise STEP lexical scanning.
pub fn lex(data: &[u8]) {
    let _ = crate::lex::lex(data);
}

/// Exercise STEP entity parsing.
pub fn parse(data: &[u8]) {
    let _ = crate::parse::parse(data);
}
