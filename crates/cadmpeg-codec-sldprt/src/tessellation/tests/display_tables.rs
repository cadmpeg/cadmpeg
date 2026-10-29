// SPDX-License-Identifier: Apache-2.0
//! Display-list descriptor probing: where a face header places its table,
//! what ends a table sequence, and what a recognised table refuses.

use super::class;
use super::framed_surface_reference;
use super::persistent_identity;
use super::table;
use crate::container::{Block, BlockName, PayloadFamily, Section};
use crate::tessellation::admit_reused_face_tag;
use crate::tessellation::class_intervals;
use crate::tessellation::descriptor_table_offset;
use crate::tessellation::parse_table;
use crate::tessellation::parse_table_sequence;
use crate::tessellation::persistent_join_verdict;
use crate::tessellation::scene_classes;
use crate::tessellation::section_display_faces;
use crate::tessellation::PersistentJoinVerdict;
use crate::tessellation::PersistentJoinWithhold;
use crate::tessellation::ReusedTagVerdict;
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

    assert!(scene_classes(&payload).is_empty());
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

#[test]
fn display_table_reads_as_a_mesh() {
    let payload = table();
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&payload, &arena, &DecodePolicy::service()).expect("root");
    assert!(parse_table(&ctx, &payload, 0)
        .expect("parse table")
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
fn display_table_sequence_collects_consecutive_tables() {
    let mut payload = table();
    payload.extend(table());
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
fn one_face_header_over_one_table_yields_one_display_face() {
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
        family: PayloadFamily::Tessellation,
        payload,
        ps_streams: Vec::new(),
        ps_probe_capped: false,
    };
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

/// One reused-tag probe: the u16 tag and the two u32 counts it frames.
fn reused_tag_block(tag: u16, triangles: u32, strips: u32) -> Vec<u8> {
    let mut block = Vec::new();
    block.extend(tag.to_le_bytes());
    block.extend(triangles.to_le_bytes());
    block.extend(strips.to_le_bytes());
    block
}

/// Two face tables with `gap` bytes between the first table and the second
/// declaration and `second_metadata` bytes after the second table.
fn two_face_payload(gap: &[u8], second_metadata: &[u8]) -> Vec<u8> {
    let mut payload = Vec::new();
    class(&mut payload, "uoTempFaceTessData_c", &[]);
    payload.extend(1_u32.to_le_bytes());
    payload.extend(1_u32.to_le_bytes());
    payload.extend(table());
    payload.extend_from_slice(gap);
    class(&mut payload, "uoTempFaceTessData_c", &[]);
    payload.extend(1_u32.to_le_bytes());
    payload.extend(1_u32.to_le_bytes());
    payload.extend(table());
    payload.extend_from_slice(second_metadata);
    payload
}

fn display_faces_of(payload: &[u8]) -> Vec<crate::tessellation::DisplayFace> {
    let block = Block {
        offset: 0,
        type_id: 0x41,
        comp_sz: 0,
        section: BlockName::Named(cadmpeg_ir::stream_name!("Contents/DisplayLists")),
        family: PayloadFamily::Tessellation,
        payload: payload.to_vec(),
        ps_streams: Vec::new(),
        ps_probe_capped: false,
    };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&block.payload, &arena, &DecodePolicy::service())
        .expect("root");
    section_display_faces(&ctx, Section::Block(&block)).expect("faces admit")
}

#[test]
fn no_tag_bytes_leave_no_reused_tag() {
    let payload = two_face_payload(&[0; 24], &[]);
    let faces = display_faces_of(&payload);
    assert_eq!(faces.len(), 2);
    let classes = class_intervals(&payload);
    assert_eq!(
        admit_reused_face_tag(&payload, &faces[1], faces[0].table.end(), &classes),
        ReusedTagVerdict::Absent
    );
    assert_eq!(
        persistent_join_verdict(&payload, &faces, 1, &classes),
        PersistentJoinVerdict::Withhold(PersistentJoinWithhold::InvalidTokens)
    );
}

#[test]
fn single_reused_tag_before_a_known_table_admits() {
    let payload = two_face_payload(
        &reused_tag_block(0x8001, 1, 1),
        &framed_surface_reference("moPlaneSurfIdRep_c,7,3,"),
    );
    let faces = display_faces_of(&payload);
    assert_eq!(faces.len(), 2);
    let classes = class_intervals(&payload);
    assert_eq!(
        admit_reused_face_tag(&payload, &faces[1], faces[0].table.end(), &classes),
        ReusedTagVerdict::Admit(0x8001)
    );
    assert_eq!(
        persistent_join_verdict(&payload, &faces, 1, &classes),
        PersistentJoinVerdict::Admit(persistent_identity(7, 3, &[]))
    );
}

#[test]
fn conflicting_reused_tags_withhold_the_join() {
    let mut gap = reused_tag_block(0x8001, 1, 1);
    gap.extend(reused_tag_block(0x8002, 1, 1));
    let payload = two_face_payload(&gap, &framed_surface_reference("moPlaneSurfIdRep_c,7,3,"));
    let faces = display_faces_of(&payload);
    assert_eq!(faces.len(), 2);
    let classes = class_intervals(&payload);
    assert_eq!(
        admit_reused_face_tag(&payload, &faces[1], faces[0].table.end(), &classes),
        ReusedTagVerdict::Withhold
    );
    assert_eq!(
        persistent_join_verdict(&payload, &faces, 1, &classes),
        PersistentJoinVerdict::Withhold(PersistentJoinWithhold::AmbiguousReusedTag)
    );
}

#[test]
fn absent_tag_values_never_survive() {
    let mut gap = reused_tag_block(0xffff, 1, 1);
    gap.extend(reused_tag_block(0x8000, 1, 1));
    let payload = two_face_payload(&gap, &[]);
    let faces = display_faces_of(&payload);
    assert_eq!(faces.len(), 2);
    let classes = class_intervals(&payload);
    assert_eq!(
        admit_reused_face_tag(&payload, &faces[1], faces[0].table.end(), &classes),
        ReusedTagVerdict::Absent
    );
}

#[test]
fn tag_with_disagreeing_counts_never_survives() {
    let payload = two_face_payload(&reused_tag_block(0x8001, 2, 1), &[]);
    let faces = display_faces_of(&payload);
    assert_eq!(faces.len(), 2);
    let classes = class_intervals(&payload);
    assert_eq!(
        admit_reused_face_tag(&payload, &faces[1], faces[0].table.end(), &classes),
        ReusedTagVerdict::Absent
    );
}

#[test]
fn tag_outside_face_intervals_never_survives() {
    let payload = two_face_payload(&reused_tag_block(0x8001, 1, 1), &[]);
    let faces = display_faces_of(&payload);
    assert_eq!(faces.len(), 2);
    assert_eq!(
        admit_reused_face_tag(&payload, &faces[1], faces[0].table.end(), &[]),
        ReusedTagVerdict::Absent
    );
}

#[test]
fn join_verdict_without_a_face_withholds() {
    let payload = two_face_payload(&[], &[]);
    assert_eq!(
        persistent_join_verdict(&payload, &[], 0, &[]),
        PersistentJoinVerdict::Withhold(PersistentJoinWithhold::InvalidTokens)
    );
}
