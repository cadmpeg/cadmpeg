// SPDX-License-Identifier: Apache-2.0
//! Display-list descriptor probing: where a face header places its table,
//! what ends a table sequence, and what a recognised table refuses.

use super::class;
use super::table;
use crate::container::{Block, BlockName, PayloadFamily, Section};
use crate::tessellation::descriptor_table_offset;
use crate::tessellation::parse_table;
use crate::tessellation::parse_table_sequence;
use crate::tessellation::scene_feature_classes;
use crate::tessellation::section_display_faces;
use crate::tessellation::CLASS_MARKER;
use crate::test_support::container::make_block;
use crate::test_support::container::sldprt_with_body;
use crate::test_support::parasolid::triangle_body;
use crate::test_support::tessellation::descriptor;
use crate::SldprtCodec;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::codec::DecodeOptions;
use cadmpeg_ir::Codec;
use std::io::Cursor;

#[test]
fn scene_objects_carry_history_source_identity() {
    let mut payload = Vec::new();
    class(&mut payload, "moAmbientLight_c", &[12]);
    class(&mut payload, "moDirectionLight_c", &[30, 32]);
    class(&mut payload, "moVisualProperties_c", &[99]);
    class(&mut payload, "moPointLight_c", &[21]);
    class(&mut payload, "moSpotLight_c", &[20]);

    assert_eq!(
        display_scene_classes(&payload),
        [
            (12, "moAmbientLight_c"),
            (30, "moDirectionLight_c"),
            (32, "moDirectionLight_c"),
            (21, "moPointLight_c"),
            (20, "moSpotLight_c"),
        ]
        .into_iter()
        .map(|(source, class)| (source, class.to_owned()))
        .collect()
    );
}

/// The scene classes of a document whose display-list section is `payload`.
fn display_scene_classes(payload: &[u8]) -> std::collections::HashMap<u32, String> {
    let mut source = crate::test_support::container::outer_header();
    source.extend(make_block(0x41, "Contents/DisplayLists", payload));
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service()).unwrap();
    let scan = crate::container::scan(&ctx, root).unwrap();
    scene_feature_classes(&ctx, &scan).unwrap()
}

#[test]
fn anonymous_scene_object_counts_do_not_create_source_bindings() {
    let mut payload = Vec::new();
    payload.extend_from_slice(CLASS_MARKER);
    let class = b"moDirectionLight_c";
    payload
        .extend_from_slice(&(u16::try_from(class.len()).expect("length fits u16")).to_le_bytes());
    payload.extend_from_slice(class);
    for name in ["UnNamed", "Another"] {
        payload.extend_from_slice(&1_u32.to_le_bytes());
        payload.extend_from_slice(&[0xff, 0xfe, 0xff, 7]);
        for byte in name.bytes() {
            payload.extend_from_slice(&[byte, 0]);
        }
        payload.extend_from_slice(&[0xff, 0xfe, 0xff]);
    }

    assert!(display_scene_classes(&payload).is_empty());
}

#[test]
fn compact_face_tessellation_header_places_table_at_plus_8() {
    let mut payload = Vec::new();
    payload.extend(1_u32.to_le_bytes());
    payload.extend(1_u32.to_le_bytes());
    payload.extend(table());
    assert_eq!(descriptor_table_offset(&payload, 0), 8);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&payload, &arena, &DecodePolicy::service()).unwrap();
    assert!(parse_table_sequence(&ctx, &payload, 8, payload.len())
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
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&payload, &arena, &DecodePolicy::service()).unwrap();
    assert!(parse_table_sequence(&ctx, &payload, 40, payload.len())
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
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&payload, &arena, &DecodePolicy::service()).unwrap();
    assert!(parse_table(&ctx, &payload, 0)
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
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&payload, &arena, &DecodePolicy::service()).unwrap();
    let error =
        parse_table(&ctx, &payload, 0).expect_err("a recognised table refuses its lane error");
    let text = error.to_string();
    assert!(
        text.contains("sldprt display-list table at byte 0")
            && text.contains("strip span(s) do not cut the vertex lane"),
        "{text}"
    );
}

/// Refuse `operation` one collection item below its need, with every earlier
/// charge admitted.
fn collection_refusal<T>(
    operation: &'static str,
    payload: &[u8],
    decode: impl Fn(&DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        operation,
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy).expect("root");
            decode(&ctx)
        },
    );
    assert!(matches!(error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == operation));
}

