// SPDX-License-Identifier: Apache-2.0

#[test]
fn native_four_reference_payload_is_separate_from_five_reference_geometry() {
    let bytes = [
        0xb2, 3, 0x5e, 5, 5, 9, 15, 7, 11, 0x21, 0xb2, 3, 0x5e, 6, 5, 3, 9, 15, 7, 11, 0x21,
    ];
    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.consolidated_edge_nodes.len(), 1);
    let wire = serde_json::to_value(&native).expect("native wire");
    let rows = wire["consolidated_four_reference_edges"]
        .as_array()
        .expect("four-reference arena");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["frame"]["payload"], "090f070b21");
    assert_eq!(rows[0]["operands"].as_array().expect("operands").len(), 4);
    assert_eq!(rows[0]["operands"][0]["value"], 2);
    assert_eq!(rows[0]["operands"][1]["value"], 3);
    assert_eq!(rows[0]["terminal"], 0x21);
    assert!(rows[0].get("curve_ref").is_none());
    assert!(rows[0].get("vertex_refs").is_none());
    assert!(rows[0].get("parameter_selectors").is_none());
}

#[test]
fn native_four_reference_alternate_flag_roundtrips_with_source_identity() {
    let bytes = [0xff, 0xff, 0xff, 0xb2, 0x83, 0x5e, 5, 5, 9, 15, 7, 11, 0x21];
    let native = crate::native::CatiaNative::decode_with_record_ranges(
        &bytes,
        std::slice::from_ref(&(3..bytes.len())),
    );
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).expect("native store");
    let value = serde_json::to_value(&namespace).expect("namespace wire");
    let rows = value["consolidated_four_reference_edges"]
        .as_array()
        .expect("four-reference arena");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["frame"]["flag"], 0x83);
    assert_eq!(rows[0]["frame"]["byte_offset"], 3);
    assert_eq!(rows[0]["source_index"], 0);
    assert_eq!(rows[0]["source_offset"], 0);
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("checked namespace"),
        native
    );
}
