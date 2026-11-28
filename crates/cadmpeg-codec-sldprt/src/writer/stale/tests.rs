// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy};

/// The stale mark edits the stored text of a document whose external subset
/// declares an entity the text references.
#[test]
fn the_stale_mark_keeps_the_text_of_a_document_with_an_unread_external_subset() {
    let source = r#"<!DOCTYPE swSolidWorks SYSTEM 'sw.dtd'><swSolidWorks><swModel swNote="&units;"/><swConfiguration swName="A" swConfigurationNeedsUpdate="NO"/><swConfiguration swName="B" swID="1"/></swSolidWorks>"#;
    let expected = r#"<!DOCTYPE swSolidWorks SYSTEM 'sw.dtd'><swSolidWorks><swModel swNote="&units;"/><swConfiguration swName="A" swConfigurationNeedsUpdate="YES"/><swConfiguration swName="B" swConfigurationNeedsUpdate="YES" swID="1"/></swSolidWorks>"#;
    let names = BTreeSet::from(["A".to_owned(), "B".to_owned()]);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let (marked, rows) = super::mark(&ctx, source.as_bytes(), &names)
        .unwrap()
        .unwrap();
    assert_eq!(rows, ["A", "B"]);
    assert_eq!(String::from_utf8(marked).unwrap(), expected);
}
