// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::CodecError;

#[test]
fn replacement_index_checks_exposed_key_order_and_duplicates() {
    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    struct TextKey(u8, String);
    impl AsRef<str> for TextKey {
        fn as_ref(&self) -> &str {
            &self.1
        }
    }
    for texts in [["later", "earlier"], ["same", "same"]] {
        let replacements = std::collections::BTreeMap::from([
            (TextKey(0, texts[0].to_owned()), "first".to_owned()),
            (TextKey(1, texts[1].to_owned()), "second".to_owned()),
        ]);
        assert!(
            matches!(super::ReplacementIndex::build(replacements.iter()), Err(CodecError::Malformed(message)) if message == "text replacements must have unique keys in byte order")
        );
    }
}

#[test]
fn replacement_index_looks_up_keys_in_byte_order() {
    let replacements = std::collections::BTreeMap::from([
        ("é".to_owned(), "third".to_owned()),
        ("alpha".to_owned(), "first".to_owned()),
        ("beta".to_owned(), "second".to_owned()),
    ]);
    let index = super::ReplacementIndex::build(replacements.iter()).unwrap();
    assert_eq!(
        index.values,
        [("alpha", "first"), ("beta", "second"), ("é", "third")]
    );
    assert_eq!(index.get("alpha"), Some("first"));
    assert_eq!(index.get("é"), Some("third"));
    assert_eq!(index.get("missing"), None);
}
