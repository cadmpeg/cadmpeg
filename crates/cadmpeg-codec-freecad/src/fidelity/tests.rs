// SPDX-License-Identifier: Apache-2.0
//! Source-only records retain exact bytes.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn value_ranges_cannot_bind_to_a_different_source_element_or_value_order() {
    let xml = "<Property><Bool value='1'/></Property>";
    let arena = DecodeArena::new();
    let (_ctx, root) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service()).unwrap();
    assert!(super::relative_xml_extent(root, 1..xml.len() + 1).is_err());
}

#[test]
fn application_payloads_link_real_entry_extents_without_an_extra_source_copy() {
    use cadmpeg_ir::{Codec, DecodeOptions};
    let document = "<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object name='Owner' type='App::Feature'/></Objects><ObjectData Count='1'><Object name='Owner'><Properties Count='1'><Property name='Included' type='App::PropertyFileIncluded'><FileIncluded file='shared.bin'/></Property></Properties></Object></ObjectData></Document>";
    let payload = b"source bytes shared by application references";
    let archive = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("shared.bin", payload),
    ]);
    let decoded = crate::FcstdCodec
        .decode(
            &mut std::io::Cursor::new(&archive),
            &DecodeOptions::default(),
        )
        .unwrap();
    let fidelity = decoded.source_fidelity();
    let id = crate::native::native_id("entry", "shared.bin");
    // #212 requires the original archive once, rather than an expanded entry copy.
    assert!(fidelity
        .retained_record(&id)
        .is_none_or(|record| record.data().is_none()));
    let source = crate::test_support::source_fidelity::entry_bytes(fidelity, &id);
    assert_eq!(source, payload);
    assert_eq!(
        cadmpeg_ir::hash::sha256_hex(&source),
        cadmpeg_ir::hash::sha256_hex(payload)
    );
    assert_eq!(
        fidelity
            .retained_record(super::SOURCE_IMAGE)
            .unwrap()
            .data(),
        Some(archive.as_slice())
    );
    assert_eq!(
        fidelity
            .retained_records()
            .values()
            .filter(|record| record.data() == Some(archive.as_slice()))
            .count(),
        1
    );
    let applications = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<serde_json::Value>("applications")
        .unwrap();
    let link = &applications[0]["property_records"][0]["payloads"][0];
    assert_eq!(link["entry"], id);
    assert_eq!(link["name"], "shared.bin");
    for field in ["sha256", "byte_len", "data"] {
        assert!(link.get(field).is_none(), "{field}");
    }
}
