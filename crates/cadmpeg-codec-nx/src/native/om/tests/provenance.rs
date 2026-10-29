// SPDX-License-Identifier: Apache-2.0
//! Source owner tokens do not become document identities.

use cadmpeg_core::decode::DecodeContext;

#[test]
fn native_part_attribute_owner_token_is_not_parsed_as_an_identity() {
    let source = br#"<UgAttributes version="4"><Attribute owner="00000001" title="Title" value="Value" type="string" pdmBased="false" version="1"/></UgAttributes>"#;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy).expect("bounded source");
    let attributes = super::super::parse_part_attributes(&ctx, source, 0, "/Root/part/attrs", 0)
        .expect("typed native attribute ownership")
        .expect("actual NX attribute grammar");
    assert_eq!(attributes.len(), 1);
    assert_eq!(attributes[0].owner, "00000001");
}
