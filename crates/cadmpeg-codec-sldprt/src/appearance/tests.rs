// SPDX-License-Identifier: Apache-2.0
//! Material and face-color decode tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::container;
use crate::layout::visual_states_feature_appearance_prefix as feature_visual;
use crate::test_support::*;
use crate::SldprtCodec;

fn display_descriptor(item_size: u32, kind: u32, count: u32, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(item_size.to_le_bytes());
    out.extend(kind.to_le_bytes());
    out.extend(2_u32.to_le_bytes());
    out.extend(count.to_le_bytes());
    out.extend(data);
    out
}

fn display_table(x: f32) -> Vec<u8> {
    let mut out = display_descriptor(4, 8, 1, &3_u32.to_le_bytes());
    let positions = [x, 0.0_f32, 0.0, x + 1.0, 0.0, x, 0.0, 1.0, x]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    out.extend(display_descriptor(12, 100, 3, &positions));
    out.extend(display_descriptor(12, 100, 3, &[0; 36]));
    out.extend(display_descriptor(4, 8, 4, &[0; 16]));
    out.extend(display_descriptor(4, 8, 1, &4_u32.to_le_bytes()));
    out.extend(display_descriptor(1, 8, 4, &[0; 4]));
    out
}

fn display_class(out: &mut Vec<u8>, name: &str) {
    out.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    out.extend_from_slice(name.as_bytes());
}

fn inline_display_appearance(name: &str, rgb: [u8; 3]) -> Vec<u8> {
    let payload = material_payload(name, rgb);
    let mut out = vec![0x33, 0x80];
    out.extend_from_slice(&payload[b"moVisualProperties_c".len()..]);
    out
}

fn feature_visual_record(source_id: u32, timestamp: u32, rgb: [u8; 3]) -> Vec<u8> {
    let mut record = vec![0; feature_visual::LEN];
    for (offset, value) in [
        (feature_visual::VERSION, feature_visual::VERSION_VALUE),
        (feature_visual::FEATURE_SOURCE_ID, source_id),
        (feature_visual::FEATURE_TIMESTAMP, timestamp),
        (
            feature_visual::SELECTOR_ONE_A,
            feature_visual::SELECTOR_ONE_A_VALUE,
        ),
        (
            feature_visual::SELECTOR_ONE_B,
            feature_visual::SELECTOR_ONE_B_VALUE,
        ),
        (
            feature_visual::SELECTOR_TWO,
            feature_visual::SELECTOR_TWO_VALUE,
        ),
    ] {
        record[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    record[feature_visual::INSTANCE_PREFIX
        ..feature_visual::INSTANCE_PREFIX + feature_visual::INSTANCE_PREFIX_VALUE.len()]
        .copy_from_slice(&feature_visual::INSTANCE_PREFIX_VALUE);
    record[feature_visual::MARKER..feature_visual::MARKER + feature_visual::MARKER_VALUE.len()]
        .copy_from_slice(&feature_visual::MARKER_VALUE);
    let packed = u32::from(rgb[0]) | (u32::from(rgb[1]) << 8) | (u32::from(rgb[2]) << 16);
    record[feature_visual::PACKED_COLOR..feature_visual::PACKED_COLOR + 4]
        .copy_from_slice(&packed.to_le_bytes());
    record
}

fn framed_surface_reference(class: &str, source_id: u32, local_id: u32) -> Vec<u8> {
    let units = format!("{class},{source_id},{local_id},opaque")
        .encode_utf16()
        .collect::<Vec<_>>();
    let mut out = vec![0xff, 0xfe, 0xff, units.len().try_into().unwrap()];
    out.extend(units.into_iter().flat_map(u16::to_le_bytes));
    out
}

fn display_fixture(
    body: [u8; 3],
    faces: &[Vec<(&str, u32, u32)>],
    face_overrides: &[(usize, [u8; 3])],
    features: &[(u32, u32, [u8; 3])],
) -> Vec<u8> {
    let mut display = Vec::new();
    display_class(&mut display, "uoTempFaceTessData_c");
    display.extend(1_u32.to_le_bytes());
    display.extend(1_u32.to_le_bytes());
    for (index, references) in faces.iter().enumerate() {
        display.extend(display_table(index as f32 * 10.0));
        for (class, source, local) in references {
            display.extend(framed_surface_reference(class, *source, *local));
        }
        if let Some((_, rgb)) = face_overrides.iter().find(|(face, _)| *face == index) {
            display.extend(inline_display_appearance("Face override", *rgb));
        }
    }
    display_class(&mut display, "uoBodyPropInfo_c");
    display.extend(inline_display_appearance("Body default", body));

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x41, "Contents/DisplayLists", &display));
    if !features.is_empty() {
        let mut visual_states = Vec::new();
        display_class(&mut visual_states, "moCompFeature_c");
        for (source_id, timestamp, rgb) in features {
            visual_states.extend(feature_visual_record(*source_id, *timestamp, *rgb));
        }
        source.extend(make_block(
            0x42,
            "ThirdPtyStore/VisualStates",
            &visual_states,
        ));
    }
    source
}

