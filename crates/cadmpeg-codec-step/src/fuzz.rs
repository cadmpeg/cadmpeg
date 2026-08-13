//! Feature-gated entry points for focused parser fuzzing.

/// Exercise STEP lexical scanning.
pub fn lex(data: &[u8]) {
    let _ = crate::lex::lex(data);
}

/// Exercise STEP entity parsing.
pub fn parse(data: &[u8]) {
    let _ = crate::parse::parse(data);
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::units::Units;
    use cadmpeg_ir::CadIr;

    #[test]
    fn wrappers_accept_empty() {
        super::lex(&[]);
        super::parse(&[]);
    }

    #[test]
    fn wrappers_accept_exported_document() {
        let source = crate::test_support::export(&CadIr::empty(Units::default()));
        super::lex(source.as_bytes());
        super::parse(source.as_bytes());
    }
}
