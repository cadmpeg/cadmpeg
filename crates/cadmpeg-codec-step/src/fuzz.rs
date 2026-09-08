// SPDX-License-Identifier: Apache-2.0
//! `()`-returning wrappers over internal parsers for the `cadmpeg-fuzz` targets.
//!
//! Each wrapper feeds arbitrary bytes to one internal parser and discards the
//! result. The contract is that no input may panic.
#![doc(hidden)]

/// Exercise STEP lexical scanning.
pub fn lex(data: &[u8]) {
    let _ = crate::lex::lex(data);
}

/// Exercise STEP entity parsing.
pub fn parse(data: &[u8]) {
    let _ = crate::parse::parse(data);
}

/// Entity count for the parse benchmark; hides the typed exchange.
#[doc(hidden)]
pub fn parse_entity_count(data: &[u8]) -> Option<usize> {
    crate::parse::parse(data)
        .ok()
        .map(|(exchange, _)| exchange.records.len())
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::CadIr;

    #[test]
    fn wrappers_accept_empty() {
        super::lex(&[]);
        super::parse(&[]);
    }

    #[test]
    fn wrappers_accept_exported_document() {
        let source = crate::test_support::export(&CadIr::empty());
        super::lex(source.as_bytes());
        super::parse(source.as_bytes());
        assert!(super::parse_entity_count(source.as_bytes()).is_some());
    }
}