fn display_colors(bytes: Vec<u8>) -> Vec<[u8; 3]> {
    use cadmpeg_ir::appearance::AppearanceTarget;

    let result = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let colors = result
        .ir()
        .model
        .appearances
        .iter()
        .filter_map(|appearance| Some((appearance.id.clone(), appearance.base_color?)))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut colors = result
        .ir()
        .model
        .tessellations
        .iter()
        .map(|tessellation| {
            let binding = result
                .ir()
                .model
                .appearance_bindings
                .iter()
                .find(|binding| {
                    binding.target == AppearanceTarget::Tessellation(tessellation.id.clone())
                })
                .unwrap();
            let color = colors[&binding.appearance];
            let table_index = tessellation
                .id
                .rsplit(':')
                .next()
                .unwrap()
                .parse::<usize>()
                .unwrap();
            (
                table_index,
                [color.r, color.g, color.b].map(|value| (value * 255.0).round() as u8),
            )
        })
        .collect::<Vec<_>>();
    colors.sort_by_key(|(table_index, _)| *table_index);
    colors.into_iter().map(|(_, color)| color).collect()
}

#[test]
fn packed_rgb_uses_low_to_high_red_green_blue_bytes() {
    for (packed, expected) in [
        (0x0000_00ff, [1.0, 0.0, 0.0]),
        (0x0000_ff00, [0.0, 1.0, 0.0]),
        (0x00ff_0000, [0.0, 0.0, 1.0]),
    ] {
        let color = super::packed_rgb(packed);
        assert_eq!([color.r, color.g, color.b], expected);
    }
}

#[test]
fn visual_states_feature_assignment_decodes_identity_and_color() {
    let mut visual_states = Vec::new();
    display_class(&mut visual_states, "moCompFeature_c");
    visual_states.extend(feature_visual_record(36, 0x6a81_f0f4, [236, 255, 0]));
    let mut source = outer_header();
    source.extend(make_block(
        0x42,
        "ThirdPtyStore/VisualStates",
        &visual_states,
    ));

    let assignments = super::feature_assignments(&container::scan_bytes(&source));
    assert_eq!(assignments.len(), 1);
    assert_eq!(assignments[0].feature_source_id, 36);
    assert_eq!(assignments[0].feature_timestamp, 0x6a81_f0f4);
    assert_eq!(assignments[0].packed_color, 0x0000_ffec);
}

#[test]
fn display_body_and_face_assignments_use_structural_precedence() {
    let faces = vec![Vec::new(), Vec::new(), Vec::new()];
    assert_eq!(
        display_colors(display_fixture([46, 255, 7], &faces, &[], &[])),
        [[46, 255, 7]; 3]
    );
    assert_eq!(
        display_colors(display_fixture(
            [202, 209, 238],
            &faces,
            &[(0, [247, 0, 23]), (1, [7, 15, 255])],
            &[],
        )),
        [[247, 0, 23], [7, 15, 255], [202, 209, 238]]
    );
}

#[test]
fn persistent_surface_sources_bind_feature_appearances() {
    let classes = [
        "moFromSktEntSurfIdRep_c",
        "moFromSktEnt3IntSurfIdRep_c",
        "moEndFace3IntSurfIdRep_c",
    ];
    let faces = (0..12)
        .map(|index| {
            let source = match index {
                0..=5 => 36,
                6..=9 => 44,
                _ => 51,
            };
            vec![(classes[index % classes.len()], source, index as u32 + 1)]
        })
        .collect::<Vec<_>>();
    let features = [
        (36, 10, [236, 255, 0]),
        (44, 20, [239, 0, 0]),
        (51, 30, [7, 255, 43]),
    ];
    let mut expected = vec![[236, 255, 0]; 6];
    expected.extend([[239, 0, 0]; 4]);
    expected.extend([[7, 255, 43]; 2]);
    assert_eq!(
        display_colors(display_fixture([128; 3], &faces, &[], &features)),
        expected
    );
}

#[test]
fn face_local_wins_and_conflicting_or_missing_feature_sources_do_not_guess() {
    let faces = vec![
        vec![
            ("moFromSktEntSurfIdRep_c", 36, 1),
            ("moFromSktEntSurfIdRep_c", 36, 1),
        ],
        vec![
            ("moFromSktEntSurfIdRep_c", 36, 2),
            ("moFromSktEntSurfIdRep_c", 44, 2),
        ],
        vec![("moEndFace3IntSurfIdRep_c", 99, 1)],
    ];
    assert_eq!(
        display_colors(display_fixture(
            [128; 3],
            &faces,
            &[(0, [0, 0, 255])],
            &[(36, 10, [255, 0, 0]), (44, 20, [0, 255, 0])],
        )),
        [[0, 0, 255], [128; 3], [128; 3]]
    );
}

