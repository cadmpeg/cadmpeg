// SPDX-License-Identifier: Apache-2.0

use super::entry_referenced;
use indexmap::IndexMap;

#[test]
fn archive_references_match_escaped_xml_attributes() {
    let mut properties = IndexMap::new();
    properties.insert(
        "Attachment".to_owned(),
        "<File file=\"drawing&amp;notes.svg\"/>".to_owned(),
    );
    assert!(entry_referenced("drawing&notes.svg", &[], &properties, ""));
    assert!(!entry_referenced("drawing", &[], &properties, ""));
}
