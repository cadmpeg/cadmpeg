// SPDX-License-Identifier: Apache-2.0
//! Display-list descriptor probing: where a face header places its table,
//! what ends a table sequence, and what a recognised table refuses.

use super::*;

#[test]
fn scene_objects_carry_history_source_identity() {
    let mut payload = Vec::new();
    class(&mut payload, "moAmbientLight_c", &[12]);
    class(&mut payload, "moDirectionLight_c", &[30, 32]);
    class(&mut payload, "moVisualProperties_c", &[99]);
    class(&mut payload, "moPointLight_c", &[21]);
    class(&mut payload, "moSpotLight_c", &[20]);

    assert_eq!(
        scene_classes(&payload),
        vec![
            (12, "moAmbientLight_c".into()),
            (30, "moDirectionLight_c".into()),
            (32, "moDirectionLight_c".into()),
            (21, "moPointLight_c".into()),
            (20, "moSpotLight_c".into()),
        ]
    );
}

#[test]
fn anonymous_scene_object_counts_do_not_create_source_bindings() {
    let mut payload = Vec::new();
    payload.extend_from_slice(CLASS_MARKER);
    let class = b"moDirectionLight_c";
    payload.extend_from_slice(&(class.len() as u16).to_le_bytes());
    payload.extend_from_slice(class);
    for name in ["UnNamed", "Another"] {
        payload.extend_from_slice(&1_u32.to_le_bytes());
        payload.extend_from_slice(&[0xff, 0xfe, 0xff, 7]);
        for byte in name.bytes() {
            payload.extend_from_slice(&[byte, 0]);
        }
        payload.extend_from_slice(&[0xff, 0xfe, 0xff]);
    }

    assert!(scene_classes(&payload).is_empty());
}

#[test]
fn compact_face_tessellation_header_places_table_at_plus_8() {
    let mut payload = Vec::new();
    payload.extend(1_u32.to_le_bytes());
    payload.extend(1_u32.to_le_bytes());
    payload.extend(table());
    assert_eq!(descriptor_table_offset(&payload, 0), 8);
    assert!(parse_table_sequence(&payload, 8, payload.len())
        .expect("a display-list table reads")
        .is_some());
}

#[test]
fn extended_face_tessellation_header_places_table_at_plus_40() {
    let mut payload = Vec::new();
    for word in [1_u32, 1, 1, 0, 0, 0x0020_1296, 0, 0, 0, 0] {
        payload.extend(word.to_le_bytes());
    }
    payload.extend(table());
    assert_eq!(descriptor_table_offset(&payload, 0), 40);
    assert!(parse_table_sequence(&payload, 40, payload.len())
        .expect("a display-list table reads")
        .is_some());
}

#[test]
fn body_property_class_does_not_end_face_table_sequence() {
    let mut payload = Vec::new();
    class(&mut payload, "uoTempFaceTessData_c", &[]);
    payload.extend(1_u32.to_le_bytes());
    payload.extend(1_u32.to_le_bytes());
    payload.extend(table());

    class(&mut payload, "uoBodyPropInfo_c", &[]);
    payload.extend([0x37, 0x80]);
    payload.extend(1_u32.to_le_bytes());
    payload.extend(1_u32.to_le_bytes());
    payload.extend(table());

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x41, "Contents/DisplayLists", &payload));
    let result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.tessellations.len(), 2);
}

#[test]
fn next_face_class_ends_face_table_sequence() {
    let mut payload = Vec::new();
    for _ in 0..2 {
        class(&mut payload, "uoTempFaceTessData_c", &[]);
        payload.extend(1_u32.to_le_bytes());
        payload.extend(1_u32.to_le_bytes());
        payload.extend(table());
    }

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x41, "Contents/DisplayLists", &payload));
    let result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.tessellations.len(), 2);
}

#[test]
fn incomplete_extended_header_does_not_shift_the_table() {
    let mut payload = Vec::new();
    for word in [1_u32, 1, 1, 0, 0, 0, 0, 0, 0, 0] {
        payload.extend(word.to_le_bytes());
    }
    payload.extend(table());
    assert_eq!(descriptor_table_offset(&payload, 0), 8);
}

#[test]
fn inconsistent_auxiliary_count_invalidates_the_table() {
    let mut payload = table();
    let list_b_count = 20 + 52 + 52 + 12;
    payload[list_b_count..list_b_count + 4].copy_from_slice(&3_u32.to_le_bytes());
    assert!(parse_table(&payload, 0)
        .expect("a probe miss is not a refusal")
        .is_none());
}

#[test]
fn a_strip_span_past_the_vertex_lane_refuses_the_recognised_table() {
    // The descriptor is the record boundary: a table whose auxiliary channels
    // agree with a four-vertex strip, over a three-vertex lane, is a table the
    // codec recognizes and a lane error inside it, not a probe miss.
    let mut payload = descriptor(4, 8, 1, &4_u32.to_le_bytes());
    let positions = [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    payload.extend(descriptor(12, 100, 3, &positions));
    payload.extend(descriptor(12, 100, 3, &[0; 36]));
    payload.extend(descriptor(4, 8, 6, &[0; 24]));
    payload.extend(descriptor(4, 8, 1, &6_u32.to_le_bytes()));
    payload.extend(descriptor(1, 8, 6, &[0; 6]));
    let error = parse_table(&payload, 0).expect_err("a recognised table refuses its lane error");
    let text = error.to_string();
    assert!(
        text.contains("sldprt display-list table at byte 0")
            && text.contains("strip span(s) do not cut the vertex lane"),
        "{text}"
    );
}

#[test]
fn a_zero_face_header_carries_the_table_it_does_not_state() {
    // Two zero counts state no mesh, not an empty one. The primary writes that
    // header over a face whose descriptor table still holds a mesh, and
    // `body_display_list.sldprt` is one such document.
    let mut payload = Vec::new();
    class(&mut payload, "uoTempFaceTessData_c", &[]);
    payload.extend(0_u32.to_le_bytes());
    payload.extend(0_u32.to_le_bytes());
    payload.extend(table());

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x41, "Contents/DisplayLists", &payload));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("a zero display-face header states no mesh and refuses nothing");
    assert_eq!(decoded.ir().model.faces.len(), 1);
}

#[test]
fn a_declared_face_count_disagreement_refuses_the_display_face() {
    // The face header states one strip and one triangle for the table that
    // follows. A header stating two triangles over the same table contradicts
    // bytes that are present, so the table is refused, not skipped.
    let mut payload = Vec::new();
    class(&mut payload, "uoTempFaceTessData_c", &[]);
    payload.extend(2_u32.to_le_bytes());
    payload.extend(1_u32.to_le_bytes());
    payload.extend(table());

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x41, "Contents/DisplayLists", &payload));
    let error = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect_err("a display-face count disagreement refuses the decode");
    let text = error.to_string();
    assert!(
        text.contains("sldprt display-face table at byte")
            && text.contains("header states 2 triangle(s) and 1 strip(s)")
            && text.contains("parsed mesh has 1 triangle(s) and 1 strip(s)"),
        "{text}"
    );
}