#[test]
fn decode_retains_visual_property_without_fabricating_body_ownership() {
    let f = sldprt_with_body_and_material(&triangle_body(), "Steel", [32, 64, 128]);
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert!(result.ir().model.bodies[0].color.is_none());
    let color = result.ir().model.appearances[0].base_color.unwrap();
    assert!((color.r - 32.0 / 255.0).abs() < 1e-6);
    assert!((color.g - 64.0 / 255.0).abs() < 1e-6);
    assert!((color.b - 128.0 / 255.0).abs() < 1e-6);
    assert_eq!(result.ir().model.appearances.len(), 1);
    assert!(result.ir().model.appearance_bindings.is_empty());
    assert_eq!(
        result.ir().model.appearances[0].name.as_deref(),
        Some("Steel")
    );
}

#[test]
fn decode_preserves_ambiguous_materials_without_fabricating_ownership() {
    let mut source = sldprt_with_body(&triangle_body());
    let mut materials = material_payload("Steel", [32, 64, 128]);
    materials.extend(material_payload("Aluminum", [160, 170, 180]));
    source.extend(make_block(0x40, "SWObjects", &materials));

    let mut result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.appearances.len(), 2);
    assert!(result.ir().model.appearance_bindings.is_empty());
    assert!(result
        .ir()
        .model
        .bodies
        .iter()
        .all(|body| body.color.is_none() && body.name.is_none()));

    result.ir_mut().model.points[0].position.z += 1.0;
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(result.ir(), result.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(regenerated.ir().model.appearances.len(), 2);
    assert_eq!(
        regenerated
            .ir()
            .model
            .appearances
            .iter()
            .filter_map(|appearance| appearance.name.as_deref())
            .collect::<Vec<_>>(),
        vec!["Steel", "Aluminum"]
    );
    assert!(regenerated.ir().model.appearance_bindings.is_empty());
}

#[test]
fn decode_binds_entity53_color_to_face() {
    use cadmpeg_ir::appearance::AppearanceTarget;
    let mut body = Vec::new();
    body.extend(face_color_definition());
    body.extend(entity51(2, 500, 0x0017, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(
        1,
        700,
        FACE_COLOR_DEFINITION_ID,
        &[0, 0, 0, 0, 0, 900],
    ));
    body.extend(entity53_color(900, [0.25, 0.5, 0.75]));
    body.extend(owned_triangle(0, 700, 0.0));
    let mut cur = Cursor::new(sldprt_with_body(&body));
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        result.report().losses.len(),
        1,
        "{:#?}",
        result.report().losses
    );
    assert_eq!(
        result.report().losses[0].message,
        "1 configuration state(s) are inferred from geometry partitions without native configuration definitions."
    );
    let binding = result
        .ir()
        .model
        .appearance_bindings
        .iter()
        .find(|binding| matches!(binding.target, AppearanceTarget::Face(_)))
        .expect("face binding");
    let appearance = result
        .ir()
        .model
        .appearances
        .iter()
        .find(|appearance| appearance.id == binding.appearance)
        .unwrap();
    let color = appearance.base_color.unwrap();
    assert_eq!([color.r, color.g, color.b], [0.25, 0.5, 0.75]);
}

#[test]
fn decode_does_not_bind_color_to_an_unemitted_face() {
    use cadmpeg_ir::appearance::AppearanceTarget;

    let mut body = Vec::new();
    body.extend(face_color_definition());
    body.extend(entity51(
        1,
        700,
        FACE_COLOR_DEFINITION_ID,
        &[0, 0, 0, 0, 0, 900],
    ));
    body.extend(entity53_color(900, [0.25, 0.5, 0.75]));
    body.extend(entity51(
        1,
        701,
        FACE_COLOR_DEFINITION_ID,
        &[0, 0, 0, 0, 0, 901],
    ));
    body.extend(entity53_color(901, [0.75, 0.5, 0.25]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(plane_carrier(
        200,
        [2.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    body.extend(bridge_owned(110, 120, 200, 701));

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.appearances.len(), 2);
    assert_eq!(
        result
            .ir()
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| matches!(binding.target, AppearanceTarget::Face(_)))
            .count(),
        1
    );
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
fn decode_binds_adjacent_entity53_color_to_disc14_face() {
    use cadmpeg_ir::appearance::AppearanceTarget;
    let mut body = Vec::new();
    body.extend(face_color_definition());
    body.extend(entity51(2, 500, 0x0017, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(
        1,
        700,
        FACE_COLOR_DEFINITION_ID,
        &[0, 0, 0, 0, 0, 0],
    ));
    body.extend(entity53_color(901, [1.0, 0.125, 0.0]));
    body.extend(owned_triangle(0, 700, 0.0));
    let mut cur = Cursor::new(sldprt_with_body(&body));
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    let binding = result
        .ir()
        .model
        .appearance_bindings
        .iter()
        .find(|binding| matches!(binding.target, AppearanceTarget::Face(_)))
        .expect("face binding");
    let color = result
        .ir()
        .model
        .appearances
        .iter()
        .find(|appearance| appearance.id == binding.appearance)
        .unwrap()
        .base_color
        .unwrap();
    assert_eq!([color.r, color.g, color.b], [1.0, 0.125, 0.0]);
}