#[test]
fn display_table_lanes_refuse_collection_limit_before_allocation() {
    let payload = table();
    for operation in [
        "decode display-list strips",
        "decode display-list vertices",
        "decode display-list normals",
        "decode display-list channels",
        "pair display-list shaded vertices",
        "partition display-list strip vertices",
        "partition display-list strips",
    ] {
        collection_refusal(operation, &payload, |ctx| parse_table(ctx, &payload, 0));
    }
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&payload, &arena, &DecodePolicy::service()).expect("root");
    assert!(parse_table(&ctx, &payload, 0)
        .expect("service profile admits the table")
        .is_some());
}

#[test]
fn display_table_channel_bytes_refuse_retained_limit_before_copy() {
    let payload = table();
    let arena = DecodeArena::new();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy display-list channel bytes",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).expect("root");
            parse_table(&ctx, &payload, 0).map(|_| ())
        },
    );
    assert!(matches!(error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "copy display-list channel bytes"));
}

#[test]
fn display_table_sequence_refuses_collection_limit_before_adding_first_table() {
    let payload = table();
    collection_refusal("collect display-list tables", &payload, |ctx| {
        parse_table_sequence(ctx, &payload, 0, payload.len())
    });
}

#[test]
fn display_table_sequence_refuses_collection_limit_before_adding_next_table() {
    let mut payload = table();
    payload.extend(table());
    // The second table's slot is the sequence's last collection charge, so
    // one item below the smallest admitting cap refuses there.
    let run = |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).expect("root");
        parse_table_sequence(&ctx, &payload, 0, payload.len()).map(|tables| tables.map(|t| t.len()))
    };
    let need = (0..1024)
        .find(|cap| run(*cap).is_ok())
        .expect("a cap admits both tables");
    assert_eq!(run(need).expect("admitted"), Some(2));
    assert!(matches!(run(need - 1),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "collect display-list tables"));

    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&payload, &arena, &DecodePolicy::service()).expect("root");
    assert_eq!(
        parse_table_sequence(&ctx, &payload, 0, payload.len())
            .expect("two tables")
            .expect("table sequence")
            .len(),
        2
    );
}

#[test]
fn display_faces_refuse_collection_limit_before_insertion() {
    let mut payload = Vec::new();
    class(&mut payload, "uoTempFaceTessData_c", &[]);
    payload.extend(1_u32.to_le_bytes());
    payload.extend(1_u32.to_le_bytes());
    payload.extend(table());
    let block = Block {
        offset: 0,
        type_id: 0x41,
        comp_sz: 0,
        section: BlockName::Named(cadmpeg_ir::stream_name!("Contents/DisplayLists")),
        name_words: crate::container::NameWords::default(),
        family: PayloadFamily::Tessellation,
        payload,
        ps_streams: Vec::new(),
    };
    collection_refusal("collect display-list faces", &block.payload, |ctx| {
        section_display_faces(ctx, Section::Block(&block))
    });

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&block.payload, &arena, &DecodePolicy::service())
        .expect("root");
    assert_eq!(
        section_display_faces(&ctx, Section::Block(&block))
            .expect("face admitted")
            .len(),
        1
    );
}

#[test]
fn a_zero_face_header_states_no_display_face_and_refuses_nothing() {
    // Two zero counts state no mesh, not an empty one. The primary writes that
    // header over a face whose descriptor table still holds a mesh --
    // `body_display_list.sldprt` is one such document -- and the marker then
    // states no display face rather than contradicting the bytes below it.
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
    assert!(decoded.ir().model.tessellations.is_empty());
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

/// A display-list table whose normal lane does not cover its vertex lane
/// states a shaded mesh it cannot fill. It is refused by name, not dropped.
#[test]
fn a_short_normal_lane_refuses_the_display_table() {
    let mut payload = descriptor(4, 8, 1, &3_u32.to_le_bytes());
    let positions = [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    payload.extend(descriptor(12, 100, 3, &positions));
    // Two normals against three vertices.
    payload.extend(descriptor(12, 100, 2, &[0; 24]));
    payload.extend(descriptor(4, 8, 4, &[0; 16]));
    payload.extend(descriptor(4, 8, 1, &4_u32.to_le_bytes()));
    payload.extend(descriptor(1, 8, 4, &[0; 4]));

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &payload,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("root");
    let error = parse_table(&ctx, &payload, 0)
        .expect_err("a short normal lane is refused")
        .to_string();
    assert!(error.contains("vertex normal(s)"), "{error}");
}
