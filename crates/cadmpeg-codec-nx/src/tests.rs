// SPDX-License-Identifier: Apache-2.0
//! Tests over synthetic byte fixtures. No real CAD file exists in this repo and
//! none may be added, so every fixture is a hand-built `.prt` byte image whose
//! bytes exercise the real SPLMSSTR container parse, the Parasolid zlib
//! extraction/classification, and the analytic geometry decode, and fail if the
//! code regresses.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, CodecEntry, Confidence, DecodeOptions};
use cadmpeg_ir::decode::{DecodeMode, InspectOptions};
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, CurveGeometry, PcurveGeometry, ProceduralCurveDefinition,
    ProceduralSurfaceDefinition, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Vector3};
use cadmpeg_ir::report::LossCategory;
use cadmpeg_ir::Exactness;

use crate::container;
use crate::parasolid::{self, StreamKind};
use crate::test_support::*;
use crate::NxCodec;

fn extract_streams(bytes: &[u8]) -> Vec<crate::parasolid::Stream> {
    let arena = cadmpeg_ir::decode::DecodeArena::new();
    let policy = cadmpeg_ir::decode::DecodePolicy::default();
    let (ctx, root) = cadmpeg_ir::decode::DecodeContext::from_root_bytes(bytes, &arena, &policy)
        .expect("bounded test input");
    let container = container::scan_bytes(bytes.to_vec()).expect("test SPLMSSTR container");
    parasolid::extract_streams(&ctx, root, &container).expect("test Parasolid streams")
}

fn options_in(mode: DecodeMode, container_only: bool) -> DecodeOptions {
    DecodeOptions {
        container_only,
        policy: cadmpeg_ir::decode::DecodePolicy {
            mode,
            ..Default::default()
        },
    }
}

#[test]
fn jt_int32_cdp2_decodes_empty_and_bitlength_packets() {
    assert_eq!(
        crate::jt::decode_int32_cdp2(&[0, 0, 0, 0], 0),
        Some((vec![], 4))
    );

    let encode_packet = |bits: &[u8], value_count: u32| {
        let mut code_words = Vec::new();
        for chunk in bits.chunks(32) {
            let mut word = 0u32;
            for bit in chunk {
                word = (word << 1) | u32::from(*bit);
            }
            word <<= 32 - chunk.len();
            code_words.extend_from_slice(&word.to_le_bytes());
        }
        let mut packet = value_count.to_le_bytes().to_vec();
        packet.push(1);
        packet.extend_from_slice(&(bits.len() as u32).to_le_bytes());
        packet.extend(code_words);
        packet
    };
    let field = |bits: &mut Vec<u8>, value: u32, width: u8| {
        bits.extend((0..width).rev().map(|shift| ((value >> shift) & 1) as u8));
    };

    // Fixed-width mode: range [-1, 1], followed by codes for 1 and -1.
    let mut bits = vec![0];
    field(&mut bits, 2, 6);
    field(&mut bits, 2, 6);
    field(&mut bits, 0b11, 2);
    field(&mut bits, 0b01, 2);
    field(&mut bits, 2, 2);
    field(&mut bits, 0, 2);
    let packet = encode_packet(&bits, 2);
    assert_eq!(
        crate::jt::decode_int32_cdp2(&packet, 0),
        Some((vec![1, -1], packet.len()))
    );

    // Variable-width mode: mean 10, one two-bit run containing +1 and -1.
    let mut bits = vec![1];
    field(&mut bits, 10, 32);
    field(&mut bits, 3, 3);
    field(&mut bits, 3, 3);
    field(&mut bits, 2, 3);
    field(&mut bits, 2, 3);
    field(&mut bits, 1, 2);
    field(&mut bits, 3, 2);
    let packet = encode_packet(&bits, 2);
    assert_eq!(
        crate::jt::decode_int32_cdp2(&packet, 0),
        Some((vec![11, 9], packet.len()))
    );
}

#[test]
fn jt_int32_cdp2_decodes_arithmetic_context_with_zero_frequency_entry() {
    let mut context_bits = Vec::<bool>::new();
    let mut push = |value: u32, width: u8| {
        for shift in (0..width).rev() {
            context_bits.push((value >> shift) & 1 != 0);
        }
    };
    push(2, 6);
    push(1, 6);
    push(1, 6);
    push(7, 32);
    push(0, 2);
    push(0, 1);
    push(0, 1);
    push(1, 2);
    push(1, 1);
    push(0, 1);
    let mut context = vec![0, 2];
    for chunk in context_bits.chunks(8) {
        let mut byte = 0u8;
        for bit in chunk {
            byte = (byte << 1) | u8::from(*bit);
        }
        byte <<= 8 - chunk.len();
        context.push(byte);
    }
    let mut packet = Vec::new();
    packet.extend_from_slice(&3_u32.to_le_bytes());
    packet.push(3);
    packet.extend_from_slice(&16_u32.to_le_bytes());
    packet.extend_from_slice(&0_u32.to_le_bytes());
    packet.extend_from_slice(&context);
    packet.extend_from_slice(&0_u32.to_le_bytes());
    assert_eq!(
        crate::jt::decode_int32_cdp2(&packet, 0),
        Some((vec![7, 7, 7], packet.len()))
    );

    packet.truncate(packet.len() - 4);
    assert!(crate::jt::decode_int32_cdp2(&packet, 0).is_none());
}

#[test]
fn jt_int32_cdp2_decodes_unsplit_and_split_chopper_packets() {
    let nested = [2, 0, 0, 0, 1, 21, 0, 0, 0, 0x00, 0xc0, 0x16, 0x04];
    let low_bits = [2, 0, 0, 0, 1, 17, 0, 0, 0, 0x00, 0x80, 0x12, 0x04];
    let mut unsplit = vec![2, 0, 0, 0, 4, 0];
    unsplit.extend_from_slice(&nested);
    assert_eq!(
        crate::jt::decode_int32_cdp2(&unsplit, 0),
        Some((vec![1, -1], unsplit.len()))
    );

    let mut split = vec![2, 0, 0, 0, 4, 2];
    split.extend_from_slice(&10_i32.to_le_bytes());
    split.push(4);
    split.extend_from_slice(&nested);
    split.extend_from_slice(&low_bits);
    assert_eq!(
        crate::jt::decode_int32_cdp2(&split, 0),
        Some((vec![15, 7], split.len()))
    );
}

#[test]
fn jt_int32_cdp2_frames_zero_chop_nested_packet() {
    let nested = [2, 0, 0, 0, 1, 21, 0, 0, 0, 0x00, 0xc0, 0x16, 0x04];
    let mut packet = vec![2, 0, 0, 0, 4, 0];
    packet.extend_from_slice(&nested);
    assert_eq!(
        crate::jt::frame_int32_cdp2(&packet, 0),
        Some((2, 4, packet.len()))
    );

    packet[6] = 3;
    assert!(crate::jt::frame_int32_cdp2(&packet, 0).is_none());
}

#[test]
fn jt_predictors_reconstruct_primal_integers() {
    use crate::jt::{unpack_predictor_residuals, Predictor};

    let primers = [10, 20, 30, 40];
    let residuals = [10, 20, 30, 40, 5, -2];
    assert_eq!(
        unpack_predictor_residuals(&residuals, Predictor::Lag1),
        [10, 20, 30, 40, 45, 43]
    );
    assert_eq!(
        unpack_predictor_residuals(&residuals, Predictor::Lag2),
        [10, 20, 30, 40, 35, 38]
    );
    assert_eq!(
        unpack_predictor_residuals(&residuals, Predictor::Stride1),
        [10, 20, 30, 40, 55, 68]
    );
    assert_eq!(
        unpack_predictor_residuals(&residuals, Predictor::Stride2),
        [10, 20, 30, 40, 55, 58]
    );
    assert_eq!(
        unpack_predictor_residuals(&residuals, Predictor::StripIndex),
        [10, 20, 30, 40, 37, 40]
    );
    assert_eq!(
        unpack_predictor_residuals(&residuals, Predictor::Ramp),
        [10, 20, 30, 40, 9, 3]
    );
    assert_eq!(
        unpack_predictor_residuals(&[10, 20, 30, 40, 0x2d ^ 0x28], Predictor::Xor1),
        [10, 20, 30, 40, 45]
    );
    assert_eq!(
        unpack_predictor_residuals(&[10, 20, 30, 40, 0x23 ^ 0x1e], Predictor::Xor2),
        [10, 20, 30, 40, 35]
    );
    assert_eq!(
        unpack_predictor_residuals(&residuals, Predictor::Null),
        residuals
    );
    assert_eq!(primers, residuals[..4]);
}

#[test]
fn jt_predictors_use_wrapping_i32_arithmetic() {
    use crate::jt::{unpack_predictor_residuals, Predictor};

    assert_eq!(
        unpack_predictor_residuals(&[0, 0, 0, i32::MAX, 1], Predictor::Lag1),
        [0, 0, 0, i32::MAX, i32::MIN]
    );
}

#[test]
fn jt_topological_dual_mesh_reconstructs_closed_tetrahedron() {
    let polygons = crate::jt_topology::decode(
        [&[3, 3, 3], &[3], &[], &[], &[], &[], &[], &[]],
        &[3, 3, 3, 3],
        &[10, 12, 11, 13],
        &[0, 0, 0, 0],
        &[],
        &[],
        crate::jt_topology::AttributeMaskLanes {
            small: [&[], &[1, 1, 1, 1], &[], &[], &[], &[], &[], &[]],
            context_7_next_30: &[],
            context_7_upper_4: &[],
            large_words: &[],
        },
    )
    .expect("valid closed dual mesh");

    assert_eq!(
        polygons
            .iter()
            .map(|polygon| polygon.vertex_indices.as_slice())
            .collect::<Vec<_>>(),
        vec![&[0, 1, 2], &[2, 1, 3], &[2, 3, 0], &[3, 1, 0]]
    );
    assert_eq!(
        polygons
            .iter()
            .map(|polygon| polygon.group)
            .collect::<Vec<_>>(),
        vec![10, 12, 11, 13]
    );
    assert_eq!(
        polygons[0].attribute_indices,
        vec![Some(0), Some(1), Some(2)]
    );
}

#[test]
fn jt_uniform_dequantization_uses_the_full_unsigned_code_range() {
    assert_eq!(
        crate::jt::dequantize_uniform(0, [10.0, 20.0], 2),
        Some(8.333_333)
    );
    assert_eq!(
        crate::jt::dequantize_uniform(3, [10.0, 20.0], 2),
        Some(18.333_334)
    );
    assert_eq!(crate::jt::dequantize_uniform(4, [10.0, 20.0], 2), None);
    assert_eq!(crate::jt::dequantize_uniform(-1, [4.0, 4.0], 32), Some(4.0));
}

#[test]
fn jt_quantized_coordinate_array_decodes_three_lag1_code_vectors() {
    let mut code = Vec::new();
    let mut push = |value: u32, width: u8| {
        code.extend((0..width).rev().map(|shift| ((value >> shift) & 1) as u8));
    };
    push(0, 1);
    push(0, 6);
    push(3, 6);
    push(3, 3);
    for value in 0..4 {
        push(value, 2);
    }
    let mut word = 0u32;
    for bit in &code {
        word = (word << 1) | u32::from(*bit);
    }
    word <<= 32 - code.len();
    let mut packet = 4_u32.to_le_bytes().to_vec();
    packet.push(1);
    packet.extend_from_slice(&(code.len() as u32).to_le_bytes());
    packet.extend_from_slice(&word.to_le_bytes());
    let mut array = Vec::new();
    for _ in 0..3 {
        array.extend_from_slice(&packet);
    }
    array.extend_from_slice(&0x1234_5678_u32.to_le_bytes());

    let (points, hash, consumed) =
        crate::jt::decode_vertex_coordinates(&array, 4, [[10.0, 20.0]; 3], [2; 3])
            .expect("complete quantized coordinate array");
    assert_eq!(hash, 0x1234_5678);
    assert_eq!(consumed, array.len());
    assert_eq!(points[0], [8.333_333; 3]);
    assert_eq!(points[3], [18.333_334; 3]);
}

#[test]
fn jt_deering_normal_applies_sextant_octant_and_code_bounds() {
    let normal = crate::jt::deering_normal(1, 7, 8191, 0, 13).unwrap();
    assert!(normal[0].abs() < 1e-3);
    assert!(normal[1].abs() < 1e-6);
    assert!((normal[2] - 1.0).abs() < 1e-6);
    assert!(crate::jt::deering_normal(6, 7, 0, 0, 13).is_none());
    assert!(crate::jt::deering_normal(0, 8, 0, 0, 13).is_none());
    assert!(crate::jt::deering_normal(0, 7, 8192, 0, 13).is_none());
}

#[test]
fn jt_quantized_texture_coordinates_decode_component_major_lag1_codes() {
    let mut code = Vec::new();
    let mut push = |value: u32, width: u8| {
        code.extend((0..width).rev().map(|shift| ((value >> shift) & 1) as u8));
    };
    push(0, 1);
    push(0, 6);
    push(3, 6);
    push(3, 3);
    for value in 0..4 {
        push(value, 2);
    }
    let mut word = 0u32;
    for bit in &code {
        word = (word << 1) | u32::from(*bit);
    }
    word <<= 32 - code.len();
    let mut packet = 4_u32.to_le_bytes().to_vec();
    packet.push(1);
    packet.extend_from_slice(&(code.len() as u32).to_le_bytes());
    packet.extend_from_slice(&word.to_le_bytes());

    let mut array = 4_u32.to_le_bytes().to_vec();
    array.extend_from_slice(&[2, 2]);
    for _ in 0..2 {
        array.extend_from_slice(&0_f32.to_le_bytes());
        array.extend_from_slice(&3_f32.to_le_bytes());
        array.push(2);
    }
    array.extend_from_slice(&packet);
    array.extend_from_slice(&packet);
    array.extend_from_slice(&0x8765_4321_u32.to_le_bytes());

    let (values, hash, consumed) =
        crate::jt::decode_vertex_texture_coordinates(&array, 4, 2).unwrap();
    assert_eq!(hash, 0x8765_4321);
    assert_eq!(consumed, array.len());
    assert_eq!(values[0], vec![-0.5, -0.5]);
    assert_eq!(values[3], vec![2.5, 2.5]);
}

#[test]
fn jt_quantized_colors_decode_rgb_and_hsv_quantizers() {
    let mut code = Vec::new();
    let mut push = |value: u32, width: u8| {
        code.extend((0..width).rev().map(|shift| ((value >> shift) & 1) as u8));
    };
    push(0, 1);
    push(0, 6);
    push(3, 6);
    push(3, 3);
    for value in 0..4 {
        push(value, 2);
    }
    let mut word = 0u32;
    for bit in &code {
        word = (word << 1) | u32::from(*bit);
    }
    word <<= 32 - code.len();
    let mut packet = 4_u32.to_le_bytes().to_vec();
    packet.push(1);
    packet.extend_from_slice(&(code.len() as u32).to_le_bytes());
    packet.extend_from_slice(&word.to_le_bytes());

    let mut rgb = 4_u32.to_le_bytes().to_vec();
    rgb.extend_from_slice(&[3, 2, 0]);
    for _ in 0..4 {
        rgb.extend_from_slice(&0_f32.to_le_bytes());
        rgb.extend_from_slice(&3_f32.to_le_bytes());
        rgb.push(2);
    }
    for _ in 0..4 {
        rgb.extend_from_slice(&packet);
    }
    rgb.extend_from_slice(&0x1234_5678_u32.to_le_bytes());
    let (colors, hash, consumed) = crate::jt::decode_vertex_colors(&rgb, 4, 2).unwrap();
    assert_eq!(hash, 0x1234_5678);
    assert_eq!(consumed, rgb.len());
    assert_eq!(colors[0], [-0.5; 4]);
    assert_eq!(colors[3], [2.5; 4]);

    let mut hsv = 4_u32.to_le_bytes().to_vec();
    hsv.extend_from_slice(&[4, 2, 1, 2, 2, 2, 2]);
    for _ in 0..4 {
        hsv.extend_from_slice(&packet);
    }
    hsv.extend_from_slice(&0x8765_4321_u32.to_le_bytes());
    let (colors, hash, consumed) = crate::jt::decode_vertex_colors(&hsv, 4, 2).unwrap();
    assert_eq!(hash, 0x8765_4321);
    assert_eq!(consumed, hsv.len());
    assert!(colors
        .iter()
        .flatten()
        .all(|component| component.is_finite()));
    assert!((colors[1][0] - 1.0 / 6.0).abs() < 1e-6);
    assert!((colors[1][1] - 1.0 / 6.0).abs() < 1e-6);
    assert!((colors[1][2] - 5.0 / 36.0).abs() < 1e-6);
    assert!((colors[1][3] - 1.0 / 6.0).abs() < 1e-6);
}

#[test]
fn jt_vertex_flags_require_a_complete_binary_value_packet() {
    let mut bits = vec![0];
    let mut field = |value: u32, width: u8| {
        bits.extend((0..width).rev().map(|shift| ((value >> shift) & 1) as u8));
    };
    field(1, 6);
    field(2, 6);
    field(0, 1);
    field(1, 2);
    field(0, 1);
    field(1, 1);
    field(0, 1);
    let mut word = 0u32;
    for bit in &bits {
        word = (word << 1) | u32::from(*bit);
    }
    word <<= 32 - bits.len();
    let mut packet = 3_u32.to_le_bytes().to_vec();
    packet.push(1);
    packet.extend_from_slice(&(bits.len() as u32).to_le_bytes());
    packet.extend_from_slice(&word.to_le_bytes());
    let mut array = 3_u32.to_le_bytes().to_vec();
    array.extend_from_slice(&packet);

    assert_eq!(
        crate::jt::decode_vertex_flags(&array, 3),
        Some((vec![0, 1, 0], array.len()))
    );
    assert!(crate::jt::decode_vertex_flags(&array, 2).is_none());
    let last = array.len() - 1;
    array[last] |= 1;
    assert!(crate::jt::decode_vertex_flags(&array, 3).is_none());
}

#[test]
fn nx_hole_completeness_accepts_independent_placement_and_rejects_opaque_operands() {
    use cadmpeg_ir::features::{Extent, FaceSelection, HoleKind, Length, ProfileRef};
    use cadmpeg_ir::math::{Point3, Vector3};

    assert!(!crate::decode::hole_feature_is_incomplete(
        None,
        None,
        Some(Point3::new(1.0, 2.0, 3.0)),
        Some(Vector3::new(0.0, 0.0, 1.0)),
        (&HoleKind::Simple, None),
        Some(Length(5.0)),
        Some(&Extent::ThroughAll),
    ));
    assert!(crate::decode::hole_feature_is_incomplete(
        Some(&ProfileRef::Unresolved("hole".into())),
        Some(&FaceSelection::Unresolved),
        None,
        None,
        (&HoleKind::Simple, None),
        Some(Length(5.0)),
        Some(&Extent::ThroughAll),
    ));
    assert!(crate::decode::hole_feature_is_incomplete(
        None,
        None,
        Some(Point3::new(1.0, 2.0, 3.0)),
        Some(Vector3::new(0.0, 0.0, 1.0)),
        (&HoleKind::Simple, None),
        Some(Length(5.0)),
        Some(&Extent::Unresolved),
    ));
    assert!(crate::decode::hole_feature_is_incomplete(
        None,
        None,
        Some(Point3::new(1.0, 2.0, 3.0)),
        Some(Vector3::new(0.0, 0.0, 1.0)),
        (
            &HoleKind::Simple,
            Some(&HoleKind::Unresolved {
                form: Some(cadmpeg_ir::features::HoleForm::Chamfer),
                counterbore_diameter: None,
                counterbore_depth: None,
                countersink_diameter: None,
                countersink_angle: None,
            }),
        ),
        Some(Length(5.0)),
        Some(&Extent::ThroughAll),
    ));
}

#[test]
fn nx_extent_completeness_checks_nested_and_face_termination() {
    use cadmpeg_ir::features::{Extent, FaceSelection, Length};

    assert!(!crate::decode::extent_is_incomplete(
        &Extent::TwoSidedExtents {
            first: Box::new(Extent::Blind {
                length: Length(5.0),
            }),
            second: Box::new(Extent::ThroughAll),
        }
    ));
    assert!(crate::decode::extent_is_incomplete(
        &Extent::SymmetricExtent {
            extent: Box::new(Extent::Unresolved),
        }
    ));
    assert!(crate::decode::extent_is_incomplete(&Extent::ToFace {
        face: FaceSelection::Native("nx:face-selection#0".to_string()),
        offset: None,
    }));
    assert!(crate::decode::extent_is_incomplete(&Extent::ToShape {
        target: FaceSelection::Resolved {
            faces: Vec::new(),
            native: "nx:face-selection#1".to_string(),
        },
    }));
}

#[test]
fn nx_rib_completeness_requires_a_resolved_profile() {
    use cadmpeg_ir::features::{BooleanOp, Length, ProfileRef, RibConstruction, RibDraft, RibSide};
    use cadmpeg_ir::math::Vector3;

    let mut construction = RibConstruction {
        profile: Some(ProfileRef::Native("nx:profile#0".to_string())),
        direction: Some(Vector3::new(0.0, 0.0, 1.0)),
        thickness: Some(Length(2.0)),
        side: Some(RibSide::Centered),
        draft: RibDraft::None,
    };
    assert!(crate::decode::rib_feature_is_incomplete(
        &construction,
        BooleanOp::Join,
    ));
    construction.profile = Some(ProfileRef::Faces(vec![cadmpeg_ir::ids::FaceId(
        "face#0".to_string(),
    )]));
    assert!(!crate::decode::rib_feature_is_incomplete(
        &construction,
        BooleanOp::Join,
    ));
    construction.profile = Some(ProfileRef::Faces(Vec::new()));
    assert!(crate::decode::rib_feature_is_incomplete(
        &construction,
        BooleanOp::Join,
    ));
}

#[test]
fn nx_pattern_completeness_requires_every_regeneration_operand() {
    use cadmpeg_ir::features::{
        Length, PathRef, PatternKind, PatternStage, PatternStageCombination,
    };
    use cadmpeg_ir::math::Vector3;

    let linear = PatternKind::Linear {
        direction: Some(Vector3::new(1.0, 0.0, 0.0)),
        spacing: Length(10.0),
        count: 3,
        second: None,
    };
    assert!(!crate::decode::pattern_is_incomplete(&linear));
    assert!(crate::decode::pattern_is_incomplete(&PatternKind::Linear {
        direction: None,
        spacing: Length(10.0),
        count: 3,
        second: None,
    }));
    assert!(crate::decode::pattern_is_incomplete(
        &PatternKind::CurveDriven {
            path: Some(PathRef::Native("nx:path".into())),
            spacing: Length(10.0),
            count: 3,
        }
    ));
    assert!(crate::decode::pattern_is_incomplete(
        &PatternKind::Composite {
            stages: vec![PatternStage {
                pattern: Box::new(PatternKind::Linear {
                    direction: None,
                    spacing: Length(10.0),
                    count: 3,
                    second: None,
                }),
                combination: PatternStageCombination::Initialize,
            }],
        }
    ));
}

#[test]
fn nx_variable_radius_completeness_requires_a_law_interval() {
    use cadmpeg_ir::features::{Length, RadiusSpec, VariableRadius};

    assert!(crate::decode::radius_spec_is_incomplete(
        &RadiusSpec::Variable { points: Vec::new() }
    ));
    assert!(crate::decode::radius_spec_is_incomplete(
        &RadiusSpec::Variable {
            points: vec![VariableRadius {
                parameter: 0.0,
                radius: Length(2.0),
            }],
        }
    ));
    assert!(!crate::decode::radius_spec_is_incomplete(
        &RadiusSpec::Variable {
            points: vec![
                VariableRadius {
                    parameter: 0.0,
                    radius: Length(2.0),
                },
                VariableRadius {
                    parameter: 1.0,
                    radius: Length(3.0),
                },
            ],
        }
    ));
    assert!(!crate::decode::radius_spec_is_incomplete(
        &RadiusSpec::Constant {
            radius: Length(2.0),
        }
    ));
}

#[test]
fn nx_empty_resolved_selections_remain_incomplete() {
    use cadmpeg_ir::features::{BodySelection, EdgeSelection, FaceSelection, PathRef, ProfileRef};

    assert!(crate::decode::body_selection_is_incomplete(
        &BodySelection::Bodies(Vec::new())
    ));
    assert!(crate::decode::face_selection_is_incomplete(
        &FaceSelection::Resolved {
            faces: Vec::new(),
            native: "nx:faces".into(),
        }
    ));
    assert!(crate::decode::edge_selection_is_incomplete(
        &EdgeSelection::Edges(Vec::new())
    ));
    assert!(!crate::decode::edge_selection_is_incomplete(
        &EdgeSelection::All
    ));
    assert!(crate::decode::profile_ref_is_incomplete(
        &ProfileRef::Faces(Vec::new())
    ));
    assert!(crate::decode::path_ref_is_incomplete(&PathRef::Curves(
        Vec::new()
    )));
    let edge = cadmpeg_ir::ids::EdgeId("edge#0".into());
    assert!(crate::decode::path_ref_is_incomplete(&PathRef::Edges(
        vec![edge.clone(), edge]
    )));
    let curve = cadmpeg_ir::ids::CurveId("curve#0".into());
    assert!(crate::decode::path_ref_is_incomplete(&PathRef::Curves(
        vec![curve.clone(), curve]
    )));
}

#[test]
fn om_index_pairs_object_ids_with_bounded_entity_records() {
    let bytes = indexed_om_section();
    let sections = crate::om::indexed_sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].base, 8);
    assert_eq!(sections[0].records.len(), 2);
    assert_eq!(sections[0].records[0].object_id, Some(0x101));
    assert_eq!(
        sections[0].records[0].object_id_offset,
        Some(sections[0].object_id_table_offset + 8)
    );
    assert_eq!(
        sections[0].records[0].bytes,
        b"\x04\x01\x0eNX 2027.3102\x00hostglobalvariables"
    );
    assert_eq!(sections[0].records[1].object_id, Some(0x102));
    assert_eq!(
        sections[0].records[1].object_id_offset,
        Some(sections[0].object_id_table_offset + 12)
    );
    assert_eq!(sections[0].column_storage, None);
    assert_eq!(sections[0].fields.len(), 1);
    assert_eq!(sections[0].fields[0].name, "m_target");
    assert_eq!(
        sections[0].records[1].bytes,
        b"\x04\x36p8_CircularPattern_pattern_Circular_Dir_offset_angle\x00\x04\x05120\x00\x99\x04P(Number [degrees]) p8_CircularPattern_pattern_Circular_Dir_offset_angle: 120; \x00\x66\x32\x03\x0cSKETCH_001\0\xe0\x12\x34\x56\x78\xca\xbc\xde\xf0\x01\x02\x90\x00\x00"
    );
}

#[test]
fn ug_part_segment_index_uses_row_one_self_boundary() {
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_index_payload())]);
    let container = container::scan_bytes(file).unwrap();
    let (_, index) = container.segment_index().expect("segment index");
    assert_eq!(index.byte_len, 28);
    assert_eq!(index.rows.len(), 2);
    assert_eq!(index.rows[0].type_code, 7);
    assert_eq!(index.rows[0].subtype_code, 9);
    assert_eq!(index.rows[0].value, 11);
    assert_eq!(index.rows[1].type_code, 1);
    assert_eq!(index.rows[1].subtype_code, 1);
    assert_eq!(index.rows[1].value, 28);
    assert_eq!(index.padding, &[0xaa, 0xbb, 0xcc, 0xdd]);
}

#[test]
fn nx_pattern_completeness_requires_distinct_seeds() {
    let seed = cadmpeg_ir::features::PatternSeed::Feature(cadmpeg_ir::features::FeatureId(
        "test:feature#seed".into(),
    ));
    let pattern = cadmpeg_ir::features::PatternKind::Mirror {
        plane_origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        plane_normal: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
    };

    assert!(!crate::decode::pattern_feature_is_incomplete(
        std::slice::from_ref(&seed),
        &pattern,
    ));
    assert!(crate::decode::pattern_feature_is_incomplete(
        &[seed.clone(), seed],
        &pattern,
    ));
}

#[test]
fn nx_face_blend_completeness_requires_disjoint_supports() {
    use cadmpeg_ir::features::FaceSelection;
    use cadmpeg_ir::ids::FaceId;

    let shared = FaceId("test:face#shared".into());
    let distinct = FaceId("test:face#distinct".into());
    let first = FaceSelection::Faces(vec![shared.clone()]);

    assert!(crate::decode::face_selections_overlap(
        &first,
        &FaceSelection::Resolved {
            faces: vec![shared],
            native: "test:first-support".into(),
        },
    ));
    assert!(!crate::decode::face_selections_overlap(
        &first,
        &FaceSelection::Faces(vec![distinct]),
    ));
    assert!(!crate::decode::face_selections_overlap(
        &first,
        &FaceSelection::Unresolved,
    ));
}

#[test]
fn nx_selection_completeness_rejects_repeated_faces_and_edges() {
    use cadmpeg_ir::features::{EdgeSelection, FaceSelection, ProfileRef};
    use cadmpeg_ir::ids::{EdgeId, FaceId};

    let face = FaceId("test:face#repeated".into());
    assert!(crate::decode::face_selection_is_incomplete(
        &FaceSelection::Faces(vec![face.clone(), face]),
    ));

    let face = FaceId("test:profile-face#repeated".into());
    assert!(crate::decode::profile_ref_is_incomplete(
        &ProfileRef::Faces(vec![face.clone(), face]),
    ));

    let edge = EdgeId("test:edge#repeated".into());
    assert!(crate::decode::edge_selection_is_incomplete(
        &EdgeSelection::Edges(vec![edge.clone(), edge]),
    ));
}

#[test]
fn nx_hole_completeness_rejects_opaque_supplied_operands() {
    use cadmpeg_ir::features::{Extent, FaceSelection, HoleKind, Length, ProfileRef};
    use cadmpeg_ir::math::{Point3, Vector3};

    let incomplete = |profile, face| {
        crate::decode::hole_feature_is_incomplete(
            profile,
            face,
            Some(Point3::new(0.0, 0.0, 0.0)),
            Some(Vector3::new(0.0, 0.0, 1.0)),
            (&HoleKind::Simple, None),
            Some(Length(1.0)),
            Some(&Extent::ThroughAll),
        )
    };

    assert!(!incomplete(None, None));
    let unresolved_profile = ProfileRef::Unresolved("hole".into());
    assert!(incomplete(Some(&unresolved_profile), None));
    assert!(incomplete(None, Some(&FaceSelection::Unresolved)));
}

#[test]
fn nx_sketch_completeness_reports_native_geometry_and_constraints() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, SketchSpace};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
        SketchEntityId, SketchGeometry, SketchId,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube();
    let sketch_id = SketchId("test:sketch#0".into());
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#sketch".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: SketchSpace::Planar,
            sketch: Some(sketch_id.clone()),
        },
        native_ref: None,
    });
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    });
    let entity_id = SketchEntityId("test:sketch-entity#0".into());
    ir.model.sketch_entities.push(SketchEntity {
        id: entity_id.clone(),
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Native {
            native_kind: "test".into(),
        },
    });
    ir.model.sketch_constraints.push(SketchConstraint {
        id: SketchConstraintId("test:sketch-constraint#0".into()),
        sketch: sketch_id,
        definition: SketchConstraintDefinition::Native {
            native_kind: "test".into(),
            entities: vec![entity_id],
            parameter: None,
            operands: Vec::new(),
            native_state: None,
        },
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    });

    let mut losses = Vec::new();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0]
        .message
        .contains("1 NX sketch geometry record(s) and 1 sketch constraint"));
}

#[test]
fn nx_sketch_completeness_requires_planar_space() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, SketchSpace};
    use cadmpeg_ir::sketches::SketchId;

    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#sketch".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: SketchSpace::Spatial,
            sketch: Some(SketchId("test:sketch#0".into())),
        },
        native_ref: None,
    });

    let mut losses = Vec::new();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert!(losses.iter().any(|loss| {
        loss.message.contains(
            "construction fields or output lineage remain unresolved or native-only: sketch (1)",
        )
    }));
}

#[test]
fn nx_body_operation_completeness_requires_disjoint_roles() {
    use cadmpeg_ir::features::BodySelection;
    use cadmpeg_ir::ids::BodyId;

    let shared = BodyId("test:body#shared".into());
    let distinct = BodyId("test:body#distinct".into());
    let target = BodySelection::Bodies(vec![shared.clone()]);

    assert!(crate::decode::body_selection_is_incomplete(
        &BodySelection::Bodies(vec![shared.clone(), shared.clone()]),
    ));
    assert!(!crate::decode::body_selection_is_incomplete(&target));

    assert!(crate::decode::body_selections_overlap(
        &target,
        &BodySelection::Resolved {
            bodies: vec![shared],
            native: "test:tools".into(),
        },
    ));
    assert!(!crate::decode::body_selections_overlap(
        &target,
        &BodySelection::Bodies(vec![distinct]),
    ));
    assert!(!crate::decode::body_selections_overlap(
        &target,
        &BodySelection::Unresolved,
    ));
}

#[test]
fn nx_configuration_completeness_requires_one_active_full_body_set() {
    use cadmpeg_ir::features::{ConfigurationBodies, ConfigurationId, DesignConfiguration};

    let mut ir = cadmpeg_ir::examples::unit_cube();
    let bodies = ir
        .model
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<Vec<_>>();
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId("test:configuration#0".into()),
        ordinal: 0,
        active: true,
        source_index: Some(0),
        name: "Model".into(),
        material: None,
        properties: Default::default(),
        parameter_overrides: Default::default(),
        suppressed_features: Vec::new(),
        bodies: ConfigurationBodies::Resolved(Vec::new()),
        parameter_values: Default::default(),
        feature_states: Default::default(),
        native_ref: None,
    });

    let mut losses = Vec::new();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("1 NX design configuration"));

    ir.model.configurations[0].bodies = ConfigurationBodies::Resolved(bodies);
    losses.clear();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());

    ir.model.configurations[0].active = false;
    losses.clear();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("1 NX design configuration"));
}

#[test]
fn nx_body_producing_feature_families_require_history_outputs() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, Length};
    use std::collections::BTreeMap;

    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#block".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Block {
            dimensions: Some([Length(1.0), Length(2.0), Length(3.0)]),
            placement: Some(cadmpeg_ir::transform::Transform::identity()),
        },
        native_ref: None,
    });

    let mut losses = Vec::new();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("block (1)"));

    let output = cadmpeg_ir::ids::BodyId("test:body#output".into());
    ir.model.features[0].outputs = vec![output.clone()];
    losses.clear();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("block (1)"));

    ir.model.features[0].outputs = vec![output.clone(), output.clone()];
    losses.clear();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("block (1)"));

    ir.model.features[0].suppressed = Some(true);
    losses.clear();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());

    ir.model.features[0].definition = FeatureDefinition::Loft {
        sections: Vec::new(),
        centerline: None,
        guides: Vec::new(),
        op: cadmpeg_ir::features::BooleanOp::Unresolved,
        closed: false,
        solid: false,
        ruled: false,
        max_degree: None,
        check_compatibility: None,
        allow_multi_profile_faces: None,
    };
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("loft (1)"));

    ir.model.features[0].definition = FeatureDefinition::Draft {
        faces: cadmpeg_ir::features::FaceSelection::Unresolved,
        neutral_plane: cadmpeg_ir::features::FaceSelection::Unresolved,
        pull_direction: Some(cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)),
        angle: Some(cadmpeg_ir::features::Angle(0.1)),
        outward: Some(false),
    };
    losses.clear();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("draft (1)"));

    ir.model.features[0].definition = FeatureDefinition::DatumOffsetPlane {
        reference: None,
        distance: Length(5.0),
    };
    losses.clear();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("datum plane (1)"));

    let datum = FeatureId("test:feature#datum-source".into());
    ir.model.features[0].definition = FeatureDefinition::DatumOffsetPlane {
        reference: Some(datum.clone()),
        distance: Length(5.0),
    };
    losses.clear();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("datum plane (1)"));

    ir.model.features[0].ordinal = 1;
    ir.model.features.push(Feature {
        id: datum.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::DatumPrincipalPlane {
            plane: cadmpeg_ir::features::PrincipalPlane::Top,
        },
        native_ref: None,
    });
    losses.clear();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("datum plane (1)"));

    ir.model.features[0].dependencies.push(datum);
    losses.clear();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());

    ir.model.features[0].definition = FeatureDefinition::SewBodies {
        bodies: cadmpeg_ir::features::BodySelection::Bodies(vec![output.clone()]),
        gap_tolerance: Some(Length(0.01)),
    };
    losses.clear();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("sew bodies (1)"));

    assert_eq!(
        crate::decode::body_output_feature_family(&FeatureDefinition::DatumPointUnresolved),
        None
    );
    assert_eq!(
        crate::decode::body_output_feature_family(&FeatureDefinition::Loft {
            sections: Vec::new(),
            centerline: None,
            guides: Vec::new(),
            op: cadmpeg_ir::features::BooleanOp::NewBody,
            closed: false,
            solid: false,
            ruled: false,
            max_degree: None,
            check_compatibility: None,
            allow_multi_profile_faces: None,
        }),
        Some("loft")
    );
    assert_eq!(
        crate::decode::body_output_feature_family(&FeatureDefinition::Draft {
            faces: cadmpeg_ir::features::FaceSelection::Unresolved,
            neutral_plane: cadmpeg_ir::features::FaceSelection::Unresolved,
            pull_direction: Some(cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)),
            angle: Some(cadmpeg_ir::features::Angle(0.1)),
            outward: Some(false),
        }),
        Some("draft")
    );
    assert_eq!(
        crate::decode::body_output_feature_family(&FeatureDefinition::DeleteBody {
            bodies: cadmpeg_ir::features::BodySelection::Unresolved,
            mode: cadmpeg_ir::features::BodyRetentionMode::DeleteSelected,
        }),
        None
    );
}

#[test]
fn nx_sew_completeness_does_not_invent_a_gap_tolerance() {
    use cadmpeg_ir::features::{BodySelection, Feature, FeatureDefinition, FeatureId};

    let mut ir = cadmpeg_ir::examples::unit_cube();
    let first = ir.model.bodies[0].id.clone();
    let mut second_body = ir.model.bodies[0].clone();
    second_body.id = cadmpeg_ir::ids::BodyId("test:body#second".into());
    let second = second_body.id.clone();
    ir.model.bodies.push(second_body);
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#sew".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: vec![first.clone()],
        definition: FeatureDefinition::SewBodies {
            bodies: BodySelection::Bodies(vec![first, second]),
            gap_tolerance: None,
        },
        native_ref: None,
    });

    let mut losses = Vec::new();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());
}

#[test]
fn nx_circular_cone_offsets_resolve_across_equivalent_axis_origins() {
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::math::{Point3, Vector3};

    let angle = std::f64::consts::FRAC_PI_6;
    let support = SurfaceGeometry::Cone {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 4.0,
        ratio: 1.0,
        half_angle: angle,
    };
    let expected = 2.0;
    let axial_shift = -expected * angle.sin();
    let offset = SurfaceGeometry::Cone {
        origin: Point3::new(0.0, 0.0, axial_shift),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 4.0 + expected * angle.cos(),
        ratio: 1.0,
        half_angle: angle,
    };

    let distance = crate::decode::analytic_surface_offset(&support, &offset).expect("offset");
    assert!((distance - expected).abs() <= 1e-12);
    let reverse = crate::decode::analytic_surface_offset(&offset, &support).expect("reverse");
    assert!((reverse + expected).abs() <= 1e-12);

    let mut lateral = offset.clone();
    let SurfaceGeometry::Cone { origin, .. } = &mut lateral else {
        unreachable!()
    };
    origin.x = 0.1;
    assert!(crate::decode::analytic_surface_offset(&support, &lateral).is_none());

    let mut shifted_parameterization = offset.clone();
    let SurfaceGeometry::Cone { origin, .. } = &mut shifted_parameterization else {
        unreachable!()
    };
    origin.z += 0.1;
    assert!(crate::decode::analytic_surface_offset(&support, &shifted_parameterization).is_none());

    let mut elliptical = offset;
    let SurfaceGeometry::Cone { ratio, .. } = &mut elliptical else {
        unreachable!()
    };
    *ratio = 0.5;
    assert!(crate::decode::analytic_surface_offset(&support, &elliptical).is_none());
}

#[test]
fn nx_sphere_offset_lineage_follows_signed_radius_orientation() {
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::math::{Point3, Vector3};

    let sphere = |radius| SurfaceGeometry::Sphere {
        center: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius,
    };
    assert_eq!(
        crate::decode::analytic_surface_offset(&sphere(4.0), &sphere(6.5)),
        Some(2.5)
    );
    assert_eq!(
        crate::decode::analytic_surface_offset(&sphere(-4.0), &sphere(-6.5)),
        Some(2.5)
    );
    assert_eq!(
        crate::decode::analytic_surface_offset(&sphere(-6.5), &sphere(-4.0)),
        Some(-2.5)
    );
    assert!(crate::decode::analytic_surface_offset(&sphere(4.0), &sphere(-6.5)).is_none());
}

#[test]
fn nx_torus_offset_lineage_requires_one_ring_orientation() {
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::math::{Point3, Vector3};

    let torus = |minor_radius| SurfaceGeometry::Torus {
        center: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 10.0,
        minor_radius,
    };
    assert_eq!(
        crate::decode::analytic_surface_offset(&torus(2.0), &torus(3.5)),
        Some(1.5)
    );
    assert_eq!(
        crate::decode::analytic_surface_offset(&torus(-2.0), &torus(-3.5)),
        Some(1.5)
    );
    assert_eq!(
        crate::decode::analytic_surface_offset(&torus(-3.5), &torus(-2.0)),
        Some(-1.5)
    );
    assert!(crate::decode::analytic_surface_offset(&torus(2.0), &torus(-3.5)).is_none());
    assert!(crate::decode::analytic_surface_offset(&torus(2.0), &torus(10.0)).is_none());
}

#[test]
fn om_compact_index_lane_decodes_direct_extended_and_null_entries() {
    use crate::om::CompactIndex::{Null, Value};

    assert_eq!(
        crate::om::compact_indices(&[0x00, 0x7f, 0x80, 0x80, 0x81, 0x00, 0xfe, 0xff, 0xff]),
        Some(vec![
            Value(0),
            Value(127),
            Value(128),
            Value(256),
            Value(32_511),
            Null,
        ])
    );
    assert_eq!(crate::om::compact_indices(&[0x80]), None);
}

#[test]
fn om_data_block_object_frame_requires_complete_discriminator() {
    let discriminator = [
        0x00, 0x72, 0x01, 0xc0, 0x20, 0x02, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x01,
        0x02, 0x80, 0xa4,
    ];
    let mut bytes = vec![0xaa, 0x81, 0x72];
    bytes.extend_from_slice(&discriminator);
    bytes.push(0xff);

    let references = crate::om::data_block_object_frames(&bytes);
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].object_id, 370);
    assert_eq!(references[0].raw_object_id, [0x81, 0x72]);
    assert_eq!(references[0].offset, 1);

    bytes.extend_from_slice(&[0x73]);
    bytes.extend_from_slice(&discriminator);
    let references = crate::om::data_block_object_frames(&bytes);
    assert_eq!(references.len(), 2);
    assert_eq!(references[1].object_id, 0x73);
    assert_eq!(references[1].raw_object_id, [0x73]);
    assert_eq!(references[1].offset, 22);

    bytes[8] ^= 1;
    let references = crate::om::data_block_object_frames(&bytes);
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].object_id, 0x73);
    let mut null = vec![0xff];
    null.extend_from_slice(&discriminator);
    assert!(crate::om::data_block_object_frames(&null).is_empty());
}

#[test]
fn om_offset_store_counted_index_lane_requires_complete_non_null_members() {
    let bytes = [
        0xaa, 0x01, 0x06, 0x42, 0x62, 0x80, 0x48, 0x80, 0x50, 0x7c, 0x01, 0x11, 0xbb,
    ];
    let lanes = crate::om::offset_store_counted_index_lanes(&bytes);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].offset, 1);
    assert_eq!(lanes[0].declared_count, 6);
    assert_eq!(lanes[0].anchor, 0x42);
    assert_eq!(lanes[0].raw_anchor, [0x42]);
    assert_eq!(lanes[0].anchor_offset, 3);
    assert_eq!(
        lanes[0].members,
        vec![(0x62, 4), (0x48, 5), (0x50, 7), (0x7c, 9)]
    );
    assert_eq!(
        lanes[0].raw_members,
        [vec![0x62], vec![0x80, 0x48], vec![0x80, 0x50], vec![0x7c]]
    );

    assert!(
        crate::om::offset_store_counted_index_lanes(&[0x01, 0x03, 0x42, 0xff, 0x01, 0x11,])
            .is_empty()
    );
    assert!(
        crate::om::offset_store_counted_index_lanes(&[0x01, 0x03, 0x42, 0x80, 0x01, 0x11,])
            .is_empty()
    );
    assert!(
        crate::om::offset_store_counted_index_lanes(&[0x01, 0x03, 0x42, 0x62, 0x01, 0x10,])
            .is_empty()
    );
}

#[test]
fn om_offset_store_abr_lane_requires_sixteen_slots_and_exact_terminator() {
    let mut bytes = vec![0xaa, 0x11];
    bytes.extend_from_slice(&[0xff; 6]);
    bytes.extend_from_slice(&[0x82, 0x83]);
    bytes.extend_from_slice(&[0xff; 9]);
    bytes.extend_from_slice(&[0x02, 0x11, b'A', b'B', b'R', 0xff, 0x03, 0xbb]);

    let lanes = crate::om::offset_store_abr_reference_lanes(&bytes);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].offset, 1);
    assert_eq!(lanes[0].slots.len(), 16);
    assert_eq!(lanes[0].slots[6], (Some(643), 8));
    assert_eq!(lanes[0].raw_slots[6], [0x82, 0x83]);
    assert!(lanes[0]
        .raw_slots
        .iter()
        .enumerate()
        .all(|(slot, raw)| slot == 6 || raw == &[0xff]));
    assert!(lanes[0]
        .slots
        .iter()
        .enumerate()
        .all(|(slot, (value, _))| slot == 6 || value.is_none()));

    bytes[23] = b'X';
    assert!(crate::om::offset_store_abr_reference_lanes(&bytes).is_empty());
    bytes[23] = b'R';
    bytes.remove(18);
    assert!(crate::om::offset_store_abr_reference_lanes(&bytes).is_empty());
}

#[test]
fn om_sketch_scalar_field_requires_exact_frame_and_finite_shifted_value() {
    let bytes = [
        0xaa, 0x50, 0x59, 0x66, 0x64, 0x00, 0x30, 0x43, 0x0c, 0xcc, 0xcc, 0xcc, 0xcd, 0x72, 0xbb,
    ];
    let fields = crate::om::construction_payload_scalar_fields(&bytes);
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].offset, 1);
    assert_eq!(fields[0].field_code, 0x64);
    assert!((fields[0].value - 38.1).abs() < 2.0e-12);

    let mut malformed = bytes;
    malformed[5] = 1;
    assert!(crate::om::construction_payload_scalar_fields(&malformed).is_empty());
    malformed = bytes;
    malformed[6] = 0x70;
    assert!(crate::om::construction_payload_scalar_fields(&malformed).is_empty());
}

#[test]
fn om_sketch_name_field_decodes_direct_and_extended_compact_type_codes() {
    let bytes = [
        0x66, 0x32, 0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'1', 0x00, 0xaa, 0x66, 0x80, 0x83,
        0x03, 0x07, b'L', b'i', b'n', b'e', b'2', 0x00,
    ];
    let fields = crate::om::construction_payload_named_fields(&bytes);
    assert_eq!(fields.len(), 2);
    assert_eq!(
        (fields[0].offset, fields[0].type_code, fields[0].value),
        (0, Some(0x32), "Point1")
    );
    assert_eq!(fields[0].raw_type_code, Some(vec![0x32]));
    assert_eq!(fields[0].type_code_offset, Some(1));
    assert_eq!(
        (fields[1].offset, fields[1].type_code, fields[1].value),
        (12, Some(0x83), "Line2")
    );
    assert_eq!(fields[1].raw_type_code, Some(vec![0x80, 0x83]));
    assert_eq!(fields[1].type_code_offset, Some(13));

    assert!(crate::om::construction_payload_named_fields(&[
        0x66, 0xff, 0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'1', 0x00,
    ])
    .is_empty());
    assert!(crate::om::construction_payload_named_fields(&[
        0x66, 0x32, 0x03, 0x08, b'P', b'o', b'i', b'n', b't',
    ])
    .is_empty());
}

#[test]
fn om_sketch_name_field_decodes_type_free_payload_leading_form() {
    let fields = crate::om::construction_payload_named_fields(&[
        0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'1', 0x00, 0x04,
    ]);
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].offset, 0);
    assert_eq!(fields[0].type_code, None);
    assert_eq!(fields[0].raw_type_code, None);
    assert_eq!(fields[0].type_code_offset, None);
    assert!(fields[0].payload_leading);
    assert_eq!(fields[0].value, "Point1");

    assert!(crate::om::construction_payload_named_fields(&[
        0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'1',
    ])
    .is_empty());
}

#[test]
fn om_offset_store_named_point_uses_minimal_consecutive_block_span() {
    let first = [
        0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'7', 0x00, 0x50, 0x59, 0x66, 0x58, 0x00, 0x30,
        0x4c, 0x93, 0x33, 0x33, 0x33, 0x33, 0x07,
    ];
    let second = [
        0x45, 0x04, 0x00, 0x50, 0x59, 0x66, 0x58, 0x00, 0x30, 0x4c, 0x93, 0x33, 0x33, 0x33, 0x33,
        0x07,
    ];
    let point = crate::om::offset_store_named_point(&[&first, &second]).unwrap();
    assert_eq!(point.name, "Point7");
    assert!(point
        .values
        .iter()
        .all(|value| (*value - 57.15).abs() < 1.0e-12));
    let expected_raw: [[u8; 8]; 2] = [
        first[14..22].try_into().unwrap(),
        second[8..16].try_into().unwrap(),
    ];
    assert_eq!(point.raw_values, expected_raw);
    assert_eq!(point.value_offsets, [9, first.len() + 3]);
    assert_eq!(point.block_count, 2);

    let mut same_block = first.to_vec();
    same_block.extend_from_slice(&second);
    assert_eq!(
        crate::om::offset_store_named_point(&[&same_block])
            .unwrap()
            .block_count,
        1
    );
    assert_eq!(
        crate::om::offset_store_named_point(&[&first[..9], &first[9..], &second])
            .unwrap()
            .block_count,
        3
    );
    let mut zero = first;
    zero[7] = b'0';
    assert!(crate::om::offset_store_named_point(&[&zero, &second]).is_none());
}

#[test]
fn sketch_fixed_pair_parser_reads_signed_q1_55_atoms() {
    let bytes = [
        0x04, 0xe0, 0x48, 0x0e, 0x02, 0x03, 0x80, 0x84, 0x30, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x30, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let pairs = crate::om::sketch_payload_fixed_pairs(&bytes);
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].values, [0.5, -0.5]);
    assert_eq!(pairs[0].value_offsets, [8, 17]);
    assert_eq!(pairs[0].raw_values[0], [0x40, 0, 0, 0, 0, 0, 0]);

    let mut malformed = bytes;
    malformed[16] = 1;
    assert!(crate::om::sketch_payload_fixed_pairs(&malformed).is_empty());
}

#[test]
fn datum_csys_fixed_pair_requires_its_exact_branch_discriminator() {
    let mut bytes = vec![
        0x0b, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00, 0x03,
        0x30,
    ];
    bytes.extend_from_slice(&[0x40, 0, 0, 0, 0, 0, 0]);
    bytes.extend_from_slice(&[0x00, 0x30]);
    bytes.extend_from_slice(&[0xc0, 0, 0, 0, 0, 0, 0]);
    let pairs = crate::om::datum_csys_payload_fixed_pairs(&bytes);
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].values, [0.5, -0.5]);
    assert_eq!(pairs[0].value_offsets, [15, 24]);
    assert_eq!(pairs[0].raw_values[0], [0x40, 0, 0, 0, 0, 0, 0]);

    bytes[0] = 0x08;
    assert!(crate::om::datum_csys_payload_fixed_pairs(&bytes).is_empty());
}

#[test]
fn om_datum_csys_scalar_field_uses_the_common_shifted_binary64_frame() {
    let mut shifted = 25.4_f64.to_be_bytes();
    shifted[0] -= 0x10;
    let mut payload = vec![0xaa, 0x50, 0x59, 0x66, 0x64, 0x00];
    payload.extend_from_slice(&shifted);
    payload.push(0xbb);

    let fields = crate::om::construction_payload_scalar_fields(&payload);
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].offset, 1);
    assert_eq!(fields[0].field_code, 0x64);
    assert_eq!(fields[0].value, 25.4);
    assert_eq!(fields[0].raw_value, shifted);
}

#[test]
fn om_simple_hole_lane_requires_two_identical_nonempty_scalar_runs() {
    let shifted = |value: f64| {
        let mut bytes = value.to_be_bytes();
        bytes[0] -= 0x10;
        bytes
    };
    let mut payload = Vec::new();
    for value in [508.0, 38.1, 508.0, 38.1] {
        payload.extend_from_slice(&shifted(value));
        payload.push(0x7f);
    }
    payload.extend_from_slice(&[0x04, 0x08]);
    payload.extend_from_slice(b"Hole_X");
    payload.push(0x00);
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 120,
        value: "SIMPLE HOLE",
        object_indices: [None; 4],
        object_index_offsets: [0; 4],
    };
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: &payload,
        payload_offset: 200,
        payload: &payload,
        label,
    };
    let lane = crate::om::simple_hole_repeated_scalar_lane(record).unwrap();
    assert_eq!(lane.values[0], 508.0);
    assert!((lane.values[1] - 38.1).abs() < 2.0e-12);
    assert_eq!(lane.raw_values, [shifted(508.0), shifted(38.1)]);
    assert_eq!(lane.witness_offsets, [vec![200, 209], vec![218, 227]]);

    let mut mismatched = payload.clone();
    mismatched[18 + 7] ^= 1;
    assert!(
        crate::om::simple_hole_repeated_scalar_lane(crate::om::OperationRecord {
            bytes: &mismatched,
            payload: &mismatched,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_simple_hole_lane_accepts_one_repeated_scalar() {
    let mut scalar = 25.4f64.to_be_bytes();
    scalar[0] -= 0x10;
    let mut payload = scalar.to_vec();
    payload.push(0x7f);
    payload.extend_from_slice(&scalar);
    payload.extend_from_slice(&[0x04, 0x08]);
    payload.extend_from_slice(b"Hole_X\0");
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: &payload,
        payload_offset: 200,
        payload: &payload,
        label: crate::om::OperationLabel {
            header_offset: 100,
            offset: 120,
            value: "SIMPLE HOLE",
            object_indices: [None; 4],
            object_index_offsets: [0; 4],
        },
    };
    let lane = crate::om::simple_hole_repeated_scalar_lane(record).unwrap();
    assert_eq!(lane.values, [25.4]);
    assert_eq!(lane.raw_values, [scalar]);
    assert_eq!(lane.witness_offsets, [vec![200], vec![209]]);
}

#[test]
fn om_simple_hole_lane_block_references_follow_both_scalar_runs() {
    let shifted = |value: f64| {
        let mut bytes = value.to_be_bytes();
        bytes[0] -= 0x10;
        bytes
    };
    let mut payload = Vec::new();
    payload.extend_from_slice(&shifted(508.0));
    payload.extend_from_slice(&shifted(38.1));
    payload.extend_from_slice(&[0xf0, 0xe7, 0xf0, 0xe8]);
    payload.extend_from_slice(&shifted(508.0));
    payload.extend_from_slice(&shifted(38.1));
    payload.extend_from_slice(&[0xf0, 0xe9, 0xf0, 0xea]);
    payload.extend_from_slice(&[0x04, 0x08]);
    payload.extend_from_slice(b"Hole_X");
    payload.push(0x00);
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 120,
        value: "SIMPLE HOLE",
        object_indices: [None; 4],
        object_index_offsets: [0; 4],
    };
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: &payload,
        payload_offset: 200,
        payload: &payload,
        label,
    };
    let references = crate::om::simple_hole_repeated_scalar_lane_block_references(record).unwrap();
    assert_eq!(references.first, [231, 232]);
    assert_eq!(references.second, [233, 234]);
    assert_eq!(references.offsets, [[216, 218], [236, 238]]);

    let mut null = payload.clone();
    null[16] = 0xff;
    assert!(
        crate::om::simple_hole_repeated_scalar_lane_block_references(crate::om::OperationRecord {
            bytes: &null,
            payload: &null,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_datum_csys_reference_lane_requires_eight_canonical_indices() {
    let mut payload = vec![
        0x13, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
    ];
    for value in 42..50 {
        payload.extend_from_slice(&[0xf0, value]);
    }
    payload.extend_from_slice(&[0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00]);
    let label = crate::om::OperationLabel {
        header_offset: 10,
        offset: 20,
        value: "DATUM_CSYS",
        object_indices: [None; 4],
        object_index_offsets: [0; 4],
    };
    let record = crate::om::OperationRecord {
        offset: 10,
        bytes: &payload,
        payload_offset: 100,
        payload: &payload,
        label,
    };
    let field = crate::om::datum_csys_references(record).unwrap();
    assert_eq!(field.control, 0x13);
    assert_eq!(
        field
            .references
            .each_ref()
            .map(|reference| reference.object_index),
        [42, 43, 44, 45, 46, 47, 48, 49]
    );
    assert_eq!(
        field
            .references
            .each_ref()
            .map(|reference| reference.offset),
        [114, 116, 118, 120, 122, 124, 126, 128]
    );
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| reference.raw_object_index.clone())
            .collect::<Vec<_>>(),
        (42..50).map(|value| vec![0xf0, value]).collect::<Vec<_>>()
    );

    let mut alternate_control = payload.clone();
    alternate_control[0] = 0x1a;
    assert_eq!(
        crate::om::datum_csys_references(crate::om::OperationRecord {
            bytes: &alternate_control,
            payload: &alternate_control,
            ..record
        })
        .unwrap()
        .control,
        0x1a
    );

    let mut malformed = payload.clone();
    malformed[14] = 0x2a;
    assert!(
        crate::om::datum_csys_references(crate::om::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_datum_plane_header_requires_common_prefix_and_nontrivial_count() {
    let payload = [
        0x22, 0x00, 0x00, 0x01, 0x00, 0x01, 0x03, 0x29, 0x01, 0x02, 0xf1, 0x02, 0xcf,
    ];
    let label = crate::om::OperationLabel {
        header_offset: 10,
        offset: 20,
        value: "DATUM_PLANE",
        object_indices: [None; 4],
        object_index_offsets: [0; 4],
    };
    let record = crate::om::OperationRecord {
        offset: 10,
        bytes: &payload,
        payload_offset: 100,
        payload: &payload,
        label,
    };
    assert_eq!(
        crate::om::datum_plane_payload_header(record),
        Some(crate::om::DatumPlanePayloadHeader {
            control: 0x22,
            declared_count: 3,
            branch_tag: 0x29,
        })
    );
    let mut malformed = payload;
    malformed[6] = 1;
    assert!(
        crate::om::datum_plane_payload_header(crate::om::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );

    let branch_payload = [
        0x22, 0x00, 0x00, 0x01, 0x00, 0x01, 0x02, 0x23, 0x01, 0x02, 0x80, 0x4c, 0x01, 0xf1, 0x02,
        0xbb, 0x00, 0x14, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x00,
    ];
    let branch = crate::om::datum_plane_single_reference_branch(crate::om::OperationRecord {
        bytes: &branch_payload,
        payload: &branch_payload,
        ..record
    })
    .unwrap();
    assert_eq!(branch.descriptor_index, 76);
    assert_eq!(branch.raw_descriptor_index, [0x80, 0x4c]);
    assert_eq!(branch.descriptor_offset, 110);
    assert_eq!(branch.object_index, 699);
    assert_eq!(branch.raw_object_index, [0xf1, 0x02, 0xbb]);
    assert_eq!(branch.object_offset, 113);

    let double_payload = [
        0x22, 0x00, 0x00, 0x01, 0x00, 0x01, 0x02, 0x29, 0x01, 0x02, 0xf1, 0x02, 0x77, 0x01, 0x01,
        0x18, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0xff, 0xf1, 0x02, 0x78, 0x01, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x0d,
    ];
    let double = crate::om::datum_plane_double_reference_branch(crate::om::OperationRecord {
        bytes: &double_payload,
        payload: &double_payload,
        ..record
    })
    .unwrap();
    assert_eq!(
        double
            .references
            .each_ref()
            .map(|reference| reference.object_index),
        [631, 632]
    );
    assert_eq!(
        double
            .references
            .each_ref()
            .map(|reference| reference.offset),
        [110, 124]
    );

    let count_three_payload = [
        0x22, 0x00, 0x00, 0x01, 0x00, 0x01, 0x03, 0x29, 0x01, 0x02, 0xf1, 0x02, 0xcf, 0x01, 0x01,
        0x3a, 0x01, 0x02, 0xf1, 0x02, 0xd0, 0x01, 0x17, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
        0xff, 0xff, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0d,
    ];
    let count_three = crate::om::datum_plane_double_reference_branch(crate::om::OperationRecord {
        bytes: &count_three_payload,
        payload: &count_three_payload,
        ..record
    })
    .unwrap();
    assert_eq!(
        count_three
            .references
            .each_ref()
            .map(|reference| reference.object_index),
        [719, 720]
    );
    assert_eq!(
        count_three
            .references
            .each_ref()
            .map(|reference| reference.offset),
        [110, 118]
    );

    let descriptor_count_three_payload = [
        0x22, 0x00, 0x00, 0x01, 0x00, 0x01, 0x03, 0x28, 0x01, 0x02, 0x80, 0x4d, 0x01, 0x29, 0x01,
        0x02, 0xf1, 0x02, 0xd1, 0x01, 0x01, 0x07, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff,
        0xff, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0d,
    ];
    let descriptor_count_three =
        crate::om::datum_plane_descriptor_reference_branch(crate::om::OperationRecord {
            bytes: &descriptor_count_three_payload,
            payload: &descriptor_count_three_payload,
            ..record
        })
        .unwrap();
    assert_eq!(descriptor_count_three.descriptor_index, 77);
    assert_eq!(descriptor_count_three.raw_descriptor_index, [0x80, 0x4d]);
    assert_eq!(descriptor_count_three.descriptor_offset, 110);
    assert_eq!(descriptor_count_three.object_index, 721);
    assert_eq!(descriptor_count_three.object_offset, 116);
}

#[test]
fn om_datum_plane_object_index_lane_ends_at_logical_payload_boundary() {
    let bytes = [
        0x80, 0xab, 0x01, 0x04, 0x81, 0x01, 0x01, 0x01, 0x00, 0x12, 0x34, 0x56, 0x78,
    ];
    let lanes = crate::om::datum_plane_object_index_lanes(&bytes);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].offset, 2);
    assert_eq!(lanes[0].declared_count, 4);
    assert_eq!(lanes[0].indices, [(257, 4), (1, 6), (1, 7)]);
    assert_eq!(lanes[0].raw_indices, [vec![0x81, 0x01], vec![1], vec![1]]);
    assert_eq!(lanes[0].trailer, 0x1234_5678);

    let mut trailing = bytes.to_vec();
    trailing.push(0);
    assert!(crate::om::datum_plane_object_index_lanes(&trailing).is_empty());
}

#[test]
fn om_datum_plane_object_scalar_pairs_require_the_complete_discriminator() {
    let mut bytes = vec![0x7f, 0x01, 0x01, 0xff];
    bytes.extend_from_slice(&[
        0x6d, 0x00, 0xf0, 0x08, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86,
        0x02, 0x00, 0x03,
    ]);
    bytes.extend_from_slice(&[0x30, 0x24, 0, 0, 0, 0, 0, 0]);
    bytes.push(0);
    bytes.extend_from_slice(&[0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
    let pairs = crate::om::datum_plane_object_scalar_pairs(&bytes);
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].offset, 4);
    assert_eq!(pairs[0].value_offsets, [22, 31]);
    assert_eq!(pairs[0].values, [10.0, -20.0]);
    assert_eq!(pairs[0].raw_values[0], [0x30, 0x24, 0, 0, 0, 0, 0, 0]);
    assert_eq!(pairs[0].raw_values[1], [0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
    bytes[10] ^= 1;
    assert!(crate::om::datum_plane_object_scalar_pairs(&bytes).is_empty());
}

#[test]
fn om_datum_plane_descriptor_requires_complete_lowercase_hex_identity() {
    let mut bytes = *b"793487222121a5474a9125451b8e31f5?A\xf0\x1e\xff\x02\x01\x33";
    let descriptor = crate::om::datum_plane_descriptor_block(&bytes).unwrap();
    assert_eq!(descriptor.identity, "793487222121a5474a9125451b8e31f5");
    assert_eq!(descriptor.suffix, b"?A\xf0\x1e\xff\x02\x01\x33");
    assert_eq!(descriptor.schema_index, 28_702);
    assert_eq!(descriptor.label, "3");

    let short_bytes = *b"a75c5f0ed880dd1443b3c5c57908aae?A\xf0\x1f\xff\x02\x01\x66\x33";
    let short = crate::om::datum_plane_descriptor_block(&short_bytes).unwrap();
    assert_eq!(short.identity.len(), 31);
    assert_eq!(short.schema_index, 28_703);
    assert_eq!(short.label, "f3");

    bytes[0] = b'G';
    assert!(crate::om::datum_plane_descriptor_block(&bytes).is_none());
    assert!(crate::om::datum_plane_descriptor_block(&bytes[..39]).is_none());
}

#[test]
fn om_datum_csys_scalar_pairs_require_discriminator_and_separator() {
    let mut bytes = vec![0x2f, 0x2f, 0x41, 0x6d, 0x00, 0xf0];
    bytes.extend_from_slice(&[
        0x08, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00, 0x03,
    ]);
    bytes.extend_from_slice(&[0x30, 0x24, 0, 0, 0, 0, 0, 0]);
    bytes.push(0);
    bytes.extend_from_slice(&[0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
    let pairs = crate::om::object_payload_scalar_pairs(&bytes);
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].offset, 6);
    assert_eq!(pairs[0].value_offsets, [21, 30]);
    assert_eq!(pairs[0].values, [10.0, -20.0]);
    assert_eq!(pairs[0].raw_values[0], [0x30, 0x24, 0, 0, 0, 0, 0, 0]);
    assert_eq!(pairs[0].raw_values[1], [0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
    assert_eq!(pairs[0].discriminator.len(), 15);

    let mut extended = vec![
        0x08, 0x02, 0x03, 0x01, 0x81, 0x02, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00,
        0x03,
    ];
    extended.extend_from_slice(&[0x30, 0x24, 0, 0, 0, 0, 0, 0]);
    extended.push(0);
    extended.extend_from_slice(&[0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
    let extended_pairs = crate::om::object_payload_scalar_pairs(&extended);
    assert_eq!(extended_pairs.len(), 1);
    assert_eq!(extended_pairs[0].discriminator.len(), 16);
    assert_eq!(extended_pairs[0].value_offsets, [16, 25]);
    assert_eq!(
        extended_pairs[0].raw_values[0],
        [0x30, 0x24, 0, 0, 0, 0, 0, 0]
    );

    bytes[29] = 1;
    assert!(crate::om::object_payload_scalar_pairs(&bytes).is_empty());
}

#[test]
fn om_datum_csys_descriptor_requires_one_maximal_hex_identity() {
    let bytes = b"\x02\x01ae166162820ea2d993e1fdf49091850e?A\x80\xa0\xf0\x26";
    let descriptor = crate::om::datum_csys_descriptor_block(bytes).unwrap();
    assert_eq!(descriptor.prefix, [0x02, 0x01]);
    assert_eq!(descriptor.identity, "ae166162820ea2d993e1fdf49091850e");
    assert_eq!(descriptor.identity_offset, 2);
    assert_eq!(descriptor.suffix, b"?A\x80\xa0\xf0\x26");

    let mut ambiguous = bytes.to_vec();
    ambiguous.extend_from_slice(b"012345678901234567890123456789");
    assert!(crate::om::datum_csys_descriptor_block(&ambiguous).is_none());
}

#[test]
fn om_draft_identity_frames_require_complete_typed_framing() {
    let bytes = b"\x00A\x81\x54\xf0\x38\x02\x01abc123?A\xf0\x27\xff\x02\x01def456?\x00";
    let frames = crate::om::draft_construction_identity_frames(bytes);
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0].offset, 1);
    assert_eq!(frames[0].prefix, b"A\x81\x54\xf0\x38\x02\x01");
    assert_eq!(
        frames[0].form,
        crate::om::DraftConstructionIdentityFrameForm::IndexedBranch {
            first_index: 340,
            second_index: Some(56),
            branch: 2,
        }
    );
    assert_eq!(frames[0].identity, "abc123");
    assert_eq!(frames[0].identity_offset, 8);
    assert_eq!(frames[1].offset, 15);
    assert_eq!(frames[1].prefix, b"A\xf0\x27\xff\x02\x01");
    assert_eq!(
        frames[1].form,
        crate::om::DraftConstructionIdentityFrameForm::Tagged { index: Some(39) }
    );
    assert_eq!(frames[1].identity, "def456");

    assert!(
        crate::om::draft_construction_identity_frames(b"A\x81\x54\xf0\x38\x02\x01abc123")
            .is_empty()
    );
    assert!(
        crate::om::draft_construction_identity_frames(b"A\x81\x54\xf0\x38\x04\x01abc123?")
            .is_empty()
    );
    assert!(
        crate::om::draft_construction_identity_frames(b"A\xf0\x27\xff\x02\x01ABC123?").is_empty()
    );
}

#[test]
fn om_draft_fixed_lanes_require_complete_discriminator_atoms_and_terminator() {
    let discriminator = [
        0x25, 0x25, 0x41, 0x00, 0x04, 0x01, 0x07, 0x01, 0xc0, 0x45, 0x10, 0x00, 0x80, 0x86, 0x02,
        0x00, 0x01, 0x00,
    ];
    let mut bytes = vec![0xff];
    bytes.extend_from_slice(&discriminator);
    bytes.extend_from_slice(&[0x30, 0x40, 0, 0, 0, 0, 0, 0]);
    bytes.extend_from_slice(&[0xb0, 0xc0, 0, 0, 0, 0, 0, 0]);
    bytes.push(0);
    let lanes = crate::om::draft_construction_fixed_lanes(&bytes);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].offset, 1);
    assert_eq!(lanes[0].values, [0.5, -0.5]);
    assert_eq!(lanes[0].markers, [0x30, 0xb0]);
    assert_eq!(lanes[0].value_offsets, [19, 27]);

    bytes.pop();
    assert!(crate::om::draft_construction_fixed_lanes(&bytes).is_empty());
    bytes.truncate(22);
    assert!(crate::om::draft_construction_fixed_lanes(&bytes).is_empty());
    assert!(crate::om::draft_construction_fixed_lanes(&discriminator).is_empty());
}

#[test]
fn om_draft_binary32_lanes_require_complete_typed_atoms_and_terminator() {
    let discriminator = [
        0x90, 0x18, 0x45, 0x01, 0x04, 0x01, 0x04, 0x01, 0xc0, 0x45, 0x04, 0x04, 0x80, 0x86, 0x02,
        0x00, 0x03, 0x00,
    ];
    let mut bytes = vec![0xff];
    bytes.extend_from_slice(&discriminator);
    bytes.extend_from_slice(&[0x4f, 0x80, 0, 0]);
    bytes.extend_from_slice(&[0xcf, 0x80, 0, 0]);
    bytes.push(0);
    let lanes = crate::om::draft_construction_binary32_lanes(&bytes);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].offset, 1);
    assert_eq!(lanes[0].discriminator, discriminator);
    assert_eq!(lanes[0].branch, 4);
    assert_eq!(lanes[0].values, [1.0, -1.0]);
    assert_eq!(lanes[0].value_offsets, [19, 23]);

    bytes.pop();
    assert!(crate::om::draft_construction_binary32_lanes(&bytes).is_empty());
    bytes.truncate(21);
    assert!(crate::om::draft_construction_binary32_lanes(&bytes).is_empty());
    assert!(crate::om::draft_construction_binary32_lanes(&discriminator).is_empty());
}

#[test]
fn om_operation_primary_body_reference_requires_one_complete_field() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 100,
        value: "EXTRUDE",
        object_indices: [None; 4],
        object_index_offsets: [0; 4],
    };
    let bytes = [0x01, 0x02, 0x10, 0x90, 0x19, 0x42, 0xff];
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: &bytes,
        payload_offset: 100,
        payload: &bytes,
        label,
    };
    assert_eq!(
        crate::om::operation_body_reference(record),
        Some(crate::om::OperationBodyReference {
            offset: 103,
            object_index: 6466,
            raw_object_index: vec![0x90, 0x19, 0x42],
        })
    );

    let duplicate = [bytes.as_slice(), bytes.as_slice()].concat();
    assert_eq!(
        crate::om::operation_body_references(crate::om::OperationRecord {
            offset: 100,
            bytes: &duplicate,
            payload_offset: 100,
            payload: &duplicate,
            label,
        }),
        [
            crate::om::OperationBodyReference {
                offset: 103,
                object_index: 6466,
                raw_object_index: vec![0x90, 0x19, 0x42],
            },
            crate::om::OperationBodyReference {
                offset: 110,
                object_index: 6466,
                raw_object_index: vec![0x90, 0x19, 0x42],
            },
        ]
    );
    assert!(
        crate::om::operation_body_reference(crate::om::OperationRecord {
            offset: 100,
            bytes: &duplicate,
            payload_offset: 100,
            payload: &duplicate,
            label,
        })
        .is_none()
    );
}

#[test]
fn om_data_block_object_references_require_complete_field_frames() {
    let bytes = [
        0x04, 0x00, 0x2a, 0x02, 0x0b, 0xff, 0x04, 0x00, 0x80, 0xc9, 0x02, 0x0b, 0x04, 0x00, 0x90,
        0x19, 0x42, 0x02, 0x0b,
    ];
    assert_eq!(
        crate::om::data_block_object_references(&bytes),
        [
            crate::om::DataBlockObjectReference {
                offset: 2,
                object_index: 42,
                raw_object_index: vec![0x2a],
            },
            crate::om::DataBlockObjectReference {
                offset: 8,
                object_index: 201,
                raw_object_index: vec![0x80, 0xc9],
            },
            crate::om::DataBlockObjectReference {
                offset: 14,
                object_index: 6466,
                raw_object_index: vec![0x90, 0x19, 0x42],
            },
        ]
    );
    assert_eq!(
        crate::om::data_block_object_references(&bytes[..bytes.len() - 1]).len(),
        2
    );
}

#[test]
fn om_size_frame_bounds_its_type_declarations() {
    let bytes = size_framed_om_section();
    let sections = crate::om::sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].offset, 0);
    assert_eq!(sections[0].byte_len, bytes.len());
    assert_eq!(sections[0].types.len(), 2);
    assert_eq!(sections[0].types[0].name, "UGS::FEATURE_RECORD");
    assert_eq!(
        sections[0].types[0].registry_suffix,
        &[0x81, 0x21, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x06]
    );
    assert_eq!(sections[0].types[1].trailing_code, 0x65);
    assert_eq!(sections[0].fields.len(), 2);
    assert_eq!(sections[0].fields[0].name, "m_target");
    assert_eq!(sections[0].fields[1].trailing_code, 0x81);
    assert_eq!(sections[0].record_area, None);

    let mut truncated = bytes;
    truncated.pop();
    assert!(crate::om::sections(&truncated).is_empty());
}

#[test]
fn om_size_frame_uses_validated_internal_record_area_pointer() {
    let bytes = size_framed_om_section_with_record_area();
    let section = crate::om::sections(&bytes).remove(0);
    let offset = section.record_area_offset.expect("record area");
    assert_eq!(offset, size_framed_om_section().len() + 20);
    assert_eq!(section.record_area.unwrap(), &bytes[offset..]);
    assert_eq!(&bytes[offset + 12..offset + 15], &[0x05, 0x01, 0x0e]);

    let mut invalid = bytes;
    invalid[offset + 12] = 1;
    assert_eq!(crate::om::sections(&invalid)[0].record_area, None);
}

#[test]
fn om_operation_labels_require_the_complete_frame() {
    let bytes = b"\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\x01\x82\x40\x90\x17\xd3\xff\x03\x07UNITE\0\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\x02\x03\xff\xff\x03\x08SKETCH\0";
    let labels = crate::om::operation_labels(bytes, 100);
    assert_eq!(labels.len(), 2);
    assert_eq!(labels[0].offset, 122);
    assert_eq!(labels[0].header_offset, 100);
    assert_eq!(labels[0].value, "UNITE");
    assert_eq!(
        labels[0].object_indices,
        [Some(1), Some(576), Some(6099), None]
    );
    assert_eq!(labels[1].value, "SKETCH");
    assert_eq!(labels[1].object_indices, [Some(2), Some(3), None, None]);

    assert!(crate::om::operation_labels(b"\xff\xff\x03\x07UNITE\0", 0).is_empty());
    let mut invalid = bytes.to_vec();
    invalid[15] = 0x91;
    assert_eq!(crate::om::operation_labels(&invalid, 0).len(), 1);
}

#[test]
fn om_operation_records_use_consecutive_validated_headers() {
    let bytes = b"prefix\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\xff\xff\xff\xff\x03\x07UNITE\0payload\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\xff\xff\xff\xff\x03\x08SKETCH\0tail";
    let records = crate::om::operation_records(bytes, 10);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].offset, 16);
    assert_eq!(records[0].label.value, "UNITE");
    assert!(records[0].bytes.ends_with(b"payload"));
    assert_eq!(records[0].payload, b"payload");
    assert_eq!(records[0].payload_offset, 43);
    assert_eq!(records[1].label.value, "SKETCH");
    assert!(records[1].bytes.ends_with(b"tail"));
    assert_eq!(records[1].payload, b"tail");
}

#[test]
fn om_operation_payload_strings_require_complete_utf8_frames() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "SIMPLE HOLE",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x00\x04\x07BLOCK\0\x04\x04\xc3\x97\0\x04\x07BROKEN";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let strings = crate::om::operation_payload_strings(record);
    assert_eq!(strings.len(), 2);
    assert_eq!(strings[0].offset, 201);
    assert_eq!(strings[0].value, "BLOCK");
    assert_eq!(strings[1].value, "×");
}

#[test]
fn om_surface_payload_strings_require_exact_length_utf8_and_terminator() {
    let bytes = b"\x66\x1b\x03\x05Steel\0\xaa\x66\x1b\x03\x02\xc3\x97\0";
    let strings = crate::om::surface_payload_strings(bytes);
    assert_eq!(strings.len(), 2);
    assert_eq!(strings[0].offset, 0);
    assert_eq!(strings[0].value, "Steel");
    assert_eq!(strings[1].offset, 11);
    assert_eq!(strings[1].value, "×");

    let truncated = b"\x66\x1b\x03\x05Steel";
    assert!(crate::om::surface_payload_strings(truncated).is_empty());
    let invalid_utf8 = b"\x66\x1b\x03\x01\xff\0";
    assert!(crate::om::surface_payload_strings(invalid_utf8).is_empty());
    let control = b"\x66\x1b\x03\x01\n\0";
    assert!(crate::om::surface_payload_strings(control).is_empty());
}

#[test]
fn om_projected_curve_references_require_one_complete_field() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "CPROJ",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload =
        b"\0\x01\x02\xf1\x02\xc8\xf1\x02\xc9\x80\x57\x00\x02\x01\xf1\x02\xca\xff\x01\x02\x02\x7d\0";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let field = crate::om::projected_curve_payload_references(record).expect("complete field");
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| (reference.object_index, reference.offset))
            .collect::<Vec<_>>(),
        [(712, 203), (713, 206), (714, 214)]
    );

    let mut malformed = payload.to_vec();
    malformed[17] = 0x00;
    assert!(
        crate::om::projected_curve_payload_references(crate::om::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );

    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(
        crate::om::projected_curve_payload_references(crate::om::OperationRecord {
            bytes: &ambiguous,
            payload: &ambiguous,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_combined_projected_curve_references_require_the_complete_graph() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "CPROJ_CMB",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x3c\x32\x01\x02\x32\x01\x04\x36\x01\x33\xf1\x03\x18\x33\xf1\x03\x19\x00\xf1\x03\x1a\x00\x00\x00\x00\x00\x00\xf1\x03\x1b\x16\x01\x02\xf1\x03\x18\x01\x02\x00\x00\x00\x00\x00\xff\x01\x02\xf1\x03\x1c\x00\x81\x5c\x16\x01\x02\xf1\x03\x19\x01\x02\x00\x00\x00\x00\x00\xff\x01\x02\xf1\x03\x1d\x00\x81\x5c\xff\x01\xff\x01\xf1\x03\x1e\xf1\x03\x1f\x04\x02";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let field = crate::om::projected_curve_payload_references(record).expect("complete graph");
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| (reference.object_index, reference.offset))
            .collect::<Vec<_>>(),
        [
            (792, 210),
            (793, 214),
            (794, 218),
            (795, 227),
            (796, 246),
            (797, 268),
            (798, 278),
            (799, 281),
        ]
    );

    let mut inconsistent = payload.to_vec();
    inconsistent[35] = 0x19;
    assert!(
        crate::om::projected_curve_payload_references(crate::om::OperationRecord {
            bytes: &inconsistent,
            payload: &inconsistent,
            ..record
        })
        .is_none()
    );

    let mut malformed = payload.to_vec();
    malformed[84] = 0x00;
    assert!(
        crate::om::projected_curve_payload_references(crate::om::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );

    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(
        crate::om::projected_curve_payload_references(crate::om::OperationRecord {
            bytes: &ambiguous,
            payload: &ambiguous,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_pattern_reference_graph_preserves_nullable_terminal_slot() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "Pattern Geometry",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let nullable = b"\x61\xf1\x1b\x08\xff\x00\xff\x01\xf1\x1b\x09\xf1\x1b\x0a\x61\xf1\x1b\x0b\xff\x00\xff\x01\xf1\x1b\x0c\xf1\x1b\x0d\xff\x62\xf1\x1b\x0e\xf1\x1b\x0f\xff\x00\x00\x01\xf1\x1b\x10\xff\xff\xff\x01";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: nullable,
        payload_offset: 200,
        payload: nullable,
        label,
    };
    let field = crate::om::pattern_payload_references(record).expect("complete graph");
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| reference.object_index)
            .collect::<Vec<_>>(),
        (6920..=6928).collect::<Vec<_>>()
    );

    let populated = [&nullable[..nullable.len() - 4], b"\xf1\x1b\x11\xff\xff\x01"].concat();
    let field = crate::om::pattern_payload_references(crate::om::OperationRecord {
        label: crate::om::OperationLabel {
            value: "Pattern Feature",
            ..label
        },
        bytes: &populated,
        payload: &populated,
        ..record
    })
    .expect("populated terminal slot");
    assert_eq!(field.references.len(), 10);
    assert_eq!(field.references[9].object_index, 6929);

    let mut malformed = nullable.to_vec();
    malformed[18] = 0x60;
    assert!(
        crate::om::pattern_payload_references(crate::om::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_pattern_transform_lanes_require_counted_family_rows() {
    let feature_payload = b"\xaa\x01\x03\x60\x01\x00\x00\x50\x54\x00\x00\x00\x01\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x02\x01\x01\x00\x00\xff\x00\x00\x60\x01\x00\x00\xd0\x54\x00\x00\x00\x01\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x9f\xfe\x01\x02\x00\x00\xff\x00\x00\x5f\x00\x00\x01";
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "Pattern Feature",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: feature_payload,
        payload_offset: 200,
        payload: feature_payload,
        label,
    };
    let lane = crate::om::pattern_payload_transform_lane(record).expect("feature lane");
    assert_eq!(lane.offset, 201);
    assert_eq!(lane.declared_count, 3);
    assert_eq!(lane.encoding, crate::om::PatternTransformEncoding::Binary32);
    assert_eq!(lane.values, [3.3125, -3.3125]);
    assert_eq!(lane.value_offsets, [207, 237]);
    assert_eq!(lane.selectors, [2, 8190]);
    assert_eq!(lane.raw_selectors, [vec![0x02], vec![0x9f, 0xfe]]);
    assert_eq!(lane.selector_offsets, [225, 255]);

    let geometry_payload = b"\x01\x03\x60\x01\x00\x00\x00\x00\x01\x00\x30\x60\x80\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x02\x01\x01\x00\x00\xff\x00\x00\x60\x01\x00\x00\x00\x00\x01\x00\x30\x70\x80\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x03\x01\x02\x00\x00\xff\x00\x00\x5f\x00\x00\x01";
    let geometry_record = crate::om::OperationRecord {
        label: crate::om::OperationLabel {
            value: "Pattern Geometry",
            ..label
        },
        bytes: geometry_payload,
        payload: geometry_payload,
        ..record
    };
    let lane = crate::om::pattern_payload_transform_lane(geometry_record).expect("geometry lane");
    assert_eq!(lane.encoding, crate::om::PatternTransformEncoding::Binary64);
    assert_eq!(lane.values, [132.0, 264.0]);
    assert_eq!(lane.selectors, [2, 3]);
    assert_eq!(lane.raw_selectors, [vec![0x02], vec![0x03]]);
    assert_eq!(lane.selector_offsets, [228, 262]);

    let mut wrong_ordinal = feature_payload.to_vec();
    wrong_ordinal[29] = 2;
    assert!(
        crate::om::pattern_payload_transform_lane(crate::om::OperationRecord {
            bytes: &wrong_ordinal,
            payload: &wrong_ordinal,
            ..record
        })
        .is_none()
    );
    assert!(
        crate::om::pattern_payload_transform_lane(crate::om::OperationRecord {
            bytes: &feature_payload[..feature_payload.len() - 1],
            payload: &feature_payload[..feature_payload.len() - 1],
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_geometry_instance_reference_requires_one_complete_field() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "Geometry Instance",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x44\x45\x00\xff\xff\xf1\x03\x21\x01\x02\x00\xff\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00\x00\x01\x02";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let field = crate::om::pattern_payload_references(record).expect("complete field");
    assert_eq!(field.references[0].object_index, 801);
    assert_eq!(field.references[0].offset, 205);

    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(
        crate::om::pattern_payload_references(crate::om::OperationRecord {
            bytes: &ambiguous,
            payload: &ambiguous,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_point_feature_header_requires_the_complete_leading_envelope() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "POINT",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x72\x00\x00\x01\x00\x00\x00\xf1\x1c\x8f\x00\xff\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x0d\x01\x02\x01\x00\x00\x00\x89\x02\x01\x01\x01\x00\xa5\x57\x95\x01\x00\x00\xff\x02\xc0\x1f\xff\xfd\x01\x00\x00\x01\x01\x01\x03\x02\x01\x01\x01\x00\x00\x00\x00\x00\xaa";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let header = crate::om::point_feature_payload_header(record).expect("complete header");
    assert_eq!(header.reference.object_index, 7311);
    assert_eq!(header.reference.offset, 207);
    assert_eq!(header.mode, 0x02);

    let mut alternate_mode = payload.to_vec();
    alternate_mode[52] = 0x03;
    assert_eq!(
        crate::om::point_feature_payload_header(crate::om::OperationRecord {
            bytes: &alternate_mode,
            payload: &alternate_mode,
            ..record
        })
        .expect("alternate mode")
        .mode,
        0x03
    );

    for malformed_offset in [0, 10, 51, 72] {
        let mut malformed = payload.to_vec();
        malformed[malformed_offset] ^= 0x01;
        assert!(
            crate::om::point_feature_payload_header(crate::om::OperationRecord {
                bytes: &malformed,
                payload: &malformed,
                ..record
            })
            .is_none()
        );
    }
    let mut unsupported_mode = payload.to_vec();
    unsupported_mode[52] = 0x04;
    assert!(
        crate::om::point_feature_payload_header(crate::om::OperationRecord {
            bytes: &unsupported_mode,
            payload: &unsupported_mode,
            ..record
        })
        .is_none()
    );
    assert!(
        crate::om::point_feature_payload_header(crate::om::OperationRecord {
            bytes: &payload[..72],
            payload: &payload[..72],
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_point_feature_scalar_lane_spans_the_preceding_block_atomically() {
    let mut encoded = Vec::new();
    for value in [1.0_f64, -2.0, 3.5, 4.0, 5.25, -6.0] {
        let mut bytes = value.to_be_bytes();
        bytes[0] -= 0x10;
        encoded.extend_from_slice(&bytes);
    }
    let preceding = [vec![0xaa, 0xbb], encoded[..3].to_vec()].concat();
    let mut target = encoded[3..].to_vec();
    target.extend_from_slice(&[
        0x00, 0x25, 0x25, 0x41, 0x00, 0x04, 0x01, 0x07, 0x01, 0xc0, 0x45, 0x10, 0x00, 0x80, 0x86,
        0x02, 0x00, 0x01, 0x00,
    ]);
    target.push(0xcc);

    let lane = crate::om::point_feature_scalar_lane(&preceding, &target).expect("complete lane");
    assert_eq!(lane.values, [1.0, -2.0, 3.5, 4.0, 5.25, -6.0]);
    assert_eq!(lane.raw_values.concat(), encoded);
    assert_eq!(lane.value_offsets, [2, 10, 18, 26, 34, 42]);

    let mut malformed = target.clone();
    malformed[45] = 0x01;
    assert!(crate::om::point_feature_scalar_lane(&preceding, &malformed).is_none());
    assert!(crate::om::point_feature_scalar_lane(&preceding[..2], &target).is_none());
    assert!(crate::om::point_feature_scalar_lane(&preceding, &target[..63]).is_none());

    let mut nonfinite = target;
    nonfinite[5..13].copy_from_slice(&[0x6f, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    assert!(crate::om::point_feature_scalar_lane(&preceding, &nonfinite).is_none());
}

#[test]
fn om_draft_feature_references_require_one_complete_graph() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "DRAFT",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let prefix = b"\x67\x00\x00\x01\x00\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x03\xff\xff\xff\xff\xff\xff\xff\xff\x01\x03\x80\x94\x82\x49";
    let graph = b"\x01\x02\xf1\x1b\x7c\x01\x02\xf1\x1b\x7d\x68\x2f\x70\x62\x4d\xd2\xf1\xa9\xfc\x03\x50\x44\x00\x00\x01\x46\x8a\x2a\x01\xa3\x60\x10\x01\x01\x01\x04\x02\x01\x02\x01\x00\x00\x00\x00\x01\xf1\x1b\x7e\xff\x00\x00\x00\xf1\x1b\x7f\xff";
    let terminal = b"\x81\x5e\x80\xb8\x01\x03\x02\x01\x02\x01\x01\x01\x00\x00\x00\x29\x29\x0c\x00";
    let payload = [prefix.as_slice(), graph.as_slice(), terminal.as_slice()].concat();
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: &payload,
        payload_offset: 200,
        payload: &payload,
        label,
    };
    let field = crate::om::draft_feature_payload_references(record).expect("complete graph");
    assert_eq!(
        field
            .references
            .clone()
            .map(|reference| reference.object_index),
        [7036, 7037, 7038, 7039]
    );
    assert_eq!(
        field.references.map(|reference| reference.offset),
        [230, 235, 273, 280]
    );
    let lane = crate::om::draft_feature_leading_index_lane(record).expect("complete index lane");
    assert_eq!(lane.declared_count, 3);
    assert_eq!(lane.indices, vec![(148, 224), (585, 226)]);
    assert_eq!(lane.raw_indices, vec![vec![0x80, 0x94], vec![0x82, 0x49]]);
    let terminal_lane =
        crate::om::draft_feature_terminal_lane(record).expect("complete terminal lane");
    assert_eq!(terminal_lane.indices, [350, 184]);
    assert_eq!(terminal_lane.raw_indices, [[0x81, 0x5e], [0x80, 0xb8]]);
    assert_eq!(terminal_lane.index_offsets, [284, 286]);
    assert_eq!(terminal_lane.tail, [0x29, 0x29, 0x0c]);
    assert_eq!(terminal_lane.offset, 284);

    let mut malformed = payload.clone();
    malformed[53] = 0x00;
    assert!(
        crate::om::draft_feature_payload_references(crate::om::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );
    let mut malformed_lane = payload.clone();
    malformed_lane[23] = 4;
    assert!(
        crate::om::draft_feature_leading_index_lane(crate::om::OperationRecord {
            bytes: &malformed_lane,
            payload: &malformed_lane,
            ..record
        })
        .is_none()
    );
    let ambiguous = [prefix.as_slice(), graph.as_slice(), graph.as_slice()].concat();
    assert!(
        crate::om::draft_feature_payload_references(crate::om::OperationRecord {
            bytes: &ambiguous,
            payload: &ambiguous,
            ..record
        })
        .is_none()
    );
    assert!(
        crate::om::draft_feature_payload_references(crate::om::OperationRecord {
            bytes: &payload[..prefix.len() + graph.len() - 2],
            payload: &payload[..prefix.len() + graph.len() - 2],
            ..record
        })
        .is_none()
    );
    assert!(
        crate::om::draft_feature_terminal_lane(crate::om::OperationRecord {
            bytes: &payload[..payload.len() - 1],
            payload: &payload[..payload.len() - 1],
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_surface_feature_references_require_the_complete_common_envelope() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "SKIN",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x3f\x00\x00\x01\x00\xf1\x02\x46\xf1\x02\x47\xf1\x02\x48\x01\x09\x03\x03\x04\x05\x02\x01\x01\x01\x01\x09\xf1\x02\x49\xf1\x02\x4a\xf1\x02\x4b\xf1\x02\x4c\xf1\x02\x4d\xf1\x02\x4e\xf1\x02\x4f\xf1\x02\x50\x00\x03\x03\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xf1\x02\x56\xf1\x02\x57\xf1\x02\x58\x01\x01\xff\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00\x00\x00\x01\x02";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let field = crate::om::surface_feature_payload_references(record).expect("complete envelope");
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| reference.object_index)
            .collect::<Vec<_>>(),
        [582, 583, 584, 585, 586, 587, 588, 589, 590, 591, 592, 598, 599, 600,]
    );

    let studio_payload = [&[0x14], &payload[1..]].concat();
    let studio = crate::om::OperationRecord {
        label: crate::om::OperationLabel {
            value: "Studio Surface",
            ..label
        },
        bytes: &studio_payload,
        payload: &studio_payload,
        ..record
    };
    assert!(crate::om::surface_feature_payload_references(studio).is_some());

    let mut malformed = payload.to_vec();
    let last = malformed.len() - 1;
    malformed[last] = 0x00;
    assert!(
        crate::om::surface_feature_payload_references(crate::om::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );

    let ambiguous = [payload.as_slice(), &payload[51..]].concat();
    assert!(
        crate::om::surface_feature_payload_references(crate::om::OperationRecord {
            bytes: &ambiguous,
            payload: &ambiguous,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_surface_feature_branches_require_one_complete_counted_group() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "SKIN",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\xa0\x5a\x14\x13\x01\x02\x40\x01\x04\xf1\x1b\xf4\xf1\x1b\xf5\xf1\x1b\xf6\x01\x04\x00\x00\x00\x00\x00\x00\x00\xff\x01\x02\xf1\x1b\xf7\x00\x81\x58\x01\x02\x40\x01\x05\xf1\x1b\xf8\xf1\x1b\xf9\xf1\x1b\xfa\xf1\x1b\xfb\x00\x00\x00\x00\x00\xff\x01\x02\xf1\x1b\xfc\x00\x81\x1c\x00\x00\x00\x01\x03\x00\x00\x00\xff\xff\x01";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let group = crate::om::surface_feature_payload_branches(record).expect("complete group");
    assert_eq!(group.family, 0x14);
    assert_eq!(group.header_code, 0x13);
    assert_eq!(group.branches.len(), 2);
    assert_eq!(group.branches[0].mode, 0x40);
    assert_eq!(group.branches[0].declared_count, 4);
    assert!(group.branches[0].witnessed);
    assert_eq!(group.branches[0].members.len(), 3);
    assert_eq!(group.branches[0].terminal.object_index, 7159);
    assert_eq!(group.branches[0].suffix, [0x81, 0x58, 0x01, 0x02]);
    assert_eq!(group.branches[1].declared_count, 5);
    assert!(!group.branches[1].witnessed);
    assert_eq!(group.branches[1].members.len(), 4);
    assert_eq!(group.branches[1].terminal.object_index, 7164);
    assert_eq!(group.branches[1].suffix, [0x81, 0x1c]);

    let studio_payload = [
        &payload[..payload.len() - 11],
        &[0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x01],
    ]
    .concat();
    let studio = crate::om::OperationRecord {
        label: crate::om::OperationLabel {
            value: "Studio Surface",
            ..label
        },
        bytes: &studio_payload,
        payload: &studio_payload,
        ..record
    };
    assert!(crate::om::surface_feature_payload_branches(studio).is_some());

    let mut malformed = payload.to_vec();
    malformed[19] = 0x03;
    assert!(
        crate::om::surface_feature_payload_branches(crate::om::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );

    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(
        crate::om::surface_feature_payload_branches(crate::om::OperationRecord {
            bytes: &ambiguous,
            payload: &ambiguous,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_sketch_payload_reference_field_is_counted_ordered_and_canonical() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "SKETCH",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x01\x00\x01\x05\xf0\xff\xf1\x01\x00\xf1\x01\x01\xf1\x01\x02\x00\x00\xf1\x01\x03\x01\x00\x00\x00";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let field = crate::om::sketch_payload_references(record).unwrap();
    assert_eq!(field.declared_count, 5);
    let references: [crate::om::PayloadObjectReference; 5] =
        field.references.clone().try_into().unwrap();
    assert_eq!(
        references.clone().map(|reference| reference.object_index),
        [255, 256, 257, 258, 259]
    );
    assert_eq!(
        references.map(|reference| reference.offset),
        [204, 206, 209, 212, 217]
    );
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| reference.raw_object_index.as_slice())
            .collect::<Vec<_>>(),
        [
            &[0xf0, 0xff][..],
            &[0xf1, 0x01, 0x00][..],
            &[0xf1, 0x01, 0x01][..],
            &[0xf1, 0x01, 0x02][..],
            &[0xf1, 0x01, 0x03][..],
        ]
    );
    let zero = b"\x01\x00\x00\x00\x00\xf0\x42\x01\x00\x00\x00";
    let field = crate::om::sketch_payload_references(crate::om::OperationRecord {
        payload: zero,
        bytes: zero,
        ..record
    })
    .unwrap();
    assert_eq!(field.declared_count, 0);
    assert_eq!(field.references.len(), 1);
    assert_eq!(field.references[0].object_index, 0x42);
    let two = b"\x01\x00\x01\x02\xf0\x41\x00\x00\xf0\x42\x01\x00\x00\x00";
    let field = crate::om::sketch_payload_references(crate::om::OperationRecord {
        payload: two,
        bytes: two,
        ..record
    })
    .unwrap();
    assert_eq!(field.declared_count, 2);
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| reference.object_index)
            .collect::<Vec<_>>(),
        [0x41, 0x42]
    );

    let mut noncanonical = payload.to_vec();
    noncanonical[7] = 0;
    assert!(
        crate::om::sketch_payload_references(crate::om::OperationRecord {
            payload: &noncanonical,
            bytes: &noncanonical,
            ..record
        })
        .is_none()
    );
    assert!(
        crate::om::sketch_payload_references(crate::om::OperationRecord {
            label: crate::om::OperationLabel {
                value: "BLOCK",
                ..label
            },
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_extrude_profile_references_require_matching_witness_field() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "EXTRUDE",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x01\x02\x16\x01\x03\xf0\xff\xf1\x01\x00\x01\x03\x79\xaa\x01\x03\xf0\xff\xf1\x01\x00\x00\x00";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let field = crate::om::extrude_profile_references(record).unwrap();
    assert!(field.witnessed);
    let references = field.references;
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].object_index, 255);
    assert_eq!(references[0].raw_object_index, [0xf0, 0xff]);
    assert_eq!(references[0].offset, 205);
    assert_eq!(references[1].object_index, 256);
    assert_eq!(references[1].raw_object_index, [0xf1, 0x01, 0x00]);
    assert_eq!(references[1].offset, 207);

    let without_witness = &payload[..14];
    let field = crate::om::extrude_profile_references(crate::om::OperationRecord {
        payload: without_witness,
        bytes: without_witness,
        ..record
    })
    .unwrap();
    assert!(!field.witnessed);
    assert_eq!(field.references.len(), 2);
    assert!(
        crate::om::extrude_profile_references(crate::om::OperationRecord {
            label: crate::om::OperationLabel {
                value: "SKETCH",
                ..label
            },
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_extrude_header_decodes_shifted_ieee_scalars() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "EXTRUDE",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload =
        b"\x0f\x00\x00\x01\x00\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x2f\xa3\x74\xbc\x6a\x7e\xf9\xdb";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let header = crate::om::extrude_payload_header(record).unwrap();
    assert_eq!(header.offset, 205);
    assert_eq!(header.scalars, [0.04, 0.038]);
    assert_eq!(header.raw_scalars.concat(), payload[5..21]);

    let mut invalid = payload.to_vec();
    invalid[5] = 0xf0;
    assert!(
        crate::om::extrude_payload_header(crate::om::OperationRecord {
            payload: &invalid,
            bytes: &invalid,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_extrude_footer_requires_one_complete_terminal_lane() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "EXTRUDE",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x01\x01\x02\x81\x5f\x80\xab\x01\x03\x02\x01\x01\x02\x01\x01\x00\x00\x00\x29\x29\x05\x80\xff\x00";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let footer = crate::om::extrude_payload_footer(record).unwrap();
    assert_eq!(footer.offset, 200);
    assert_eq!(footer.type_indices, [351, 171]);
    assert_eq!(
        footer.raw_type_indices,
        [vec![0x81, 0x5f], vec![0x80, 0xab]]
    );
    assert_eq!(footer.type_index_offsets, [203, 205]);
    assert_eq!(footer.mode_indices, [2, 1]);
    assert_eq!(footer.flags, [1, 2, 1, 1]);
    assert_eq!(footer.trailing_indices, [5, 255]);
    assert_eq!(footer.raw_trailing_indices, [vec![0x05], vec![0x80, 0xff]]);
    assert_eq!(footer.trailing_index_offsets, [220, 221]);

    let truncated = &payload[..payload.len() - 1];
    assert!(
        crate::om::extrude_payload_footer(crate::om::OperationRecord {
            payload: truncated,
            bytes: truncated,
            ..record
        })
        .is_none()
    );

    let mut ambiguous = payload[..payload.len() - 1].to_vec();
    ambiguous.extend_from_slice(payload);
    assert!(
        crate::om::extrude_payload_footer(crate::om::OperationRecord {
            payload: &ambiguous,
            bytes: &ambiguous,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_operation_body_scalar_clauses_preserve_body_order_and_branch() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "TRIM BODY",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let bytes = b"\x01\x02\x10\x42\xff\x1c\x00\x50\x40\x00\x00\xb0\x65\x40\x00\x00\x00\x00\x00\xaa\x01\x02\x10\x43\xff\x11\x30\x00\x00\x00\x00\x00\x00\x00\x00\x00";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes,
        payload_offset: 100,
        payload: bytes,
        label,
    };
    let triples = crate::om::operation_body_scalar_triples(record);
    assert_eq!(triples.len(), 2);
    assert_eq!(triples[0].body_reference_ordinal, 0);
    assert_eq!(triples[0].body_object_index, 66);
    assert_eq!(triples[0].branch, 0x1c);
    assert_eq!(
        triples[0].scalars.each_ref().map(|scalar| scalar.value),
        [0.0, 3.0, -170.0]
    );
    assert_eq!(
        triples[0].scalars.each_ref().map(|scalar| scalar.encoding),
        [
            crate::om::PayloadScalarEncoding::Zero,
            crate::om::PayloadScalarEncoding::Binary32,
            crate::om::PayloadScalarEncoding::Binary64,
        ]
    );
    assert_eq!(
        triples[0].scalars.each_ref().map(|scalar| scalar.offset),
        [106, 107, 111]
    );
    assert_eq!(
        triples[0]
            .scalars
            .each_ref()
            .map(|scalar| scalar.raw_value.as_slice()),
        [&bytes[6..7], &bytes[7..11], &bytes[11..19]]
    );
    assert_eq!(triples[1].body_reference_ordinal, 1);
    assert_eq!(triples[1].body_object_index, 67);
    assert_eq!(triples[1].branch, 0x11);
    assert_eq!(
        triples[1].scalars.each_ref().map(|scalar| scalar.value),
        [2.0, 0.0, 0.0]
    );
    let truncated = &bytes[..bytes.len() - 1];
    let truncated_triples = crate::om::operation_body_scalar_triples(crate::om::OperationRecord {
        bytes: truncated,
        payload: truncated,
        ..record
    });
    assert_eq!(truncated_triples.len(), 1);
    assert_eq!(truncated_triples[0], triples[0]);
}

#[test]
fn om_operation_body_branch_11_decodes_wrapped_member_lane_atomically() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "SEW",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let bytes = b"\x01\x02\x10\x42\xff\x11\x00\x50\x40\x00\x00\xb0\x65\x40\x00\x00\x00\x00\x00\x01\x03\x2e\x7f\x00\x2e\x80\x01\x00";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes,
        payload_offset: 100,
        payload: bytes,
        label,
    };
    let members = crate::om::operation_body_members(record);
    assert_eq!(members.len(), 2);
    assert_eq!(members[0].body_reference_ordinal, 0);
    assert_eq!(members[0].body_object_index, 66);
    assert_eq!(members[0].member_index, 127);
    assert_eq!(members[0].raw_member_index, [0x7f]);
    assert_eq!(members[0].offset, 122);
    assert_eq!(members[1].member_index, 1);
    assert_eq!(members[1].raw_member_index, [0x80, 0x01]);

    let truncated = &bytes[..bytes.len() - 1];
    assert!(
        crate::om::operation_body_members(crate::om::OperationRecord {
            bytes: truncated,
            payload: truncated,
            ..record
        })
        .is_empty()
    );
}

#[test]
fn om_trim_body_branch_11_decodes_terminal_continuation_atomically() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "TRIM BODY",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let bytes = b"\x01\x02\x10\x72\xff\x11\x00\x50\x40\x00\x00\xb0\x65\x40\x00\x00\x00\x00\x00\x01\x02\x2e\x41\x00\x01\x02\x80\x43\x00\x00\x01\x72\x00\x00";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes,
        payload_offset: 100,
        payload: bytes,
        label,
    };
    let continuations = crate::om::operation_body_11_continuations(record);
    assert_eq!(continuations.len(), 1);
    let continuation = &continuations[0];
    assert_eq!(continuation.body_reference_ordinal, 0);
    assert_eq!(continuation.body_object_index, 114);
    assert_eq!(continuation.continuation_index, 67);
    assert_eq!(continuation.raw_continuation_index, [0x80, 0x43]);
    assert_eq!(continuation.continuation_offset, 126);
    assert_eq!(continuation.terminal_object_index, 114);
    assert_eq!(continuation.raw_terminal_object_index, [0x72]);
    assert_eq!(continuation.terminal_offset, 131);

    let mut distinct_terminal = bytes.to_vec();
    distinct_terminal[31] = 0x71;
    assert_eq!(
        crate::om::operation_body_11_continuations(crate::om::OperationRecord {
            bytes: &distinct_terminal,
            payload: &distinct_terminal,
            ..record
        })[0]
            .terminal_object_index,
        113
    );

    let truncated = &bytes[..bytes.len() - 1];
    assert!(
        crate::om::operation_body_11_continuations(crate::om::OperationRecord {
            bytes: truncated,
            payload: truncated,
            ..record
        })
        .is_empty()
    );
}

#[test]
fn om_operation_body_decodes_homogeneous_unwrapped_reference_lanes() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "OFFSET",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let compact = b"\x01\x02\x10\x6e\xff\x1c\x00\x00\x00\x01\x03\x80\x0d\x69\x00\x00\x0b\x00";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: compact,
        payload_offset: 100,
        payload: compact,
        label,
    };
    let lanes = crate::om::operation_body_reference_lanes(record);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].body_object_index, 110);
    assert_eq!(
        lanes[0].encoding,
        crate::om::OperationBodyReferenceLaneEncoding::CompactIndex
    );
    assert_eq!(
        lanes[0]
            .values
            .iter()
            .map(|value| (value.object_index, value.offset))
            .collect::<Vec<_>>(),
        [(13, 111), (105, 113)]
    );
    assert_eq!(
        lanes[0]
            .values
            .iter()
            .map(|value| value.raw_value.as_slice())
            .collect::<Vec<_>>(),
        [b"\x80\x0d".as_slice(), b"\x69".as_slice()]
    );

    let objects =
        b"\x01\x02\x10\x70\xff\x1c\x00\x00\x00\x01\x03\xf1\x02\x9e\xf0\x44\x00\x00\x0b\x00";
    let object_record = crate::om::OperationRecord {
        bytes: objects,
        payload: objects,
        ..record
    };
    let lanes = crate::om::operation_body_reference_lanes(object_record);
    assert_eq!(
        lanes[0].encoding,
        crate::om::OperationBodyReferenceLaneEncoding::PayloadObjectIndex
    );
    assert_eq!(
        lanes[0]
            .values
            .iter()
            .map(|value| value.object_index)
            .collect::<Vec<_>>(),
        [670, 68]
    );
    assert_eq!(
        lanes[0]
            .values
            .iter()
            .map(|value| value.raw_value.as_slice())
            .collect::<Vec<_>>(),
        [b"\xf1\x02\x9e".as_slice(), b"\xf0\x44".as_slice()]
    );

    let truncated = &objects[..objects.len() - 1];
    assert!(
        crate::om::operation_body_reference_lanes(crate::om::OperationRecord {
            bytes: truncated,
            payload: truncated,
            ..object_record
        })
        .is_empty()
    );

    let branch_11 =
        b"\x01\x02\x10\x70\xff\x11\x00\x00\x00\x01\x03\xf1\x02\x9e\xf0\x44\x00\x00\x0b\x00";
    let lanes = crate::om::operation_body_reference_lanes(crate::om::OperationRecord {
        bytes: branch_11,
        payload: branch_11,
        ..record
    });
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].branch, 0x11);
    assert_eq!(
        lanes[0]
            .values
            .iter()
            .map(|value| value.object_index)
            .collect::<Vec<_>>(),
        [670, 68]
    );
}

#[test]
fn om_extrude_body_32_branch_decodes_counted_lanes() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "EXTRUDE",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let bytes = b"\x01\x02\x10\x73\xff\x32\x00\x00\x30\x77\x7e\x14\x7a\xe1\x47\xb3\x01\x03\x3d\x82\x56\x00\x3d\x82\x57\x00\x01\x04\x80\x2b\x80\x2d\x80\x2c\x01\x03\x80\x2e\x80\x77\x00\x01\x73\x00\x00";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes,
        payload_offset: 100,
        payload: bytes,
        label,
    };
    let branch = crate::om::extrude_payload_32_branch(record).unwrap();
    assert_eq!(branch.offset, 105);
    assert_eq!(branch.body_object_index, 115);
    assert!(branch.scalar.is_finite());
    assert_eq!(branch.raw_scalar, bytes[8..16]);
    assert_eq!(branch.atoms_be, [0x3d82_5600, 0x3d82_5700]);
    assert_eq!(branch.atom_offsets, [118, 122]);
    assert_eq!(branch.atom_indices, [598, 599]);
    assert_eq!(branch.first_indices, [43, 45, 44]);
    assert_eq!(
        branch.raw_first_indices,
        [vec![0x80, 0x2b], vec![0x80, 0x2d], vec![0x80, 0x2c]]
    );
    assert_eq!(branch.first_index_offsets, [128, 130, 132]);
    assert_eq!(branch.second_indices, [46, 119]);
    assert_eq!(
        branch.raw_second_indices,
        [vec![0x80, 0x2e], vec![0x80, 0x77]]
    );
    assert_eq!(branch.second_index_offsets, [136, 138]);
    assert_eq!(branch.terminal_object_index, 115);
    assert_eq!(branch.raw_terminal_object_index, [0x73]);
    assert_eq!(branch.terminal_offset, 142);

    let mut invalid = bytes.to_vec();
    invalid[36] = 0xff;
    assert!(
        crate::om::extrude_payload_32_branch(crate::om::OperationRecord {
            bytes: &invalid,
            payload: &invalid,
            ..record
        })
        .is_none()
    );

    let mut invalid_atom = bytes.to_vec();
    invalid_atom[18] = 0x3c;
    assert!(
        crate::om::extrude_payload_32_branch(crate::om::OperationRecord {
            bytes: &invalid_atom,
            payload: &invalid_atom,
            ..record
        })
        .is_none()
    );

    let mut wrong_terminal_body = bytes.to_vec();
    wrong_terminal_body[43] = 0x72;
    assert!(
        crate::om::extrude_payload_32_branch(crate::om::OperationRecord {
            bytes: &wrong_terminal_body,
            payload: &wrong_terminal_body,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_block_construction_field_decodes_ordered_canonical_references() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "BLOCK",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let mut payload = vec![0x26, 0, 0, 1, 0, 0];
    for value in 1..=18u8 {
        payload.extend([0xf0, value]);
    }
    payload.extend([0x01, 0xf1, 0x01, 0x00]);
    payload.extend([0xff; 11]);
    payload.extend([0; 4]);
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: &payload,
        payload_offset: 200,
        payload: &payload,
        label,
    };
    let field = crate::om::block_construction_references(record).unwrap();
    assert_eq!(field.control, 0x26);
    assert_eq!(field.references.len(), 19);
    assert_eq!(field.references[0].object_index, 1);
    assert_eq!(field.references[0].raw_object_index, [0xf0, 0x01]);
    assert_eq!(field.references[18].object_index, 256);
    assert_eq!(field.references[18].raw_object_index, [0xf1, 0x01, 0x00]);
    assert_eq!(field.references[0].offset, 206);

    let mut invalid = payload.clone();
    invalid[42] = 0xf0;
    assert!(
        crate::om::block_construction_references(crate::om::OperationRecord {
            bytes: &invalid,
            payload: &invalid,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_boolean_operations_decode_counted_target_and_tools() {
    let bytes = b"\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\xff\xff\xff\xff\x03\x0aSUBTRACT\0\x31\x00\x00\x01\x00\x14\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x03\x00\x00\xe0\x7f\xff\xff\xff\x01\x01\x01\x02\x90\x19\x5e\x00\x01\x05\x90\x19\x5f\x90\x19\x44\x90\x19\x43\x90\x19\x60\x00";
    let operations = crate::om::boolean_operations(bytes, 100);
    assert_eq!(operations.len(), 1);
    assert_eq!(
        operations[0].kind,
        crate::om::BooleanOperationKind::Subtract
    );
    assert_eq!(operations[0].target, 6494);
    assert_eq!(operations[0].raw_target, [0x90, 0x19, 0x5e]);
    assert_eq!(
        operations[0].target_offset,
        100 + bytes
            .windows(3)
            .position(|window| window == [0x90, 0x19, 0x5e])
            .unwrap()
    );
    assert_eq!(operations[0].tools, [6495, 6468, 6467, 6496]);
    assert_eq!(
        operations[0].raw_tools,
        [
            vec![0x90, 0x19, 0x5f],
            vec![0x90, 0x19, 0x44],
            vec![0x90, 0x19, 0x43],
            vec![0x90, 0x19, 0x60],
        ]
    );
    assert_eq!(
        operations[0].tool_offsets,
        [0x5f, 0x44, 0x43, 0x60].map(|low| {
            100 + bytes
                .windows(3)
                .position(|window| window == [0x90, 0x19, low])
                .unwrap()
        })
    );

    let mut invalid = bytes.to_vec();
    *invalid.last_mut().unwrap() = 1;
    assert!(crate::om::boolean_operations(&invalid, 0).is_empty());
}

#[test]
fn om_index_accepts_length_framed_root_version_text() {
    let mut bytes = indexed_om_section();
    let marker = bytes
        .windows(b"\x04\x01\x0eNX 2027.3102\0".len())
        .position(|window| window == b"\x04\x01\x0eNX 2027.3102\0")
        .expect("root record");
    bytes[marker + 2] = 0x0f;
    bytes.insert(marker + 3 + 12, b' ');
    let index = bytes
        .windows(4)
        .position(|window| window == 0u32.to_le_bytes())
        .expect("index");
    for ordinal in 2..4 {
        let at = index + ordinal * 4;
        let value = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) + 1;
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    let sections = crate::om::indexed_sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert!(sections[0].records[0]
        .bytes
        .starts_with(b"\x04\x01\x0fNX 2027.3102 \0"));
}

#[test]
fn om_store_version_can_follow_control_prefix() {
    let bytes = b"\xff\x00prefix\x04\x01\x0eNX 2027.3102\0tail";
    let version = crate::om::store_version(bytes, 100).expect("store version");
    assert_eq!(version.offset, 108);
    assert_eq!(version.value, "NX 2027.3102");
}

#[test]
fn om_offset_only_index_bounds_storage_blocks() {
    let bytes = offset_only_indexed_om_section();
    let sections = crate::om::indexed_sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].base, 0);
    assert_eq!(
        sections[0].control.as_ref().unwrap().bytes,
        &[0, 0, 0, 0, 0, 1, 0, 0]
    );
    assert_eq!(sections[0].records.len(), 2);
    assert_eq!(
        sections[0].column_storage.unwrap(),
        [sections[0].records[0].bytes, sections[0].records[1].bytes].concat()
    );
    assert_eq!(sections[0].records[0].object_id, None);
    assert!(sections[0].records[0].bytes.starts_with(b"\x04\x01\x0eNX "));
    assert_eq!(sections[0].records[1].object_id, None);
    assert!(sections[0].records[1].bytes.ends_with(b"\0"));
    let expressions = sections[0].numeric_expressions();
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].name, "length");
    assert_eq!(expressions[0].value, Some(25.0));
}

#[test]
fn om_offset_only_index_accepts_one_root_record_inside_control_block() {
    let bytes = control_root_offset_only_indexed_om_section();
    let sections = crate::om::indexed_sections(&bytes);

    assert_eq!(sections.len(), 1);
    assert!(sections[0]
        .control
        .as_ref()
        .unwrap()
        .bytes
        .windows(b"NX 2027.3102".len())
        .any(|window| window == b"NX 2027.3102"));
    assert_eq!(sections[0].records.len(), 2);
    assert_eq!(sections[0].records[0].bytes, &[0; 32]);
    assert_eq!(sections[0].numeric_expressions()[0].name, "length");
}

#[test]
fn om_offset_only_index_requires_one_supported_product_record() {
    let mut duplicate = control_root_offset_only_indexed_om_section();
    let first_column = duplicate
        .windows(32)
        .position(|window| window == [0; 32])
        .expect("zero first column");
    let duplicate_product = b"\x04\x01\x0eNX 2027.3102\0";
    duplicate[first_column..first_column + duplicate_product.len()]
        .copy_from_slice(duplicate_product);
    assert!(crate::om::indexed_sections(&duplicate).is_empty());

    let mut unsupported = control_root_offset_only_indexed_om_section();
    let product = unsupported
        .windows(b"\x05\x01\x0eNX 2027.3102\0".len())
        .position(|window| window == b"\x05\x01\x0eNX 2027.3102\0")
        .expect("product record");
    unsupported[product] = 0x03;
    assert!(crate::om::indexed_sections(&unsupported).is_empty());
}

#[test]
fn om_offset_store_control_values_require_complete_zero_prefixed_words() {
    assert_eq!(
        crate::om::offset_store_control_values(&[0, 0x34, 0x12, 0, 0, 0xff, 0xff, 0xff]),
        Some(vec![0x1234, 0x00ff_ffff])
    );
    assert!(crate::om::offset_store_control_values(&[]).is_none());
    assert!(crate::om::offset_store_control_values(&[0, 1, 2]).is_none());
    assert!(crate::om::offset_store_control_values(&[1, 1, 2, 3]).is_none());
}

#[test]
fn om_offset_store_index_rows_require_complete_exact_frames() {
    let first =
        b"\x2d\x02\x0b\x2a\x93\x8a\x03\x80\x18\x20\x20\x41\x00\x47\x04\x04\x01\xc0\x44\x04\x00";
    let second = b"\x2d\x02\x0b\x83\xb6\x93\x8a\x07\x80\x18\x20\x80\x4d\x41\x00\x47\x04\x04\x01\xc0\x44\x04\x00";
    let mut bytes = b"prefix".to_vec();
    bytes.extend_from_slice(first);
    bytes.extend_from_slice(b"gap");
    bytes.extend_from_slice(second);

    let rows = crate::om::offset_store_index_rows(&bytes);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].offset, 6);
    assert_eq!(rows[0].first_index, 42);
    assert_eq!(rows[0].raw_first_index, [0x2a]);
    assert_eq!(rows[0].flag, 3);
    assert_eq!(rows[0].indices, [(24, 13), (32, 15), (32, 16), (65, 17)]);
    assert_eq!(
        rows[0].raw_indices,
        [vec![0x80, 0x18], vec![0x20], vec![0x20], vec![0x41]]
    );
    assert_eq!(rows[1].first_index, 950);
    assert_eq!(rows[1].raw_first_index, [0x83, 0xb6]);
    assert_eq!(rows[1].flag, 7);
    assert_eq!(rows[1].indices, [(24, 38), (32, 40), (77, 41), (65, 43)]);
    assert_eq!(
        rows[1].raw_indices,
        [vec![0x80, 0x18], vec![0x20], vec![0x80, 0x4d], vec![0x41]]
    );

    let mut null = first.to_vec();
    null[3] = 0xff;
    assert!(crate::om::offset_store_index_rows(&null).is_empty());
    let mut other_flag = first.to_vec();
    other_flag[6] = 0x04;
    assert!(crate::om::offset_store_index_rows(&other_flag).is_empty());
    let mut overlong = first.to_vec();
    overlong.insert(12, 0x01);
    assert!(crate::om::offset_store_index_rows(&overlong).is_empty());
    assert!(crate::om::offset_store_index_rows(&first[..first.len() - 1]).is_empty());
}

#[test]
fn om_offset_store_linked_index_rows_require_complete_exact_frames() {
    let row = b"\x02\x0b\x83\x93\x93\x8c\x16\x24\xff\xff\x90\xfe\x20\x20\x41\x00\x47\x03\x04\x01\xc0\x44\x04\x00";
    let rows = crate::om::offset_store_linked_index_rows(row);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].first_index, (915, 2));
    assert_eq!(rows[0].raw_first_index, [0x83, 0x93]);
    assert_eq!(rows[0].discriminator, 0x16);
    assert_eq!(rows[0].target_index, (36, 7));
    assert_eq!(rows[0].raw_target_index, [0x24]);
    assert_eq!(rows[0].indices, [(32, 12), (32, 13), (65, 14)]);
    assert_eq!(rows[0].raw_indices, [vec![0x20], vec![0x20], vec![0x41]]);
    assert_eq!(rows[0].flag, 3);
    assert_eq!(rows[0].mode, 4);

    let mut null = row.to_vec();
    null[7] = 0xff;
    assert!(crate::om::offset_store_linked_index_rows(&null).is_empty());
    let mut discriminator = row.to_vec();
    discriminator[6] = 0x15;
    assert!(crate::om::offset_store_linked_index_rows(&discriminator).is_empty());
    let mut flag = row.to_vec();
    flag[17] = 0x04;
    assert!(crate::om::offset_store_linked_index_rows(&flag).is_empty());
    let mut mode = row.to_vec();
    mode[18] = 0x06;
    assert!(crate::om::offset_store_linked_index_rows(&mode).is_empty());
    let mut mode_seven = row.to_vec();
    mode_seven[18] = 0x07;
    assert_eq!(
        crate::om::offset_store_linked_index_rows(&mode_seven)[0].mode,
        7
    );
    assert!(crate::om::offset_store_linked_index_rows(&row[..row.len() - 1]).is_empty());
}

#[test]
fn om_offset_store_target_index_rows_require_complete_exact_frames() {
    let row =
        b"\x02\x01\x01\x01\x16\x3e\xff\xff\x90\xfe\x1e\x20\x58\x00\x47\x03\x07\x01\xc0\x44\x04\x00";
    let rows = crate::om::offset_store_target_index_rows(row);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].target_index, (62, 5));
    assert_eq!(rows[0].raw_target_index, [0x3e]);
    assert_eq!(rows[0].indices, [(30, 10), (32, 11), (88, 12)]);
    assert_eq!(rows[0].raw_indices, [vec![0x1e], vec![0x20], vec![0x58]]);
    assert_eq!(rows[0].mode, 7);

    let mut null = row.to_vec();
    null[5] = 0xff;
    assert!(crate::om::offset_store_target_index_rows(&null).is_empty());
    let mut discriminator = row.to_vec();
    discriminator[4] = 0x17;
    assert!(crate::om::offset_store_target_index_rows(&discriminator).is_empty());
    let mut suffix = row.to_vec();
    suffix[16] = 0x03;
    assert!(crate::om::offset_store_target_index_rows(&suffix).is_empty());
    let mut mode_four = row.to_vec();
    mode_four[16] = 0x04;
    assert_eq!(
        crate::om::offset_store_target_index_rows(&mode_four)[0].mode,
        4
    );
    assert!(crate::om::offset_store_target_index_rows(&row[..row.len() - 1]).is_empty());
}

#[test]
fn om_offset_store_control_class_lane_is_a_distinct_in_range_prefix() {
    let encode = |values: &[u32]| {
        values
            .iter()
            .flat_map(|value| {
                let bytes = value.to_le_bytes();
                [0, bytes[0], bytes[1], bytes[2]]
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        crate::om::offset_store_control_class_ordinals(&encode(&[2, 0, 4, 8]), 4),
        Some(vec![2, 0])
    );
    assert!(crate::om::offset_store_control_class_ordinals(&encode(&[2, 2, 4]), 4).is_none());
    assert!(crate::om::offset_store_control_class_ordinals(&encode(&[2, 4, 1]), 4).is_none());
    assert!(crate::om::offset_store_control_class_ordinals(&encode(&[4, 8]), 4).is_none());
}

#[test]
fn om_registry_uses_length_framing_and_stays_outside_entity_payloads() {
    let mut bytes = indexed_om_section();
    bytes.extend_from_slice(b"\x10UGS::PayloadText");
    let sections = crate::om::indexed_sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].types.len(), 1);
    assert_eq!(sections[0].types[0].name, "UGS::EXP_expression");
    assert_eq!(sections[0].types[0].trailing_code, 0x81);
    assert_eq!(sections[0].types[0].offset, 8);
}

#[test]
fn om_numeric_expression_retains_identity_name_unit_and_value() {
    let bytes = indexed_om_section();
    let section = crate::om::indexed_sections(&bytes).remove(0);
    let expression_records = section.numeric_expression_records();
    assert_eq!(expression_records[0].0, 1);
    let expressions = expression_records
        .iter()
        .map(|(_, expression)| expression)
        .collect::<Vec<_>>();
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].object_id, Some(0x102));
    assert_eq!(
        expressions[0].name,
        "p8_CircularPattern_pattern_Circular_Dir_offset_angle"
    );
    assert_eq!(expressions[0].parameter_index, Some(8));
    assert_eq!(
        expressions[0].qualifier,
        Some("CircularPattern_pattern_Circular_Dir_offset_angle")
    );
    assert_eq!(expressions[0].unit, crate::om::ExpressionUnit::Degree);
    assert_eq!(expressions[0].expression, "120");
    assert_eq!(expressions[0].value, Some(120.0));
    let declaration = crate::om::expression_declaration_name(section.records[1].bytes).unwrap();
    assert_eq!(
        declaration.value,
        "p8_CircularPattern_pattern_Circular_Dir_offset_angle"
    );
    assert_eq!(declaration.parameter_index, 8);
    assert_eq!(
        declaration.qualifier,
        Some("CircularPattern_pattern_Circular_Dir_offset_angle")
    );
    assert_eq!(declaration.literal, Some("120"));
    let declaration =
        crate::om::expression_declaration_name(b"\x04\x04p1\0\x04\x0a-5.1 * 2\0").unwrap();
    assert_eq!(declaration.value, "p1");
    assert_eq!(declaration.literal, Some("-5.1 * 2"));
    let declaration =
        crate::om::expression_declaration_name(b"\x04\x04p1\0\x04\x055.1\0\x04\x05120\0").unwrap();
    assert_eq!(declaration.literal, None);
    assert!(crate::om::expression_declaration_name(b"\x04\x04p1\0\x04\x04p2\0").is_none());
    assert!(crate::om::expression_declaration_name(b"\x04\x05p1-\0").is_none());
}

#[test]
fn om_numeric_expression_types_only_canonical_parameter_names() {
    for name in ["p12foo", "p12_", "p4294967296_radius"] {
        let text = format!("(Number [mm]) {name}: 5; ");
        let mut bytes = b"hostglobalvariables".to_vec();
        bytes.extend_from_slice(&[0x99, 0x04, (text.len() + 2) as u8]);
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(0);

        let expressions = crate::om::numeric_expressions(&bytes);
        assert_eq!(expressions.len(), 1);
        assert_eq!(expressions[0].name, name);
        assert_eq!(expressions[0].parameter_index, None);
        assert_eq!(expressions[0].qualifier, None);
    }
    assert!(crate::om::expression_declaration_name(b"\x04\x08p12foo\0").is_none());
    assert!(crate::om::expression_declaration_name(b"\x04\x06p12_\0").is_none());
}

#[test]
fn om_numeric_expression_evaluates_constant_arithmetic_formula() {
    let text = b"(Number [mm]) p9: (193.94 - 6) / 2 + 1.5e1; ";
    let mut bytes = b"hostglobalvariables".to_vec();
    bytes.extend_from_slice(&[0x99, 0x04, (text.len() + 2) as u8]);
    bytes.extend_from_slice(text);
    bytes.push(0);

    let expressions = crate::om::numeric_expressions(&bytes);
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].expression, "(193.94 - 6) / 2 + 1.5e1");
    assert_eq!(expressions[0].value, Some(108.97));
}

#[test]
fn om_numeric_expression_applies_power_before_unary_sign() {
    for (formula, expected) in [
        ("-2^2", -4.0),
        ("(-2)^2", 4.0),
        ("2^-2", 0.25),
        ("2^3^2", 512.0),
    ] {
        assert_eq!(
            crate::om::evaluate_constant_expression(formula),
            Some(expected),
            "{formula}"
        );
    }
}

#[test]
fn om_string_value_requires_marker_length_printability_and_terminator() {
    let bytes = b"\x66\x32\x03\x0cSKETCH_001\0\x66\x32\x03\x03A\0\x66\x32\x03\x03A\x01";
    let values = crate::om::string_values(bytes, 100);
    assert_eq!(values.len(), 2);
    assert_eq!(values[0].offset, 100);
    assert_eq!(values[0].value, "SKETCH_001");
    assert_eq!(values[1].value, "A");
}

#[test]
fn om_tagged_references_preserve_family_value_order_and_bounds() {
    let bytes = b"\xe0\x12\x34\x56\x78\xca\xbc\xde\xf0\xe0\x01";
    let references = crate::om::references(bytes, 20);
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].offset, 20);
    assert_eq!(
        references[0].kind,
        crate::om::ReferenceKind::PersistentHandle
    );
    assert_eq!(references[0].value, 0x1234_5678);
    assert_eq!(references[1].offset, 25);
    assert_eq!(references[1].kind, crate::om::ReferenceKind::Tagged28);
    assert_eq!(references[1].value, 0x0abc_def0);
}

#[test]
fn om_counted_record_references_require_a_complete_in_bounds_run() {
    let bytes = b"\xff\x01\x03\x90\x00\x02\x90\x00\x04\x01\x02\x90\x00\x05";
    let references = crate::om::counted_record_references(bytes, 100, 5);
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].offset, 103);
    assert_eq!(
        references[0].kind,
        crate::om::ReferenceKind::RecordOrdinal16
    );
    assert_eq!(references[0].value, 2);
    assert_eq!(references[1].value, 4);
}

#[test]
fn om_record_reference_stream_requires_dense_suffix() {
    let mut dense = b"ordinary-prefix".to_vec();
    for value in 1..=8u32 {
        dense.push(0xe0);
        dense.extend_from_slice(&value.to_be_bytes());
        dense.extend_from_slice(&(0xc000_0000 | value).to_be_bytes());
    }
    let references = crate::om::dense_reference_suffix(&dense, 100);
    assert_eq!(references.len(), 16);
    assert_eq!(references[0].offset, 115);

    let mut sparse = dense;
    sparse.extend_from_slice(&[0x55; 9]);
    assert!(crate::om::dense_reference_suffix(&sparse, 0).is_empty());
}

#[test]
fn om_numeric_expression_table_is_independent_of_entity_indexing() {
    let bytes = b"hostglobalvariables\x99\x04P(Number [degrees]) p8_CircularPattern_pattern_Circular_Dir_offset_angle: 120; \x00";
    let expressions = crate::om::numeric_expressions(bytes);
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].object_id, None);
    assert_eq!(
        expressions[0].name,
        "p8_CircularPattern_pattern_Circular_Dir_offset_angle"
    );
    assert_eq!(expressions[0].parameter_index, Some(8));
    assert_eq!(
        expressions[0].qualifier,
        Some("CircularPattern_pattern_Circular_Dir_offset_angle")
    );
    assert_eq!(expressions[0].value, Some(120.0));
}

#[test]
fn parasolid_entity_51_records_retain_layout_selected_references() {
    let mut bytes = vec![0, 0x51];
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&10u16.to_be_bytes());
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&0x21u16.to_be_bytes());
    for reference in 3..=8u16 {
        bytes.extend_from_slice(&reference.to_be_bytes());
    }
    bytes.extend_from_slice(&[0xaa, 0xbb]);

    let records = crate::parasolid::entity_51_records(&bytes);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].offset, 0);
    assert_eq!(records[0].byte_len, 26);
    assert_eq!(records[0].xmt, 10);
    assert_eq!(records[0].sequence, 2);
    assert_eq!(records[0].discriminator, 0x21);
    assert_eq!(records[0].references, vec![3, 4, 5, 6, 7, 8]);
}

#[test]
fn parasolid_entity_54_strings_require_exact_length_and_terminator() {
    let mut bytes = vec![0xaa, 0x00, 0x54];
    bytes.extend_from_slice(&8u32.to_be_bytes());
    bytes.extend_from_slice(&17u16.to_be_bytes());
    bytes.extend_from_slice(b"deadbeef\0");
    bytes.extend_from_slice(&[0xbb, 0x00, 0x54, 0, 0, 0, 3, 0, 18, b'a', b'b', b'c', 1]);

    let records = crate::parasolid::entity_54_string_records(&bytes);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].offset, 1);
    assert_eq!(records[0].byte_len, 17);
    assert_eq!(records[0].xmt, 17);
    assert_eq!(records[0].value, "deadbeef");
}

#[test]
fn parasolid_entity_52_integers_require_complete_counted_values() {
    let mut bytes = vec![0xaa, 0x00, 0x52];
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&17u16.to_be_bytes());
    bytes.extend_from_slice(&3u32.to_be_bytes());
    bytes.extend_from_slice(&u32::MAX.to_be_bytes());

    let records = crate::parasolid::entity_52_integer_records(&bytes);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].offset, 1);
    assert_eq!(records[0].xmt, 17);
    assert_eq!(records[0].values, [3, u32::MAX]);
    assert_eq!(records[0].byte_len, 16);
    assert!(crate::parasolid::entity_52_integer_records(&bytes[..bytes.len() - 1]).is_empty());
}

#[test]
fn parasolid_entity_53_doubles_require_complete_finite_values() {
    let mut bytes = vec![0xaa, 0x00, 0x53, 0xff];
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&18u16.to_be_bytes());
    bytes.extend_from_slice(&0.001f64.to_be_bytes());
    bytes.extend_from_slice(&0.25f64.to_be_bytes());

    let records = crate::parasolid::entity_53_double_records(&bytes);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].offset, 1);
    assert_eq!(records[0].xmt, 18);
    assert_eq!(records[0].values, [0.001, 0.25]);
    assert_eq!(records[0].byte_len, 25);

    let last = bytes.len() - 8;
    bytes[last..].copy_from_slice(&f64::NAN.to_be_bytes());
    assert!(crate::parasolid::entity_53_double_records(&bytes).is_empty());
}

#[test]
fn topology_rejects_shell_with_broken_face_ownership_chain() {
    let valid = topology_partition_stream();
    let graph = crate::topology::Graph::parse(&valid);
    assert_eq!(graph.body_shape_shells().len(), 1);

    let mut broken = valid;
    let face = broken
        .windows(2)
        .position(|window| window == [0, 14])
        .expect("face record");
    put_ref(&mut broken, face + 24, 99);
    assert!(crate::topology::Graph::parse(&broken)
        .body_shape_shells()
        .is_empty());

    let mut independent_previous = topology_partition_stream();
    let face = independent_previous
        .windows(2)
        .position(|window| window == [0, 14])
        .expect("face record");
    put_ref(&mut independent_previous, face + 20, 99);
    assert_eq!(
        crate::topology::Graph::parse(&independent_previous)
            .body_shape_shells()
            .len(),
        1
    );
}

#[test]
fn topology_retains_shell_body_identity_without_body_record() {
    let mut stream = topology_partition_stream();
    let body = stream
        .windows(4)
        .position(|window| window == [0, 12, 0, 2])
        .expect("body record");
    stream[body..body + 24].fill(0xff);

    let graph = crate::topology::Graph::parse(&stream);
    assert!(graph.get(12, 2).is_none());
    assert_eq!(graph.body_shape_shells().len(), 1);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.bodies[0].id.0, "nx:s0:body#2");
    assert_eq!(result.ir.model.faces.len(), 1);
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn topology_accepts_cached_last_face_and_implicit_region_identity() {
    let mut stream = topology_partition_stream();
    let shell = stream
        .windows(4)
        .position(|window| window == [0, 13, 0, 3])
        .expect("shell record");
    put_ref(&mut stream, shell + 22, 4);
    let region = stream
        .windows(4)
        .position(|window| window == [0, 19, 0, 12])
        .expect("region record");
    stream[region..region + 16].fill(0xff);
    let mut second_face = record(14, 39);
    put_ref(&mut second_face, 2, 20);
    put_f64(&mut second_face, 10, 0.000_2);
    put_ref(&mut second_face, 18, 1);
    put_ref(&mut second_face, 20, 1);
    put_ref(&mut second_face, 22, 1);
    put_ref(&mut second_face, 24, 3);
    put_ref(&mut second_face, 26, 6);
    second_face[28] = b'+';
    stream.extend(second_face);

    let graph = crate::topology::Graph::parse(&stream);
    assert!(graph.get(19, 12).is_none());
    assert_eq!(graph.body_shape_shells().len(), 1);
    assert_eq!(graph.body_shape_face_count(), 2);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir.model.regions.len(), 1);
    assert_eq!(result.ir.model.regions[0].id.0, "nx:s0:region#12");
    assert_eq!(result.ir.model.faces.len(), 2);
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn topology_rejects_nonreciprocal_fin_ring() {
    let mut stream = topology_partition_stream();
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    put_ref(&mut stream, fin + 8, 99);
    let graph = crate::topology::Graph::parse(&stream);
    assert!(graph.face_loop_rings(4).is_none());

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert!(result.ir.model.loops.is_empty());
    assert!(result.ir.model.coedges.is_empty());
    assert!(result.ir.model.edges.is_empty());

    let mut broken_partner = topology_partition_stream();
    let fin = broken_partner
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    put_ref(&mut broken_partner, fin + 14, 99);
    assert!(crate::topology::Graph::parse(&broken_partner)
        .face_loop_rings(4)
        .is_none());
}

#[test]
fn topology_accepts_fixed_record_envelope_escape() {
    let mut stream = topology_partition_stream();
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    stream.insert(fin + 2, 0xff);
    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(
        graph.get(17, 7).unwrap().attribute_field_offset(),
        Some(fin + 5)
    );
    assert_eq!(graph.face_loop_rings(4).unwrap().len(), 1);
}

#[test]
fn topology_iterates_each_record_family_in_physical_order() {
    let mut stream = Vec::new();
    for (xmt, x) in [(77, 0.01), (3, 0.02)] {
        let mut point = record(29, 40);
        put_ref(&mut point, 2, xmt);
        put_vec3(&mut point, 16, [x, 0.0, 0.0]);
        stream.extend(point);
    }

    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(
        graph.of_kind(29).map(|node| node.xmt).collect::<Vec<_>>(),
        vec![77, 3]
    );
}

#[test]
fn decode_synthesizes_vertex_for_closed_null_vertex_fin() {
    let mut stream = topology_partition_stream();
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    put_ref(&mut stream, fin + 12, 1);
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    let edge = result.ir.model.edges.first().expect("closed edge");
    assert_eq!(edge.start, edge.end);
    assert!(edge.start.0.contains("closed-edge"));
    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 1);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn topology_invalid_candidate_cannot_shadow_later_valid_record() {
    let mut stream = record(14, 39);
    put_ref(&mut stream, 2, 4);
    stream.extend(topology_partition_stream());

    let graph = crate::topology::Graph::parse(&stream);
    let face = graph.get(14, 4).expect("valid later FACE");
    assert!(face.pos >= 39);
    assert!(face.face_fields().is_some());
}

#[test]
fn decode_retains_topology_owned_point_at_origin() {
    let mut stream = topology_partition_stream();
    let point = stream
        .windows(4)
        .position(|window| window == [0, 29, 0, 11])
        .expect("point record");
    put_vec3(&mut stream, point + 16, [0.0, 0.0, 0.0]);

    assert!(crate::geometry::points(&stream).is_empty());
    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(
        graph
            .get(29, 11)
            .and_then(crate::topology::Node::point_position),
        Some(cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0))
    );
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert_eq!(result.ir.model.bodies[0].transform, None);
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(
        result.ir.model.points[0].position,
        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)
    );
}

#[test]
fn decode_orders_graph_only_origin_before_later_nonzero_point() {
    let mut stream = topology_partition_stream();
    let first = stream
        .windows(4)
        .position(|window| window == [0, 29, 0, 11])
        .expect("point record");
    put_vec3(&mut stream, first + 16, [0.0, 0.0, 0.0]);
    let mut second = record(29, 40);
    put_ref(&mut second, 2, 77);
    put_vec3(&mut second, 16, [0.04, 0.05, 0.06]);
    stream.extend(second);

    let graph = crate::topology::Graph::parse(&stream);
    let points = crate::decode::ordered_point_candidates(&stream, &graph);
    assert_eq!(points.len(), 2);
    assert_eq!(points[0].0, first);
    assert_eq!(points[0].1, cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0));
    assert_eq!(points[0].2.map(|node| node.xmt), Some(11));
    assert_eq!(points[1].0, stream.len() - 40);
    assert_eq!(points[1].1, cadmpeg_ir::math::Point3::new(40.0, 50.0, 60.0));
    assert_eq!(points[1].2.map(|node| node.xmt), Some(77));
}

#[test]
fn decode_orders_graph_only_escaped_analytics_before_later_records() {
    let mut stream = topology_with_escaped_geometry_envelopes();
    let first_surface = stream
        .windows(3)
        .position(|window| window == [0, 50, 0xff])
        .expect("escaped plane record");
    let first_curve = stream
        .windows(3)
        .position(|window| window == [0, 30, 0xff])
        .expect("escaped line record");

    let second_surface_offset = stream.len();
    let mut plane = record(50, 91);
    put_ref(&mut plane, 2, 77);
    plane[18] = b'+';
    put_vec3(&mut plane, 19, [0.01, 0.02, 0.03]);
    put_vec3(&mut plane, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut plane, 67, [1.0, 0.0, 0.0]);
    stream.extend(plane);

    let second_curve_offset = stream.len();
    let mut line = record(30, 67);
    put_ref(&mut line, 2, 78);
    line[18] = b'+';
    put_vec3(&mut line, 19, [0.04, 0.05, 0.06]);
    put_vec3(&mut line, 43, [0.0, 1.0, 0.0]);
    stream.extend(line);

    let graph = crate::topology::Graph::parse(&stream);
    let surfaces = crate::decode::ordered_surface_candidates(&stream, &graph);
    assert_eq!(surfaces.len(), 2);
    assert_eq!(surfaces[0].0, first_surface);
    assert_eq!(surfaces[0].2.map(|node| node.xmt), Some(6));
    assert_eq!(surfaces[1].0, second_surface_offset);
    assert_eq!(surfaces[1].2.map(|node| node.xmt), Some(77));

    let curves = crate::decode::ordered_curve_candidates(&stream, &graph);
    assert_eq!(curves.len(), 2);
    assert_eq!(curves[0].0, first_curve);
    assert_eq!(curves[0].2.map(|node| node.xmt), Some(9));
    assert_eq!(curves[1].0, second_curve_offset);
    assert_eq!(curves[1].2.map(|node| node.xmt), Some(78));
}

#[test]
fn decode_does_not_attach_unreferenced_point_to_solid_topology() {
    let mut stream = topology_partition_stream();
    let mut point = record(29, 40);
    put_ref(&mut point, 2, 77);
    put_vec3(&mut point, 16, [0.04, 0.05, 0.06]);
    stream.extend_from_slice(&point);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.points.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert_eq!(result.ir.model.shells[0].free_vertices.len(), 0);
    assert_eq!(result.ir.model.bodies.len(), 1);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_retains_connected_topology_with_unknown_surface_carrier() {
    let mut stream = topology_partition_stream();
    let face = stream
        .windows(2)
        .position(|window| window == [0, 14])
        .expect("face record");
    put_ref(&mut stream, face + 26, 99);
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.faces.len(), 1);
    let surface = result
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == result.ir.model.faces[0].surface)
        .expect("unknown face carrier");
    assert!(matches!(surface.geometry, SurfaceGeometry::Unknown { .. }));
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_retains_unknown_non_null_edge_curve_carrier() {
    let mut stream = topology_partition_stream();
    let edge = stream
        .windows(2)
        .position(|window| window == [0, 16])
        .expect("edge record");
    put_ref(&mut stream, edge + 24, 99);
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    let curve = result.ir.model.edges[0]
        .curve
        .as_ref()
        .and_then(|id| result.ir.model.curves.iter().find(|curve| &curve.id == id))
        .expect("unknown edge carrier");
    assert!(matches!(curve.geometry, CurveGeometry::Unknown { .. }));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_drops_unknown_carrier_outside_emitted_topology() {
    let mut stream = topology_partition_stream();
    let mut orphan = record(16, 32);
    put_ref(&mut orphan, 2, 88);
    put_f64(&mut orphan, 10, 0.000_3);
    put_ref(&mut orphan, 18, 1);
    put_ref(&mut orphan, 24, 99);
    stream.extend(orphan);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert!(result
        .ir
        .model
        .curves
        .iter()
        .all(|curve| !matches!(curve.geometry, CurveGeometry::Unknown { .. })));
    assert_eq!(result.ir.model.edges.len(), 1);
}

#[test]
fn decode_retains_native_carrierless_edge() {
    let mut stream = topology_partition_stream();
    let edge = stream
        .windows(2)
        .position(|window| window == [0, 16])
        .expect("edge record");
    put_ref(&mut stream, edge + 24, 1);
    let fin = stream
        .windows(2)
        .position(|window| window == [0, 17])
        .expect("fin record");
    put_ref(&mut stream, fin + 18, 1);
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    let edge = &result.ir.model.edges[0];
    assert_eq!(edge.curve, None);
    assert_eq!(edge.param_range, None);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn tolerant_edge_becomes_a_two_support_procedural_intersection() {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let edge_id = ir.model.edges[0].id.clone();
    ir.model.edges[0].curve = None;
    ir.model.edges[0].param_range = None;
    ir.model.edges[0].tolerance = Some(0.01);
    let mut edges = std::collections::BTreeMap::new();
    edges.insert(12, edge_id.clone());
    let graph = crate::topology::Graph::parse(&[]);
    let mut annotations = cadmpeg_ir::annotations::AnnotationBuilder::new();
    let stream = annotations.stream("nx:test");

    crate::decode::attach_tolerant_edge_intersections(
        &mut ir,
        &graph,
        &edges,
        "nx:test",
        stream,
        &mut annotations,
    );

    let edge = ir
        .model
        .edges
        .iter()
        .find(|edge| edge.id == edge_id)
        .expect("tolerant edge");
    assert_eq!(edge.param_range, Some([0.0, 1.0]));
    let curve = ir
        .model
        .curves
        .iter()
        .find(|curve| Some(&curve.id) == edge.curve.as_ref())
        .expect("procedural carrier");
    assert!(matches!(curve.geometry, CurveGeometry::Procedural { .. }));
    let procedural = ir
        .model
        .procedural_curves
        .iter()
        .find(|procedural| procedural.curve == curve.id)
        .expect("intersection construction");
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &procedural.definition
    else {
        panic!("intersection definition");
    };
    assert!(context.sides.iter().all(|side| side.surface.is_some()));
    assert_ne!(context.sides[0].surface, context.sides[1].surface);
}

#[test]
fn intersection_support_completion_requires_one_unique_incident_complement() {
    use cadmpeg_ir::geometry::{
        IntcurveSupportContext, IntcurveSupportSide, Pcurve, ProceduralCurve,
    };
    use cadmpeg_ir::ids::{PcurveId, ProceduralCurveId};

    let mut ir = cadmpeg_ir::examples::unit_cube();
    let edge = ir.model.edges[0].clone();
    let incident = ir
        .model
        .coedges
        .iter()
        .filter(|coedge| coedge.edge == edge.id)
        .filter_map(|coedge| {
            let face = ir
                .model
                .loops
                .iter()
                .find(|loop_| loop_.id == coedge.owner_loop)?
                .face
                .clone();
            ir.model
                .faces
                .iter()
                .find(|candidate| candidate.id == face)
                .map(|face| face.surface.clone())
        })
        .collect::<Vec<_>>();
    assert_eq!(incident.len(), 2);
    let curve = edge.curve.expect("cube edge curve");
    ir.model.procedural_curves.push(ProceduralCurve {
        id: ProceduralCurveId("nx:test:intersection#0".into()),
        curve,
        definition: ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext {
                sides: [
                    IntcurveSupportSide {
                        surface: Some(incident[0].clone()),
                        pcurve_parameter_range: None,
                        pcurve: None,
                    },
                    IntcurveSupportSide {
                        surface: None,
                        pcurve_parameter_range: None,
                        pcurve: None,
                    },
                ],
                parameter_range: [0.0, 1.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: None,
    });

    crate::decode::complete_intersection_supports_from_edge_incidence(&mut ir);
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves[0].definition
    else {
        panic!("intersection");
    };
    assert_eq!(context.sides[1].surface.as_ref(), Some(&incident[1]));

    let pcurve_id = PcurveId("nx:test:pcurve#0".into());
    let pcurve_geometry = PcurveGeometry::Line {
        origin: Point2::new(0.0, 0.0),
        direction: Point2::new(1.0, 0.0),
    };
    ir.model.pcurves.push(Pcurve {
        id: pcurve_id.clone(),
        geometry: pcurve_geometry.clone(),
        wrapper_reversed: None,
        native_tail_flags: None,
        parameter_range: Some([0.0, 1.0]),
        fit_tolerance: None,
    });
    let second_face = ir
        .model
        .faces
        .iter()
        .find(|face| face.surface == incident[1])
        .expect("second incident face")
        .id
        .clone();
    let second_loop = ir
        .model
        .loops
        .iter()
        .find(|loop_| loop_.face == second_face)
        .expect("second incident loop")
        .id
        .clone();
    ir.model
        .coedges
        .iter_mut()
        .find(|coedge| coedge.edge == edge.id && coedge.owner_loop == second_loop)
        .expect("second incident coedge")
        .pcurves = vec![cadmpeg_ir::topology::PcurveUse {
        pcurve: pcurve_id,
        isoparametric: None,
        parameter_range: None,
    }];

    crate::decode::complete_intersection_pcurves_from_coedge_incidence(&mut ir);
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves[0].definition
    else {
        panic!("intersection");
    };
    assert_eq!(context.sides[1].pcurve.as_ref(), Some(&pcurve_geometry));
}

#[test]
fn opposite_intersection_chart_transfers_adaptively_within_edge_tolerance() {
    use cadmpeg_ir::geometry::{
        Curve, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve, Surface,
    };
    use cadmpeg_ir::ids::{CurveId, EdgeId, ProceduralCurveId, SurfaceId, VertexId};
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::Edge;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let source = SurfaceId("synthetic:source-cylinder".into());
    let target = SurfaceId("synthetic:target-plane".into());
    ir.model.surfaces.extend([
        Surface {
            id: source.clone(),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 10.0,
            },
            source_object: None,
        },
        Surface {
            id: target.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
    ]);
    let curve = CurveId("synthetic:intersection-curve".into());
    let construction = ProceduralCurveId("synthetic:intersection".into());
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Procedural {
            construction: construction.clone(),
        },
        source_object: None,
    });
    ir.model.procedural_curves.push(ProceduralCurve {
        id: construction,
        curve: curve.clone(),
        definition: ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext {
                sides: [
                    IntcurveSupportSide {
                        surface: Some(source),
                        pcurve_parameter_range: None,
                        pcurve: Some(PcurveGeometry::Line {
                            origin: Point2::new(0.0, 0.0),
                            direction: Point2::new(std::f64::consts::TAU, 0.0),
                        }),
                    },
                    IntcurveSupportSide {
                        surface: Some(target.clone()),
                        pcurve_parameter_range: None,
                        pcurve: None,
                    },
                ],
                parameter_range: [0.0, 1.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: None,
    });
    ir.model.edges.push(Edge {
        id: EdgeId("synthetic:edge".into()),
        curve: Some(curve),
        start: VertexId("synthetic:start".into()),
        end: VertexId("synthetic:end".into()),
        param_range: Some([0.0, 1.0]),
        tolerance: Some(0.01),
    });

    crate::decode::complete_intersection_pcurves_from_opposite_charts(&mut ir);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    let pcurve = context.sides[1].pcurve.as_ref().unwrap();
    let PcurveGeometry::Nurbs { control_points, .. } = pcurve else {
        unreachable!()
    };
    assert!(control_points.len() > 2);
    for parameter in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let uv = cadmpeg_ir::eval::pcurve_uv(pcurve, parameter).unwrap();
        let point =
            cadmpeg_ir::eval::surface_point(&ir.model.surfaces[1].geometry, uv.u, uv.v).unwrap();
        let angle = std::f64::consts::TAU * parameter;
        assert!((point.x - 10.0 * angle.cos()).abs() < 0.01);
        assert!((point.y - 10.0 * angle.sin()).abs() < 0.01);
        assert!(point.z.abs() < 0.01);
    }
}

#[test]
fn blend_boundary_chart_uses_the_solved_curve_when_the_source_blend_is_unevaluable() {
    use cadmpeg_ir::geometry::{
        BlendSupport, Curve, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve,
        ProceduralSurface, Surface,
    };
    use cadmpeg_ir::ids::{
        CurveId, EdgeId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::Edge;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let source = SurfaceId("synthetic:unevaluable-source-blend".into());
    let other_support = SurfaceId("synthetic:other-support".into());
    let target = SurfaceId("synthetic:target-blend".into());
    let target_construction = ProceduralSurfaceId("synthetic:target-blend-construction".into());
    ir.model.surfaces.extend([
        Surface {
            id: source.clone(),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        },
        Surface {
            id: other_support.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
        Surface {
            id: target.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: target_construction.clone(),
            },
            source_object: None,
        },
    ]);
    let spine = CurveId("synthetic:target-spine".into());
    ir.model.curves.push(Curve {
        id: spine.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface {
        id: target_construction,
        surface: target.clone(),
        definition: ProceduralSurfaceDefinition::Blend {
            supports: [
                Some(BlendSupport {
                    surface: source.clone(),
                    reversed: false,
                }),
                Some(BlendSupport {
                    surface: other_support,
                    reversed: false,
                }),
            ],
            spine: Some(spine),
            radius: BlendRadiusLaw::Constant { signed_radius: 2.0 },
            cross_section: BlendCrossSection::Circular,
            native: None,
        },
        cache_fit_tolerance: None,
        record_bounds: None,
    });

    let curve = CurveId("synthetic:solved-boundary".into());
    let construction = ProceduralCurveId("synthetic:boundary-intersection".into());
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(2.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    });
    ir.model.procedural_curves.push(ProceduralCurve {
        id: construction,
        curve: curve.clone(),
        definition: ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext {
                sides: [
                    IntcurveSupportSide {
                        surface: Some(source),
                        pcurve_parameter_range: None,
                        pcurve: Some(PcurveGeometry::Line {
                            origin: Point2::new(0.0, 0.0),
                            direction: Point2::new(1.0, 0.0),
                        }),
                    },
                    IntcurveSupportSide {
                        surface: Some(target),
                        pcurve_parameter_range: None,
                        pcurve: None,
                    },
                ],
                parameter_range: [0.0, 1.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: None,
    });
    ir.model.edges.push(Edge {
        id: EdgeId("synthetic:boundary-edge".into()),
        curve: Some(curve),
        start: VertexId("synthetic:boundary-start".into()),
        end: VertexId("synthetic:boundary-end".into()),
        param_range: Some([0.0, 1.0]),
        tolerance: Some(1.0e-8),
    });

    crate::decode::complete_intersection_pcurves_from_opposite_charts(&mut ir);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    let PcurveGeometry::Nurbs { control_points, .. } = context.sides[1].pcurve.as_ref().unwrap()
    else {
        unreachable!()
    };
    assert_eq!(control_points.first(), Some(&Point2::new(0.0, 0.0)));
    assert_eq!(control_points.last(), Some(&Point2::new(1.0, 0.0)));
}

#[test]
fn tolerant_nurbs_boundary_establishes_both_intersection_charts() {
    use cadmpeg_ir::geometry::{
        Curve, IntcurveSupportContext, IntcurveSupportSide, NurbsSurface, ProceduralCurve, Surface,
    };
    use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, ProceduralCurveId, SurfaceId, VertexId};
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::{Edge, Point, Vertex};

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let nurbs = SurfaceId("synthetic:nurbs-boundary".into());
    let plane = SurfaceId("synthetic:boundary-plane".into());
    ir.model.surfaces.extend([
        Surface {
            id: nurbs.clone(),
            geometry: SurfaceGeometry::Nurbs(NurbsSurface {
                u_degree: 1,
                v_degree: 1,
                u_knots: vec![0.0, 0.0, 1.0, 1.0],
                v_knots: vec![0.0, 0.0, 1.0, 1.0],
                u_count: 2,
                v_count: 2,
                control_points: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(0.0, 5.0, 0.0),
                    Point3::new(10.0, 0.0, 0.0),
                    Point3::new(10.0, 5.0, 0.0),
                ],
                weights: None,
                u_periodic: false,
                v_periodic: false,
            }),
            source_object: None,
        },
        Surface {
            id: plane.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
    ]);
    let curve = CurveId("synthetic:boundary-curve".into());
    let construction = ProceduralCurveId("synthetic:boundary-intersection".into());
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Procedural {
            construction: construction.clone(),
        },
        source_object: None,
    });
    ir.model.procedural_curves.push(ProceduralCurve {
        id: construction,
        curve: curve.clone(),
        definition: ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext {
                sides: [
                    IntcurveSupportSide {
                        surface: Some(nurbs),
                        pcurve_parameter_range: None,
                        pcurve: None,
                    },
                    IntcurveSupportSide {
                        surface: Some(plane),
                        pcurve_parameter_range: None,
                        pcurve: None,
                    },
                ],
                parameter_range: [0.0, 1.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: None,
    });
    let point_ids = [
        PointId("synthetic:p0".into()),
        PointId("synthetic:p1".into()),
    ];
    let vertex_ids = [
        VertexId("synthetic:v0".into()),
        VertexId("synthetic:v1".into()),
    ];
    ir.model.points.extend([
        Point {
            id: point_ids[0].clone(),
            position: Point3::new(0.0, 0.0, 0.0),
            source_object: None,
        },
        Point {
            id: point_ids[1].clone(),
            position: Point3::new(10.0, 0.0, 0.0),
            source_object: None,
        },
    ]);
    ir.model.vertices.extend([
        Vertex {
            id: vertex_ids[0].clone(),
            point: point_ids[0].clone(),
            tolerance: Some(1.0e-8),
        },
        Vertex {
            id: vertex_ids[1].clone(),
            point: point_ids[1].clone(),
            tolerance: Some(1.0e-8),
        },
    ]);
    ir.model.edges.push(Edge {
        id: EdgeId("synthetic:boundary-edge".into()),
        curve: Some(curve),
        start: vertex_ids[0].clone(),
        end: vertex_ids[1].clone(),
        param_range: Some([0.0, 1.0]),
        tolerance: Some(1.0e-8),
    });

    crate::decode::complete_isoparametric_intersection_pcurves(&mut ir);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    assert!(context.sides.iter().all(|side| side.pcurve.is_some()));
    for parameter in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let points = context.sides.each_ref().map(|side| {
            let uv = cadmpeg_ir::eval::pcurve_uv(side.pcurve.as_ref().unwrap(), parameter).unwrap();
            let surface = ir
                .model
                .surfaces
                .iter()
                .find(|surface| Some(&surface.id) == side.surface.as_ref())
                .unwrap();
            cadmpeg_ir::eval::surface_point(&surface.geometry, uv.u, uv.v).unwrap()
        });
        assert!((points[0].x - 10.0 * parameter).abs() < 1.0e-8);
        assert!(
            (points[0].x - points[1].x)
                .hypot(points[0].y - points[1].y)
                .hypot(points[0].z - points[1].z)
                < 1.0e-8
        );
    }
}

#[test]
fn decode_attaches_dimension_two_bcurve_through_surface_curve() {
    let stream = pcurve_topology_partition_stream();
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.pcurves.len(), 1);
    assert_eq!(
        result.ir.model.coedges[0]
            .pcurves
            .first()
            .map(|pcurve| &pcurve.pcurve),
        Some(&result.ir.model.pcurves[0].id)
    );
    let PcurveGeometry::Nurbs {
        degree,
        knots,
        control_points,
        weights,
        periodic,
    } = &result.ir.model.pcurves[0].geometry
    else {
        panic!("expected NURBS pcurve");
    };
    assert_eq!(*degree, 1);
    assert_eq!(knots, &[0.0, 0.0, 1.0, 1.0]);
    assert_eq!(
        control_points,
        &[Point2::new(10.0, 20.0), Point2::new(10.0, 20.0)]
    );
    assert!(weights.is_none());
    assert!(!periodic);
    assert_eq!(result.ir.model.pcurves[0].fit_tolerance, Some(0.01));
    assert_eq!(
        result.ir.model.points[0].position,
        cadmpeg_ir::math::Point3::new(10.0, 20.0, 0.0)
    );
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(
        validation.findings.is_empty(),
        "findings: {:?}",
        validation.findings
    );
}

#[test]
fn decode_omits_surface_curve_missing_tolerance_sentinel() {
    let mut stream = pcurve_topology_partition_stream();
    let surface_curve = stream
        .windows(2)
        .position(|window| window == [0, 137])
        .expect("surface curve");
    put_f64(
        &mut stream,
        surface_curve + 25,
        crate::decode::MISSING_TOLERANCE,
    );
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.pcurves[0].fit_tolerance, None);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_rejects_overflowing_pcurve_parameter_conversion() {
    let mut stream = pcurve_topology_partition_stream();
    let payload = stream
        .windows(4)
        .position(|window| window == [0, 135, 0, 22])
        .expect("pcurve payload");
    put_f64(&mut stream, payload + 15, f64::MAX);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert!(result.ir.model.pcurves.is_empty());
    assert!(result.ir.model.coedges[0].pcurves.is_empty());
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_preserves_multiple_shells_in_one_region() {
    let stream = shared_region_shells_partition_stream();
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.regions.len(), 1);
    assert_eq!(result.ir.model.shells.len(), 2);
    assert_eq!(result.ir.model.regions[0].shells.len(), 2);
    assert_eq!(result.ir.model.bodies[0].regions.len(), 1);
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn nx_offset_surface_accepts_unbounded_representable_distance() {
    let mut stream = offset_surface_topology_partition_stream();
    let offset = stream
        .windows(4)
        .position(|window| window == [0, 60, 0, 12])
        .expect("offset record");
    put_f64(&mut stream, offset + 23, 1_001.0);
    let surfaces = crate::topology::offset_surfaces(&stream);
    let [surface] = surfaces.as_slice() else {
        panic!("offset surface")
    };
    assert_eq!(surface.distance, 1_001_000.0);

    put_f64(&mut stream, offset + 23, f64::INFINITY);
    assert!(crate::topology::offset_surfaces(&stream).is_empty());

    put_f64(&mut stream, offset + 23, f64::MAX);
    assert!(crate::topology::offset_surfaces(&stream).is_empty());
}

#[test]
fn offset_surface_envelope_does_not_consume_the_following_record() {
    let mut stream = offset_surface_topology_partition_stream();
    let offset_end = stream.len();
    let mut point = record(29, 40);
    put_ref(&mut point, 2, 20);
    put_vec3(&mut point, 16, [0.001, 0.002, 0.003]);
    stream.extend(point);

    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(
        graph.get(60, 12).map(crate::topology::Node::end),
        Some(offset_end)
    );
    assert!(graph.get(29, 20).is_some());
}

#[test]
fn nx_blend_surface_requires_a_nonzero_rolling_ball_radius() {
    let mut stream = blend_surface_topology_partition_stream();
    let blend = stream
        .windows(4)
        .position(|window| window == [0, 56, 0, 12])
        .expect("blend record");
    put_f64(&mut stream, blend + 26, 0.0);
    put_f64(&mut stream, blend + 34, 0.0);
    assert!(crate::topology::blend_surfaces(&stream).is_empty());

    put_f64(&mut stream, blend + 26, 0.5e-9);
    assert!(crate::topology::blend_surfaces(&stream).is_empty());

    put_f64(&mut stream, blend + 26, f64::MAX);
    put_f64(&mut stream, blend + 34, f64::MAX);
    assert!(crate::topology::blend_surfaces(&stream).is_empty());
}

#[test]
fn detect_high_on_magic() {
    assert_eq!(NxCodec.detect(MAGIC), Confidence::High);
    assert_eq!(NxCodec.detect(&single_part_prt()), Confidence::High);
    assert_eq!(NxCodec.detect(b"PK\x03\x04 not nx"), Confidence::No);
    // A Creo/Granite .prt shares the extension but not the magic.
    assert_eq!(NxCodec.detect(b"\xe0\x02\xff\xfeGRANITE"), Confidence::No);
}

#[test]
fn container_parses_header_and_directory() {
    let c = container::scan_bytes(single_part_prt()).unwrap();
    assert_eq!(c.version, 0x06);
    assert_eq!(c.file_tag, 0x33_22_11);
    assert!(c
        .entries
        .iter()
        .any(|e| e.name == "/Root/UG_PART/UG_PART" && e.file_span.is_some()));
}

#[test]
fn inspect_reports_bounded_nx_object_model_entities() {
    let mut cur = Cursor::new(prt_with_indexed_om_section());
    let summary = NxCodec
        .inspect(&mut cur, &InspectOptions::default())
        .unwrap();
    assert!(summary.notes.iter().any(|note| {
        note == "NX object model: 1 indexed section(s), 2 bounded entity record(s)"
    }));
}

#[test]
fn decode_projects_part_attributes_to_document_attributes() {
    let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<UgAttributes version="4" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <Attribute owner="part" pdmBased="false" utf8title="Material"
    utf8value="Steel" version="3" xsi:type="StringAttributeType"/>
</UgAttributes>"#;
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/part/attrs", xml.to_vec()),
    ]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.attributes.len(), 1);
    let attribute = &result.ir.model.attributes[0];
    assert_eq!(attribute.name, "Material");
    assert_eq!(
        attribute.target,
        cadmpeg_ir::attributes::AttributeTarget::Document
    );
    assert_eq!(
        attribute.values,
        vec![cadmpeg_ir::attributes::AttributeValue::String(
            "Steel".to_string()
        )]
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_exposes_strict_nx_jpeg_preview_metadata() {
    let preview = [
        0xff, 0xd8, 0xff, 0xe0, 0x00, 0x04, 0x00, 0x00, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0xb9,
        0x00, 0xf7, 0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x00, 0x03, 0x11, 0x00, 0xff, 0xd9,
    ];
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/images/preview", preview.to_vec()),
    ]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let attributes = &result.ir.source.unwrap().attributes;
    assert_eq!(attributes["jpeg_preview_count"], "1");
    assert_eq!(attributes["jpeg_preview_0_width"], "247");
    assert_eq!(attributes["jpeg_preview_0_height"], "185");
    assert_eq!(attributes["jpeg_preview_0_precision"], "8");
    assert_eq!(attributes["jpeg_preview_0_components"], "3");
    assert_eq!(
        attributes["jpeg_preview_0_byte_len"],
        preview.len().to_string()
    );

    let mut malformed = preview;
    malformed[10..12].copy_from_slice(&16u16.to_be_bytes());
    assert!(crate::decode::jpeg_dimensions(&malformed).is_none());
}

#[test]
fn decode_rejects_repeated_nx_arrangement_terminators_atomically() {
    let mut arrangements =
        br#"<Arrangements><Arrangement Default="YES" Name="Model"/></Arrangements>"#.to_vec();
    arrangements.extend_from_slice(&[0, 0]);
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/part/arrangements", arrangements),
    ]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir.model.configurations.is_empty());
}

#[test]
fn parasolid_extraction_classifies_partition_and_schema() {
    let f = single_part_prt();
    let streams = extract_streams(&f);
    let part = streams
        .iter()
        .find(|s| s.kind == StreamKind::Partition)
        .expect("a partition stream");
    assert_eq!(part.schema.as_deref(), Some("SCH_TEST_1_9999"));
    assert!(part.inflated.starts_with(b"PS\x00\x00"));
}

#[test]
fn decode_transfers_point_plane_cylinder_line() {
    let mut cur = Cursor::new(single_part_prt());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.report.geometry_transferred);
    assert_eq!(result.ir.model.points.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 1);
    // Point coordinate is scaled metres → millimetres, byte-exact.
    let p = &result.ir.model.points[0].position;
    assert!((p.x - 62.5).abs() < 1e-6 && (p.z - 12.7).abs() < 1e-6);

    // One plane, one cylinder decoded.
    let planes = result
        .ir
        .model
        .surfaces
        .iter()
        .filter(|s| matches!(s.geometry, SurfaceGeometry::Plane { .. }))
        .count();
    let cyls: Vec<_> = result
        .ir
        .model
        .surfaces
        .iter()
        .filter_map(|s| match &s.geometry {
            SurfaceGeometry::Cylinder { radius, .. } => Some(*radius),
            _ => None,
        })
        .collect();
    assert_eq!(planes, 1);
    assert_eq!(cyls.len(), 1);
    assert!((cyls[0] - 4.05).abs() < 1e-6);
    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Plane {
            u_axis: axis,
            ..
        } if axis == Vector3::new(1.0, 0.0, 0.0)
    )));
    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Cylinder {
            ref_direction: direction,
            ..
        } if direction == Vector3::new(1.0, 0.0, 0.0)
    )));

    // One line decoded, with a unit direction.
    let lines: Vec<_> = result
        .ir
        .model
        .curves
        .iter()
        .filter(|c| matches!(c.geometry, CurveGeometry::Line { .. }))
        .collect();
    assert_eq!(lines.len(), 1);

    // No topology graph is fabricated; the loss is reported as blocking.
    assert!(result.ir.model.faces.is_empty() && result.ir.model.edges.is_empty());
    assert!(result
        .report
        .losses
        .iter()
        .any(|l| l.category == cadmpeg_ir::report::LossCategory::Topology
            && l.severity == cadmpeg_ir::report::Severity::Blocking));

    // The Parasolid stream is preserved verbatim.
    let unknowns = result.ir.native_unknowns("nx").unwrap();
    assert_eq!(unknowns.len(), 1);
    assert_eq!(result.source_fidelity.retained_records[0].sha256.len(), 64);
    assert_eq!(
        unknowns[0].links,
        ["nx:s0:surf#0", "nx:s0:surf#1", "nx:s0:crv#0",]
    );
    assert_eq!(
        result.source_fidelity.annotations.exactness[&unknowns[0].id.to_string()].fields["links"],
        Exactness::Derived
    );

    // The preserved stream owns partial-decode carriers without fabricating topology.
    let report = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(report.is_ok(), "findings: {:?}", report.findings);
}

#[test]
fn decode_emits_connected_primitive_brep() {
    let mut cur = Cursor::new(topology_part_prt());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.regions.len(), 1);
    assert_eq!(result.ir.model.shells.len(), 1);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert_eq!(
        result.ir.model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Sheet
    );
    assert_eq!(
        result.ir.model.faces[0].loops,
        vec![result.ir.model.loops[0].id.clone()]
    );
    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.curves[0].id)
    );
    assert_eq!(result.ir.model.vertices[0].tolerance, Some(0.1));
    assert_eq!(result.ir.model.edges[0].tolerance, Some(0.3));
    assert_eq!(result.ir.model.faces[0].tolerance, Some(0.2));
    assert_eq!(
        result.ir.model.coedges[0].radial_next,
        result.ir.model.coedges[0].id
    );
    assert!(result
        .report
        .losses
        .iter()
        .all(|loss| loss.category != cadmpeg_ir::report::LossCategory::Topology));
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn offset_surface_parameter_solver_preserves_support_parameters() {
    let stream = offset_surface_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let surface = result.ir.model.procedural_surfaces[0].surface.clone();
    let expected = Point2::new(12.0, 7.0);
    let point =
        cadmpeg_ir::eval::model_surface_point_by_id(&result.ir, &surface, expected.u, expected.v)
            .unwrap();

    let actual =
        crate::decode::offset_surface_parameters(&result.ir, &surface, point, None).unwrap();

    assert!((actual.u - expected.u).abs() < 1.0e-8);
    assert!((actual.v - expected.v).abs() < 1.0e-8);
}

#[test]
fn offset_surface_parameter_solver_accepts_a_seed_within_fit_tolerance() {
    let stream = offset_surface_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let surface = result.ir.model.procedural_surfaces[0].surface.clone();
    let seed = Point2::new(12.0, 7.0);
    let mut point =
        cadmpeg_ir::eval::model_surface_point_by_id(&result.ir, &surface, seed.u, seed.v).unwrap();
    point.x += 0.01;

    let actual = crate::decode::offset_surface_parameters_with_tolerance(
        &result.ir,
        &surface,
        point,
        Some(seed),
        Some(0.02),
    )
    .unwrap();

    assert_eq!(actual, seed);
}

#[test]
fn decode_tracks_fully_extended_offset_common_header() {
    let stream = offset_surface_with_fully_extended_common_header();
    assert_eq!(crate::topology::offset_surfaces(&stream).len(), 1);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let procedural = result
        .ir
        .model
        .procedural_surfaces
        .first()
        .expect("offset surface");
    let ProceduralSurfaceDefinition::Offset {
        support, distance, ..
    } = &procedural.definition
    else {
        panic!("offset definition");
    };
    assert_eq!(*distance, 2.5);
    assert_ne!(procedural.surface, *support);
    assert_eq!(result.ir.model.faces[0].surface, procedural.surface);
}

#[test]
fn decode_tracks_fully_extended_compact_geometry_headers() {
    let mut blend = blend_surface_topology_partition_stream();
    fully_extend_common_header(&mut blend, [0, 56, 0, 12]);
    assert_eq!(crate::topology::blend_surfaces(&blend).len(), 1);

    let mut intersection = intersection_curve_topology_partition_stream();
    fully_extend_common_header(&mut intersection, [0, 38, 0, 12]);
    assert_eq!(crate::topology::composite_curves(&intersection).len(), 1);

    let mut surface_curve = surface_curve_topology_partition_stream();
    fully_extend_common_header(&mut surface_curve, [0, 137, 0, 12]);
    let surface_curves = crate::topology::surface_curves(&surface_curve);
    assert_eq!(surface_curves.len(), 1);
    assert_eq!(surface_curves[0].xmt, 12);
    assert_eq!(surface_curves[0].pcurve, 9);

    let mut trimmed = trimmed_topology_partition_stream();
    fully_extend_common_header(&mut trimmed, [0, 133, 0, 12]);
    let trims = crate::topology::trimmed_curves(&trimmed);
    assert_eq!(trims.len(), 1);
    assert_eq!(trims[0].parameters, [0.000_25, 0.000_75]);

    let mut bspline = bspline_partition_stream();
    fully_extend_common_header(&mut bspline, [0, 124, 0, 10]);
    fully_extend_common_header(&mut bspline, [0, 134, 0, 50]);
    let mut cur = Cursor::new(prt_with_partition(&bspline));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert!(result
        .ir
        .model
        .surfaces
        .iter()
        .any(|surface| matches!(surface.geometry, SurfaceGeometry::Nurbs(_))));
    assert!(result
        .ir
        .model
        .curves
        .iter()
        .any(|curve| matches!(curve.geometry, CurveGeometry::Nurbs(_))));
}

#[test]
fn intersection_construction_recovers_one_missing_term_from_unique_edge_endpoints() {
    let mut stream = charted_intersection_with_edge_endpoint_witnesses_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 25, 1);
    let scan = crate::intersection::scan(&stream);
    assert_eq!(scan.constructions.len(), 1);
    assert_eq!(scan.curves.len(), 1);
    assert_eq!(
        scan.rejected,
        crate::intersection::RejectionCounts::default()
    );
}

#[test]
fn intersection_construction_rejects_missing_term_without_topology_endpoint_match() {
    let mut stream = charted_intersection_with_edge_endpoint_witnesses_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 25, 1);
    let chart = stream
        .windows(8)
        .position(|window| window == [0, 40, 0, 0, 0, 2, 0, 20])
        .expect("chart record");
    put_f64(&mut stream, chart + 60, 0.005);

    let scan = crate::intersection::scan(&stream);
    assert_eq!(scan.constructions.len(), 1);
    assert!(scan.curves.is_empty());
    assert_eq!(scan.rejected.missing_start_term, 1);
}

#[test]
fn intersection_auxiliaries_reject_duplicate_identities() {
    fn append_record(stream: &mut Vec<u8>, marker: &[u8], len: usize) {
        let start = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("auxiliary record");
        let duplicate = stream[start..start + len].to_vec();
        stream.extend(duplicate);
    }

    let mut chart = charted_intersection_curve_topology_partition_stream();
    append_record(&mut chart, &[0, 40, 0, 0, 0, 2, 0, 20], 108);
    let scan = crate::intersection::scan(&chart);
    assert!(scan.curves.is_empty());
    assert_eq!(scan.rejected.missing_chart, 1);
    assert_eq!(
        crate::intersection::scan_with_auxiliary_replacements(
            &chart,
            &chart[..chart.len() - 108],
            &[&chart[chart.len() - 108..]],
        )
        .curves
        .len(),
        1
    );

    let base_term = charted_intersection_curve_topology_partition_stream();
    let mut term = base_term.clone();
    append_record(&mut term, &[0, 41, 0, 0, 0, 1, 0, 21], 34);
    assert_eq!(crate::intersection::term_use_records(&term).len(), 1);
    let scan = crate::intersection::scan(&term);
    assert!(scan.curves.is_empty());
    assert_eq!(scan.rejected.missing_start_term, 1);
    assert_eq!(
        crate::intersection::scan_with_auxiliary_replacements(
            &term,
            &base_term,
            &[&term[base_term.len()..]],
        )
        .curves
        .len(),
        1
    );

    let mut uv = charted_intersection_curve_topology_partition_stream();
    append_record(&mut uv, &[0, 204, 0, 0, 0, 4, 0, 23], 41);
    assert!(crate::intersection::support_uv_records(&uv).is_empty());
    let [curve] = crate::intersection::scan(&uv).curves.try_into().unwrap();
    assert_eq!(curve.support_uv, [None, None]);

    let mut blend_bound = blend_bound_charted_intersection_curve_stream();
    append_record(&mut blend_bound, &[0, 59, 0, 14], 24);
    assert!(crate::intersection::blend_bounds(&blend_bound).is_empty());
}

#[test]
fn intersection_chart_accepts_one_matching_parameter_complement() {
    let ext11 = ext11_charted_intersection_curve_stream();
    let ext11_start = ext11
        .windows(8)
        .position(|window| window == [0, 40, 0, 0, 0, 2, 0, 20])
        .expect("ext11 chart");
    let complement = ext11[ext11_start..ext11_start + 236].to_vec();

    let mut stream = charted_intersection_curve_topology_partition_stream();
    stream.extend_from_slice(&complement);
    let [curve] = crate::intersection::scan(&stream)
        .curves
        .try_into()
        .expect("complemented curve");
    assert_eq!(curve.parameters, [2.0, 5.0]);

    stream.extend_from_slice(&complement);
    let scan = crate::intersection::scan(&stream);
    assert!(scan.curves.is_empty());
    assert_eq!(scan.rejected.missing_chart, 1);
}

#[test]
fn decode_lifts_pcurve_only_fin_carrier_to_its_surface() {
    let mut stream = pcurve_topology_partition_stream();
    let edge = stream
        .windows(4)
        .position(|window| window == [0, 16, 0, 8])
        .expect("edge record");
    put_ref(&mut stream, edge + 24, 1);
    let surface_curve = stream
        .windows(4)
        .position(|window| window == [0, 137, 0, 25])
        .expect("surface curve");
    put_ref(&mut stream, surface_curve + 23, 1);

    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let carrier = result.ir.model.edges[0]
        .curve
        .as_ref()
        .and_then(|id| result.ir.model.curves.iter().find(|curve| &curve.id == id))
        .expect("lifted carrier");
    assert!(matches!(carrier.geometry, CurveGeometry::Procedural { .. }));
    let ProceduralCurveDefinition::SurfaceCurve {
        family: cadmpeg_ir::geometry::SurfaceCurveFamily::Parametric,
        context,
        ..
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("parametric surface curve");
    };
    assert_eq!(
        context.sides[0].surface,
        Some(result.ir.model.faces[0].surface.clone())
    );
    assert!(context.sides[0].pcurve.is_some());
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_emits_blend_with_extended_support_reference() {
    let stream = blend_surface_with_extended_support_reference();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.procedural_surfaces.len(), 1);
    assert_eq!(
        result.ir.model.faces[0].surface,
        result.ir.model.procedural_surfaces[0].surface
    );
}

#[test]
fn decode_binds_blend_ball_centre_spine() {
    let stream = blend_surface_with_intersection_spine();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let ProceduralSurfaceDefinition::Blend { spine, .. } =
        &result.ir.model.procedural_surfaces[0].definition
    else {
        panic!("blend definition");
    };
    assert_eq!(
        spine.as_ref(),
        Some(&result.ir.model.procedural_curves[0].curve)
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_resolves_forward_blend_support_reference() {
    let stream = blend_surface_with_forward_blend_support();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.procedural_surfaces.len(), 2);
    let ProceduralSurfaceDefinition::Blend { supports, .. } =
        &result.ir.model.procedural_surfaces[0].definition
    else {
        panic!("blend definition");
    };
    assert_eq!(
        supports[0].as_ref().map(|support| &support.surface),
        Some(&result.ir.model.procedural_surfaces[1].surface)
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_reports_status_framed_deltas_records_and_tombstones() {
    let stream = status_framed_deltas_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let attributes = &result.ir.source.expect("source metadata").attributes;

    assert_eq!(
        attributes.get("deltas.0.full.FACE").map(String::as_str),
        Some("1")
    );
    assert_eq!(
        attributes
            .get("deltas.0.tombstone.EDGE")
            .map(String::as_str),
        Some("1")
    );
    assert_eq!(
        attributes.get("deltas.0.grammar").map(String::as_str),
        Some("status_byte_framed_topology")
    );
}

#[test]
fn decode_accepts_exact_loop_and_rejects_incomplete_fin_deltas() {
    let stream = variable_status_framed_deltas_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let attributes = &result.ir.source.expect("source metadata").attributes;

    assert!(!attributes.contains_key("deltas.0.full.FIN"));
    assert_eq!(
        attributes.get("deltas.0.full.LOOP").map(String::as_str),
        Some("1")
    );
}

#[test]
fn deltas_point_normalizes_to_partition_record_framing() {
    let record = crate::deltas::walk(&status_framed_deltas_point_stream())
        .records
        .remove(0);
    let mut expected = crate::tests::record(29, 40);
    put_ref(&mut expected, 2, 50);
    expected[4..8].copy_from_slice(&900u32.to_be_bytes());
    for at in [8, 10, 12, 14] {
        put_ref(&mut expected, at, 1);
    }
    put_vec3(&mut expected, 16, [0.0125, -0.002, 0.004]);
    assert_eq!(record.canonical_bytes, expected);
}

#[test]
fn deltas_intersection_normalizes_before_partition_style_decode() {
    let residual = crate::deltas::procedural_residual(&status_framed_deltas_intersection_stream());
    let intersections = crate::topology::composite_curves(&residual);
    assert_eq!(intersections.len(), 1);
    assert_eq!(intersections[0].xmt, 12);
    assert_eq!(intersections[0].references, [6, 7, 20, 21, 22, 23]);
}

#[test]
fn deltas_offset_surface_normalizes_exact_record_envelope() {
    let stream = deltas_offset_surface_partition_stream();
    let record = crate::deltas::walk(&stream).records.remove(0);
    assert_eq!(record.canonical_bytes.len(), 31);
    assert_eq!(
        crate::topology::offset_surfaces(&record.canonical_bytes)[0].distance,
        4.5
    );

    let mut invalid_status = stream.clone();
    let offset = invalid_status
        .windows(4)
        .position(|window| window == [0, 60, 0, 12])
        .expect("OFFSET_SURF record");
    invalid_status[offset + 28] = 0;
    assert!(!crate::deltas::walk(&invalid_status)
        .records
        .iter()
        .any(|record| record.kind == 60));

    let mut truncated = stream;
    truncated.pop();
    assert!(!crate::deltas::walk(&truncated)
        .records
        .iter()
        .any(|record| record.kind == 60));
}

#[test]
fn deltas_procedural_wrappers_normalize_complete_record_envelopes() {
    for (stream, family, kind, byte_len) in [
        (
            deltas_blend_surface_partition_stream(),
            "BLEND_SURF",
            56,
            66,
        ),
        (
            deltas_trimmed_curve_partition_stream(),
            "TRIMMED_CURVE",
            133,
            85,
        ),
        (deltas_surface_curve_partition_stream(), "SP_CURVE", 137, 33),
    ] {
        let census = crate::deltas::walk(&stream);
        assert_eq!(census.full_counts.get(family), Some(&1));
        let record = census
            .records
            .iter()
            .find(|record| record.kind == kind)
            .expect("procedural wrapper");
        assert_eq!(record.canonical_bytes.len(), byte_len);
        assert!(crate::topology::Graph::parse(&record.canonical_bytes)
            .get(kind as u8, 12)
            .is_some());
    }

    let mut invalid_blend = deltas_blend_surface_partition_stream();
    let blend = invalid_blend
        .windows(4)
        .position(|window| window == [0, 56, 0, 12])
        .expect("BLEND_SURF record");
    invalid_blend[blend + 24] = b'X';
    assert!(!crate::deltas::walk(&invalid_blend)
        .records
        .iter()
        .any(|record| record.kind == 56));
}

#[test]
fn merged_deltas_full_record_replaces_partition_node() {
    let partition = topology_partition_stream();
    let mut deltas = status_framed_deltas_point_stream();
    deltas[2..4].copy_from_slice(&11u16.to_be_bytes());
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    let points = crate::geometry::points(&merged);
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].position.x, 12.5);
    assert_eq!(points[0].position.y, -2.0);
    assert_eq!(points[0].position.z, 4.0);
    assert!(crate::topology::Graph::parse(&merged).get(29, 11).is_some());
}

#[test]
fn merged_tombstone_preserves_a_topology_referenced_carrier() {
    let partition = topology_partition_stream();
    let mut tombstone = Vec::new();
    tombstone.extend_from_slice(&29u16.to_be_bytes());
    tombstone.extend_from_slice(&11u16.to_be_bytes());
    tombstone.extend_from_slice(&[0, 1]);
    let census = crate::deltas::walk(&tombstone);
    assert_eq!(census.tombstones.len(), 1);
    assert_eq!(census.tombstones[0].kind, 29);
    assert_eq!(census.tombstones[0].xmt, 11);
    let merged = crate::deltas::merge_full_records(&partition, &tombstone);
    assert!(crate::topology::Graph::parse(&merged).get(29, 11).is_some());
    assert_eq!(crate::geometry::points(&merged)[0].position.x, 10.0);
}

#[test]
fn merged_exact_key_tombstone_removes_unreferenced_partition_node() {
    let mut partition = record(29, 40);
    put_ref(&mut partition, 2, 11);
    put_vec3(&mut partition, 16, [0.01, 0.02, 0.03]);
    let tombstone = [0, 29, 0, 11, 0, 1];
    let merged = crate::deltas::merge_full_records(&partition, &tombstone);
    assert!(crate::topology::Graph::parse(&merged).get(29, 11).is_none());
}

#[test]
fn merged_deltas_uses_last_full_or_tombstone_event() {
    let partition = topology_partition_stream();
    let tombstone = [0, 29, 0, 11, 0, 1];
    let mut full = status_framed_deltas_point_stream();
    full[2..4].copy_from_slice(&11u16.to_be_bytes());

    let mut delete_then_replace = tombstone.to_vec();
    delete_then_replace.extend_from_slice(&full);
    let merged = crate::deltas::merge_full_records(&partition, &delete_then_replace);
    assert_eq!(crate::geometry::points(&merged)[0].position.x, 12.5);

    let mut replace_then_delete = full;
    replace_then_delete.extend_from_slice(&tombstone);
    let merged = crate::deltas::merge_full_records(&partition, &replace_then_delete);
    assert_eq!(crate::geometry::points(&merged)[0].position.x, 10.0);
}

#[test]
fn unmatched_delta_tombstones_follow_exact_last_event_identity() {
    let partition = topology_partition_stream();
    let known = [0, 29, 0, 11, 0, 1];
    let unknown = [0, 29, 0, 99, 0, 1];
    assert_eq!(
        crate::deltas::unmatched_terminal_tombstones(&partition, &known),
        0
    );
    assert_eq!(
        crate::deltas::unmatched_terminal_tombstones(&partition, &unknown),
        1
    );

    let mut full = status_framed_deltas_point_stream();
    full[2..4].copy_from_slice(&99u16.to_be_bytes());
    let mut add_then_delete = full.clone();
    add_then_delete.extend_from_slice(&unknown);
    assert_eq!(
        crate::deltas::unmatched_terminal_tombstones(&partition, &add_then_delete),
        0
    );

    let mut delete_then_add = unknown.to_vec();
    delete_then_add.extend_from_slice(&full);
    assert_eq!(
        crate::deltas::unmatched_terminal_tombstones(&partition, &delete_then_add),
        0
    );
}

#[test]
fn decode_emits_point_added_by_deltas_stream() {
    let mut cur = Cursor::new(prt_with_partition(&deltas_point_partition_stream()));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.points.len(), 1);
    assert_eq!(result.ir.model.points[0].position.x, 12.5);
    assert_eq!(result.ir.model.points[0].position.y, -2.0);
    assert_eq!(result.ir.model.points[0].position.z, 4.0);
}

#[test]
fn decode_replaces_partition_point_with_same_xmt_deltas_point() {
    let partition = topology_partition_stream();
    let mut deltas = deltas_point_partition_stream();
    let record = deltas
        .windows(2)
        .rposition(|window| window == 29u16.to_be_bytes())
        .expect("deltas POINT");
    deltas[record + 2..record + 4].copy_from_slice(&11u16.to_be_bytes());
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.points.len(), 1);
    assert_eq!(result.ir.model.points[0].position.x, 12.5);
    assert_eq!(result.ir.model.points[0].position.y, -2.0);
    assert_eq!(result.ir.model.points[0].position.z, 4.0);
}

#[test]
fn decode_preserves_partition_edge_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_edge_partition_stream();
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.edges[0].tolerance, Some(0.3));
    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.curves[0].id)
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_preserves_partition_face_and_vertex_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_face_vertex_partition_stream();
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.faces[0].tolerance, Some(0.2));
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert_eq!(result.ir.model.vertices[0].tolerance, Some(0.1));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_preserves_partition_loop_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_loop_partition_stream();
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::Graph::parse(&merged)
            .get(15, 5)
            .and_then(|node| node.u32_at(4)),
        Some(0)
    );
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 1);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_preserves_partition_shell_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_shell_partition_stream();
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::Graph::parse(&merged)
            .get(13, 3)
            .and_then(|node| node.u32_at(4)),
        Some(0)
    );
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.shells.len(), 1);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_preserves_partition_fin_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_fin_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.coedges.len(), 1);
    assert_eq!(
        result.ir.model.coedges[0].sense,
        cadmpeg_ir::topology::Sense::Forward
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_line_from_status_framed_deltas() {
    let partition = topology_partition_stream();
    let deltas = deltas_line_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let CurveGeometry::Line { origin, direction } = result.ir.model.curves[0].geometry else {
        panic!("line");
    };
    assert_eq!(origin, cadmpeg_ir::math::Point3::new(4.0, 5.0, 6.0));
    assert_eq!(direction, Vector3::new(0.0, 1.0, 0.0));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_plane_from_status_framed_deltas() {
    let partition = topology_partition_stream();
    let deltas = deltas_plane_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(matches!(
        result.ir.model.surfaces[0].geometry,
        SurfaceGeometry::Plane { origin, normal, u_axis }
            if origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && normal == Vector3::new(0.0, 1.0, 0.0)
                && u_axis == Vector3::new(1.0, 0.0, 0.0)
    ));
    assert_eq!(
        result.ir.model.faces[0].surface,
        result.ir.model.surfaces[0].id
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_offset_surface_from_status_framed_deltas() {
    let partition = offset_surface_topology_partition_stream();
    let deltas = deltas_offset_surface_partition_stream();
    let census = crate::deltas::walk(&deltas);
    assert_eq!(census.full_counts.get("OFFSET_SURF"), Some(&1));
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::offset_surfaces(&merged)
            .iter()
            .map(|surface| surface.distance)
            .collect::<Vec<_>>(),
        [4.5]
    );
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let [procedural] = result.ir.model.procedural_surfaces.as_slice() else {
        panic!("one offset surface");
    };
    let ProceduralSurfaceDefinition::Offset { distance, .. } = procedural.definition else {
        panic!("offset surface");
    };
    assert_eq!(distance, 4.5);
    assert_eq!(result.ir.model.faces[0].surface, procedural.surface);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_blend_surface_from_status_framed_deltas() {
    let partition = blend_surface_topology_partition_stream();
    let deltas = deltas_blend_surface_partition_stream();
    let result = NxCodec
        .decode(
            &mut Cursor::new(prt_with_streams(&[&partition, &deltas])),
            &DecodeOptions::default(),
        )
        .unwrap();

    let ProceduralSurfaceDefinition::Blend { radius, .. } =
        &result.ir.model.procedural_surfaces[0].definition
    else {
        panic!("blend surface");
    };
    assert_eq!(
        *radius,
        BlendRadiusLaw::Constant {
            signed_radius: -4.0
        }
    );
    assert_eq!(
        result.ir.model.faces[0].surface,
        result.ir.model.procedural_surfaces[0].surface
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_trimmed_curve_from_status_framed_deltas() {
    let partition = trimmed_topology_partition_stream();
    let deltas = deltas_trimmed_curve_partition_stream();
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::trimmed_curves(&merged)[0].parameters,
        [0.000_3, 0.000_7]
    );
    let result = NxCodec
        .decode(
            &mut Cursor::new(prt_with_streams(&[&partition, &deltas])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir.model.edges[0].param_range, Some([0.3, 0.7]));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_surface_curve_from_status_framed_deltas() {
    let partition = surface_curve_topology_partition_stream();
    let deltas = deltas_surface_curve_partition_stream();
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::surface_curves(&merged)[0].tolerance,
        0.000_02
    );
    let result = NxCodec
        .decode(
            &mut Cursor::new(prt_with_streams(&[&partition, &deltas])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.curves[0].id)
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_circle_from_status_framed_deltas() {
    let partition = circle_topology_partition_stream();
    let deltas = deltas_circle_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.curves.iter().any(|curve| matches!(
        curve.geometry,
        CurveGeometry::Circle { center, axis, ref_direction, radius }
            if center == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 25.0
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_ellipse_from_status_framed_deltas() {
    let partition = ellipse_topology_partition_stream();
    let deltas = deltas_ellipse_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.curves.iter().any(|curve| matches!(
        curve.geometry,
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } if center == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
            && axis == Vector3::new(0.0, 1.0, 0.0)
            && major_direction == Vector3::new(1.0, 0.0, 0.0)
            && major_radius == 30.0
            && minor_radius == 12.0
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_cylinder_from_status_framed_deltas() {
    let partition = cylinder_topology_partition_stream();
    let deltas = deltas_cylinder_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Cylinder { origin, axis, ref_direction, radius }
            if origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 25.0
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_cone_from_status_framed_deltas() {
    let partition = cone_topology_partition_stream();
    let deltas = deltas_cone_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Cone { origin, axis, ref_direction, radius, ratio, half_angle }
            if origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 25.0
                && ratio == 1.0
                && (half_angle - std::f64::consts::FRAC_PI_6).abs() < 1e-12
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_sphere_from_status_framed_deltas() {
    let partition = sphere_topology_partition_stream();
    let deltas = deltas_sphere_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Sphere { center, axis, ref_direction, radius }
            if center == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 25.0
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_torus_from_status_framed_deltas() {
    let partition = torus_topology_partition_stream();
    let deltas = deltas_torus_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } if center == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
            && axis == Vector3::new(0.0, 1.0, 0.0)
            && ref_direction == Vector3::new(1.0, 0.0, 0.0)
            && major_radius == 40.0
            && minor_radius == 15.0
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn intersection_pcurve_attachment_requires_face_incidence() {
    let ir = cadmpeg_ir::examples::unit_cube();
    let edge = cadmpeg_ir::ids::EdgeId("synthetic:cube:edge#0".into());
    let surface = ir
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.edge == edge && coedge.id.0.contains("bottom"))
        .and_then(|coedge| {
            let loop_ = ir
                .model
                .loops
                .iter()
                .find(|loop_| loop_.id == coedge.owner_loop)?;
            ir.model
                .faces
                .iter()
                .find(|face| face.id == loop_.face)
                .map(|face| face.surface.clone())
        })
        .expect("bottom support surface");
    let pcurve = |end| PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![Point2::new(0.0, 0.0), end],
        weights: None,
        periodic: false,
    };

    assert!(crate::decode::pcurve_matches_edge(
        &ir,
        &edge,
        &surface,
        &pcurve(Point2::new(10.0, 0.0)),
        None,
    ));
    assert!(!crate::decode::pcurve_matches_edge(
        &ir,
        &edge,
        &surface,
        &pcurve(Point2::new(10.0, 5.0)),
        None,
    ));
}

#[test]
fn decode_derives_analytic_support_uv_without_serialized_values() {
    let stream = charted_intersection_without_uv_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let carrier = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == result.ir.model.procedural_curves[0].curve)
        .expect("intersection carrier");
    assert!(matches!(carrier.geometry, CurveGeometry::Nurbs(_)));
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("intersection definition");
    };
    assert!(context.sides[0].pcurve.is_some());
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_accepts_intersection_terms_within_chart_tolerance() {
    let stream = charted_intersection_with_approximated_term_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let carrier = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == result.ir.model.procedural_curves[0].curve)
        .expect("intersection carrier");
    assert!(matches!(carrier.geometry, CurveGeometry::Nurbs(_)));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_emits_ext11_deltas_intersection_chart() {
    let stream = ext11_charted_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let curve_id = &result.ir.model.procedural_curves[0].curve;
    let curve = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| &curve.id == curve_id)
        .expect("intersection cache");
    let CurveGeometry::Nurbs(nurbs) = &curve.geometry else {
        panic!("NURBS chart cache");
    };
    assert_eq!(nurbs.control_points[1].x, 10.0);
    assert_eq!(nurbs.knots, vec![2.0, 2.0, 5.0, 5.0]);
}

#[test]
fn decode_assigns_ext11_uv_lanes_by_unique_surface_evaluation() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    let [Some(PcurveGeometry::Nurbs {
        control_points: first,
        ..
    }), Some(PcurveGeometry::Nurbs {
        control_points: second,
        ..
    })] = context.sides.clone().map(|side| side.pcurve)
    else {
        panic!("two ext11 pcurves");
    };
    assert_eq!(first, [Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)]);
    assert_eq!(second, [Point2::new(0.0, 0.0), Point2::new(0.0, 10.0)]);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn ext11_uv_assignment_eliminates_the_complementary_support_lane() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let mut result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let surfaces = [
        result.ir.model.surfaces[0].id.clone(),
        result.ir.model.surfaces[1].id.clone(),
    ];
    result.ir.model.surfaces[1].geometry = SurfaceGeometry::Unknown { record: None };
    let lanes = [
        Some(vec![[0.0, 0.0], [0.01, 0.0]]),
        Some(vec![[0.0, 0.0], [0.0, 0.01]]),
    ];

    let assigned = crate::decode::assign_ext11_support_uv_to_surfaces(
        &result.ir,
        [&surfaces[0], &surfaces[1]],
        &[
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
        ],
        0.01,
        &lanes,
    )
    .unwrap();

    assert_eq!(assigned, [lanes[0].clone(), None]);
}

#[test]
fn topology_selects_one_candidate_at_an_ambiguous_record_offset() {
    let mut stream = vec![0; 40];
    stream[..7].copy_from_slice(&[0, 12, 0xff, 0xfe, 0x00, 0x02, 0x01]);
    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(graph.of_kind(12).count(), 1);
    assert_eq!(graph.at_pos(0).map(|node| node.xmt), Some(65_536));
}

#[test]
fn trimmed_curves_reject_nonfinite_endpoint_witnesses() {
    let mut stream = trimmed_topology_partition_stream();
    let trim = stream
        .windows(4)
        .position(|window| window == [0, 133, 0, 12])
        .expect("trimmed curve");
    put_f64(&mut stream, trim + 21, f64::NAN);
    assert!(crate::topology::trimmed_curves(&stream).is_empty());

    put_f64(&mut stream, trim + 21, f64::MAX);
    assert!(crate::topology::trimmed_curves(&stream).is_empty());
}

#[test]
fn nurbs_carriers_reject_nonfinite_millimeter_control_points() {
    let mut surface = bspline_partition_stream();
    let payload = surface
        .windows(4)
        .position(|window| window == [0, 125, 0, 21])
        .expect("surface payload");
    put_f64(&mut surface, payload + 97, f64::MAX);
    assert!(crate::nurbs::surfaces(&surface).is_empty());

    let mut curve = bspline_partition_stream();
    let payload = curve
        .windows(4)
        .position(|window| window == [0, 135, 0, 41])
        .expect("curve payload");
    put_f64(&mut curve, payload + 15, f64::MAX);
    assert!(crate::nurbs::curves(&curve).is_empty());

    let descriptor = curve
        .windows(4)
        .position(|window| window == [0, 136, 0, 40])
        .expect("curve descriptor");
    put_ref(&mut curve, descriptor + 10, 2);
    put_f64(&mut curve, payload + 15, f64::MAX);
    put_f64(&mut curve, payload + 31, f64::MIN_POSITIVE);
    assert!(crate::nurbs::pcurves(&curve).is_empty());
}

#[test]
fn nurbs_carriers_reject_invalid_basis_cardinality() {
    let mut surface = bspline_partition_stream();
    let descriptor = surface
        .windows(4)
        .position(|window| window == [0, 126, 0, 20])
        .expect("surface descriptor");
    put_ref(&mut surface, descriptor + 6, 2);
    assert!(crate::nurbs::surfaces(&surface).is_empty());

    let mut curve = bspline_partition_stream();
    let descriptor = curve
        .windows(4)
        .position(|window| window == [0, 136, 0, 40])
        .expect("curve descriptor");
    put_ref(&mut curve, descriptor + 4, 2);
    assert!(crate::nurbs::curves(&curve).is_empty());

    put_ref(&mut curve, descriptor + 10, 2);
    assert!(crate::nurbs::pcurves(&curve).is_empty());

    let mut short_knots = bspline_partition_stream();
    let multiplicities = short_knots
        .windows(12)
        .position(|record| record[..2] == [0, 127] && record[6..8] == 42u16.to_be_bytes())
        .expect("curve multiplicities");
    put_ref(&mut short_knots, multiplicities + 10, 1);
    assert!(crate::nurbs::curves(&short_knots).is_empty());
}

#[test]
fn nurbs_carriers_reject_duplicate_support_identities() {
    fn duplicate_record(stream: &mut Vec<u8>, tag: u8, xmt_offset: usize, xmt: u16, len: usize) {
        let start = stream
            .windows(len)
            .position(|record| {
                record[..2] == [0, tag] && record[xmt_offset..xmt_offset + 2] == xmt.to_be_bytes()
            })
            .expect("support record");
        let duplicate = stream[start..start + len].to_vec();
        stream.extend(duplicate);
    }

    for (tag, xmt_offset, xmt, len) in [
        (126, 2, 20, 48),
        (125, 2, 21, 193),
        (127, 6, 30, 12),
        (128, 6, 32, 24),
    ] {
        let mut stream = bspline_partition_stream();
        duplicate_record(&mut stream, tag, xmt_offset, xmt, len);
        assert!(
            crate::nurbs::surfaces(&stream).is_empty(),
            "duplicate type {tag}"
        );
    }

    for (tag, xmt_offset, xmt, len) in [
        (136, 2, 40, 27),
        (135, 2, 41, 63),
        (127, 6, 42, 12),
        (128, 6, 43, 24),
    ] {
        let mut stream = bspline_partition_stream();
        duplicate_record(&mut stream, tag, xmt_offset, xmt, len);
        assert!(
            crate::nurbs::curves(&stream).is_empty(),
            "duplicate type {tag}"
        );
    }
}

#[test]
fn nurbs_decodes_descriptors_at_the_stream_boundary() {
    fn move_record_to_end(stream: &mut Vec<u8>, tag: u8, xmt: u16, len: usize) {
        let start = stream
            .windows(len)
            .position(|record| record[..2] == [0, tag] && record[2..4] == xmt.to_be_bytes())
            .expect("descriptor record");
        let record = stream.drain(start..start + len).collect::<Vec<_>>();
        stream.extend(record);
    }

    let mut surface = bspline_partition_stream();
    move_record_to_end(&mut surface, 126, 20, 48);
    assert_eq!(crate::nurbs::surfaces(&surface).len(), 1);

    let mut curve = bspline_partition_stream();
    move_record_to_end(&mut curve, 136, 40, 27);
    assert_eq!(crate::nurbs::curves(&curve).len(), 1);
}

#[test]
fn intersection_chart_rejects_nonfinite_millimeter_tolerance() {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let chart = stream
        .windows(2)
        .position(|window| window == [0, 40])
        .expect("chart record");
    put_f64(&mut stream, chart + 28, f64::MAX);
    assert!(crate::intersection::curves(&stream).is_empty());
}

#[test]
fn decode_replaces_ambiguous_ext11_uv_lanes_from_analytic_supports() {
    let stream = two_support_ext11_charted_intersection_curve_stream(true);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    assert!(context.sides.iter().all(|side| side.pcurve.is_some()));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_completes_one_non_sentinel_ext11_uv_lane_analytically() {
    let stream = partial_ext11_charted_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    assert!(context.sides[0].pcurve.is_some());
    assert!(context.sides[1].pcurve.is_some());
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn completed_intersection_support_lane_attaches_after_topology_emission() {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let edge = cadmpeg_ir::ids::EdgeId("synthetic:cube:edge#0".into());
    let target = ir
        .model
        .coedges
        .iter_mut()
        .find(|coedge| coedge.edge == edge && coedge.id.0.contains("bottom"))
        .expect("bottom coedge");
    target.id = cadmpeg_ir::ids::CoedgeId("nx:s0:fin#42".into());
    target.pcurves.clear();
    let owner_loop = target.owner_loop.clone();
    let surface = ir
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id == owner_loop)
        .and_then(|loop_| {
            ir.model
                .faces
                .iter()
                .find(|face| face.id == loop_.face)
                .map(|face| face.surface.clone())
        })
        .expect("bottom support");
    let curve = ir
        .model
        .edges
        .iter()
        .find(|candidate| candidate.id == edge)
        .and_then(|edge| edge.curve.clone())
        .expect("edge curve");
    ir.model
        .procedural_curves
        .push(cadmpeg_ir::geometry::ProceduralCurve {
            id: cadmpeg_ir::ids::ProceduralCurveId("nx:test:intersection#0".into()),
            curve,
            definition: ProceduralCurveDefinition::Intersection {
                context: cadmpeg_ir::geometry::IntcurveSupportContext {
                    sides: [
                        cadmpeg_ir::geometry::IntcurveSupportSide {
                            surface: Some(surface),
                            pcurve_parameter_range: None,
                            pcurve: Some(PcurveGeometry::Nurbs {
                                degree: 1,
                                knots: vec![0.0, 0.0, 1.0, 1.0],
                                control_points: vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
                                weights: None,
                                periodic: false,
                            }),
                        },
                        cadmpeg_ir::geometry::IntcurveSupportSide {
                            surface: None,
                            pcurve_parameter_range: None,
                            pcurve: None,
                        },
                    ],
                    parameter_range: [0.0, 1.0],
                    discontinuities: [Vec::new(), Vec::new(), Vec::new()],
                },
                discontinuity_flag: false,
            },
            cache_fit_tolerance: None,
        });
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    let source_stream = annotations.stream("nx:test");

    crate::decode::attach_completed_intersection_pcurves(
        &mut ir,
        &crate::topology::Graph::parse(&[]),
        "nx:s0",
        source_stream,
        &mut annotations,
    );

    let completed = ir
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.0.contains("intersection-pcurve-completed"))
        .expect("validated completed support lane attaches");
    assert!(ir.model.coedges.iter().any(|coedge| coedge
        .pcurves
        .iter()
        .any(|pcurve| pcurve.pcurve == completed.id)));
}

#[test]
fn ext11_uv_completion_runs_after_support_incidence_resolution() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let mut result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let procedural_id = result.ir.model.procedural_curves[0].id.clone();
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &mut result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    for side in &mut context.sides {
        side.pcurve = None;
    }
    let pending = vec![(
        procedural_id,
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
        ],
        vec![0.0, 0.01],
        0.01,
        [
            Some(vec![[0.0, 0.0], [0.01, 0.0]]),
            Some(vec![[0.0, 0.0], [0.0, 0.01]]),
        ],
    )];

    crate::decode::complete_ext11_support_uv(&mut result.ir, &pending);

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    assert!(context.sides.iter().all(|side| side.pcurve.is_some()));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn analytic_uv_completion_fills_missing_intersection_support_lanes() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let mut result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let procedural_id = result.ir.model.procedural_curves[0].id.clone();
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &mut result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    for side in &mut context.sides {
        side.pcurve = None;
    }
    let pending = vec![(
        procedural_id,
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
        ],
        vec![0.0, 0.01],
        0.01,
        [None, None],
    )];

    crate::decode::complete_support_uv(&mut result.ir, &pending);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    assert!(context.sides.iter().all(|side| side.pcurve.is_some()));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn support_uv_completion_closes_blend_spine_dependencies_to_a_fixed_point() {
    use cadmpeg_ir::geometry::{BlendSupport, ProceduralSurface, Surface};
    use cadmpeg_ir::ids::{ProceduralCurveId, ProceduralSurfaceId, SurfaceId};

    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let mut result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let spine_id = result.ir.model.procedural_curves[0].id.clone();
    let spine_curve = result.ir.model.procedural_curves[0].curve.clone();
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    let spine_surfaces = context
        .sides
        .each_ref()
        .map(|side| side.surface.clone().unwrap());
    let radius = 2.0;
    let offset_surfaces = [0usize, 1usize].map(|side| {
        let support = result
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == spine_surfaces[side])
            .unwrap();
        let SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } = support.geometry
        else {
            panic!("plane support");
        };
        let id = SurfaceId(format!("synthetic:offset-support-{side}"));
        result.ir.model.surfaces.push(Surface {
            id: id.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: cadmpeg_ir::math::Point3::new(
                    origin.x + radius * normal.x,
                    origin.y + radius * normal.y,
                    origin.z + radius * normal.z,
                ),
                normal,
                u_axis,
            },
            source_object: None,
        });
        id
    });
    let blend = SurfaceId("synthetic:dependent-blend".into());
    let blend_construction = ProceduralSurfaceId("synthetic:dependent-blend-definition".into());
    result.ir.model.surfaces.push(Surface {
        id: blend.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: blend_construction.clone(),
        },
        source_object: None,
    });
    result.ir.model.procedural_surfaces.push(ProceduralSurface {
        id: blend_construction,
        surface: blend.clone(),
        definition: ProceduralSurfaceDefinition::Blend {
            supports: offset_surfaces.map(|surface| {
                Some(BlendSupport {
                    surface,
                    reversed: false,
                })
            }),
            spine: Some(spine_curve.clone()),
            radius: BlendRadiusLaw::Constant {
                signed_radius: radius,
            },
            cross_section: BlendCrossSection::Circular,
            native: None,
        },
        cache_fit_tolerance: None,
        record_bounds: None,
    });
    let parameters = vec![0.0, 0.01];
    let spine_carrier = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == spine_curve)
        .expect("blend spine carrier");
    assert!(
        cadmpeg_ir::eval::curve_point(&spine_carrier.geometry, 0.0).is_some(),
        "spine carrier: {:?}",
        spine_carrier.geometry
    );
    let points = parameters
        .iter()
        .map(|parameter| {
            crate::decode::blend_surface_point(&result.ir, &blend, *parameter, 0.5).unwrap()
        })
        .collect::<Vec<_>>();

    let dependent_id = ProceduralCurveId("synthetic:dependent-intersection".into());
    let mut dependent = result.ir.model.procedural_curves[0].clone();
    dependent.id = dependent_id.clone();
    let ProceduralCurveDefinition::Intersection { context, .. } = &mut dependent.definition else {
        unreachable!()
    };
    context.sides[0].surface = Some(blend);
    context.sides[0].pcurve = None;
    context.sides[1].surface = None;
    context.sides[1].pcurve = None;
    result.ir.model.procedural_curves.insert(0, dependent);
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &mut result.ir.model.procedural_curves[1].definition
    else {
        unreachable!()
    };
    for side in &mut context.sides {
        side.pcurve = None;
    }
    let pending = vec![
        (dependent_id, points, parameters.clone(), 0.01, [None, None]),
        (
            spine_id,
            vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
            ],
            parameters,
            0.01,
            [None, None],
        ),
    ];

    crate::decode::complete_support_uv(&mut result.ir, &pending);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    assert!(context.sides[0].pcurve.is_some());
}

#[test]
fn analytic_uv_completion_replaces_a_sentinel_contaminated_support_lane() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let mut result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let procedural_id = result.ir.model.procedural_curves[0].id.clone();
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &mut result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    let Some(PcurveGeometry::Nurbs { control_points, .. }) = context.sides[0].pcurve.as_mut()
    else {
        panic!("NURBS support lane");
    };
    control_points[1] = Point2::new(
        crate::decode::MISSING_TOLERANCE,
        crate::decode::MISSING_TOLERANCE,
    );
    let pending = vec![(
        procedural_id,
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
        ],
        vec![0.0, 0.01],
        0.01,
        [None, None],
    )];

    crate::decode::complete_support_uv(&mut result.ir, &pending);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    let Some(PcurveGeometry::Nurbs { control_points, .. }) = context.sides[0].pcurve.as_ref()
    else {
        panic!("NURBS support lane");
    };
    assert!(control_points.iter().all(|point| {
        point.u.to_bits() != crate::decode::MISSING_TOLERANCE.to_bits()
            && point.v.to_bits() != crate::decode::MISSING_TOLERANCE.to_bits()
    }));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn equivalent_offset_supports_share_a_complete_parameter_lane() {
    use cadmpeg_ir::geometry::{ProceduralCurve, ProceduralSurface, Surface};
    use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let supports = [SurfaceId("support-a".into()), SurfaceId("support-b".into())];
    for support in &supports {
        ir.model.surfaces.push(Surface {
            id: support.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        });
    }
    let offsets = [SurfaceId("offset-a".into()), SurfaceId("offset-b".into())];
    for (ordinal, (surface, support)) in offsets.iter().zip(&supports).enumerate() {
        let construction = ProceduralSurfaceId(format!("offset-construction-{ordinal}"));
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: construction.clone(),
            },
            source_object: None,
        });
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: construction,
            surface: surface.clone(),
            definition: ProceduralSurfaceDefinition::Offset {
                support: support.clone(),
                distance: 30.0,
                u_sense: Some(0),
                v_sense: Some(0),
                extension_flags: Vec::new(),
                revision_form: None,
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        });
    }
    ir.model.procedural_curves.push(ProceduralCurve {
        id: ProceduralCurveId("intersection".into()),
        curve: CurveId("curve".into()),
        definition: ProceduralCurveDefinition::Intersection {
            context: cadmpeg_ir::geometry::IntcurveSupportContext {
                sides: [
                    cadmpeg_ir::geometry::IntcurveSupportSide {
                        surface: Some(offsets[0].clone()),
                        pcurve_parameter_range: None,
                        pcurve: None,
                    },
                    cadmpeg_ir::geometry::IntcurveSupportSide {
                        surface: Some(offsets[1].clone()),
                        pcurve_parameter_range: None,
                        pcurve: Some(PcurveGeometry::Line {
                            origin: Point2::new(1.0, 2.0),
                            direction: Point2::new(3.0, 4.0),
                        }),
                    },
                ],
                parameter_range: [0.0, 1.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: None,
    });

    assert!(crate::decode::parameterization_equivalent_surfaces(
        &ir,
        &offsets[0],
        &offsets[1]
    ));
    crate::decode::complete_parameterization_equivalent_support_uv(&mut ir);
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves[0].definition
    else {
        panic!("intersection");
    };
    assert_eq!(context.sides[0].pcurve, context.sides[1].pcurve);

    let ProceduralSurfaceDefinition::Offset { distance, .. } =
        &mut ir.model.procedural_surfaces[1].definition
    else {
        unreachable!()
    };
    *distance = 31.0;
    assert!(!crate::decode::parameterization_equivalent_surfaces(
        &ir,
        &offsets[0],
        &offsets[1]
    ));
}

#[test]
fn nurbs_parameter_solver_inverts_a_rational_surface_point() {
    let surface = cadmpeg_ir::geometry::NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 2,
        v_count: 2,
        control_points: vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 10.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 10.0, 0.0),
        ],
        weights: Some(vec![1.0, 2.0, 3.0, 4.0]),
        u_periodic: false,
        v_periodic: false,
    };
    let expected = Point2::new(0.37, 0.61);
    let point = cadmpeg_ir::eval::nurbs_surface_point(&surface, expected.u, expected.v).unwrap();

    let actual = crate::decode::nurbs_parameters(&surface, point, None).unwrap();

    assert!((actual.u - expected.u).abs() < 1.0e-10);
    assert!((actual.v - expected.v).abs() < 1.0e-10);

    let after_invalid_seed =
        crate::decode::nurbs_parameters(&surface, point, Some(Point2::new(f64::NAN, 0.5))).unwrap();
    assert!((after_invalid_seed.u - expected.u).abs() < 1.0e-10);
    assert!((after_invalid_seed.v - expected.v).abs() < 1.0e-10);
}

#[test]
fn surface_intersection_continuation_corrects_a_chart_selected_branch() {
    use cadmpeg_ir::geometry::Surface;
    use cadmpeg_ir::ids::SurfaceId;
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let first = SurfaceId("synthetic:first-intersection-plane".into());
    let second = SurfaceId("synthetic:second-intersection-plane".into());
    ir.model.surfaces.extend([
        Surface {
            id: first.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(1.0, 0.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
        Surface {
            id: second.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
    ]);
    let chart = vec![
        Point3::new(1.0e-4, -2.0e-4, 0.0),
        Point3::new(-1.0e-4, 2.0e-4, 2.0),
        Point3::new(2.0e-4, 1.0e-4, 5.0),
    ];
    let lanes = crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&first, &second],
        &chart,
        1.0e-3,
    )
    .unwrap();
    assert_eq!(lanes[0].len(), chart.len());
    for (ordinal, expected_z) in [0.0, 2.0, 5.0].into_iter().enumerate() {
        let first_point = cadmpeg_ir::eval::model_surface_point_by_id(
            &ir,
            &first,
            lanes[0][ordinal].u,
            lanes[0][ordinal].v,
        )
        .unwrap();
        let second_point = cadmpeg_ir::eval::model_surface_point_by_id(
            &ir,
            &second,
            lanes[1][ordinal].u,
            lanes[1][ordinal].v,
        )
        .unwrap();
        assert!((first_point.x - second_point.x).abs() < 1.0e-10);
        assert!((first_point.y - second_point.y).abs() < 1.0e-10);
        assert!((first_point.z - second_point.z).abs() < 1.0e-10);
        assert!((first_point.z - expected_z).abs() < 1.0e-10);
    }

    let off_branch = [chart[0], Point3::new(1.0, 1.0, 2.0)];
    assert!(crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&first, &second],
        &off_branch,
        1.0e-3,
    )
    .is_none());
    assert!(crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&first, &first],
        &chart,
        1.0e-3,
    )
    .is_none());

    let cylinder = SurfaceId("synthetic:intersection-cylinder".into());
    let section_plane = SurfaceId("synthetic:intersection-section-plane".into());
    ir.model.surfaces.extend([
        Surface {
            id: cylinder.clone(),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 2.0,
            },
            source_object: None,
        },
        Surface {
            id: section_plane.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
    ]);
    let circular_chart =
        [0.0_f64, 0.3, 0.8].map(|angle| Point3::new(2.0 * angle.cos(), 2.0 * angle.sin(), 1.0e-5));
    let circular_lanes = crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&cylinder, &section_plane],
        &circular_chart,
        1.0e-3,
    )
    .unwrap();
    for (cylinder_uv, plane_uv) in circular_lanes[0].iter().zip(&circular_lanes[1]) {
        let cylinder_point = cadmpeg_ir::eval::model_surface_point_by_id(
            &ir,
            &cylinder,
            cylinder_uv.u,
            cylinder_uv.v,
        )
        .unwrap();
        let plane_point = cadmpeg_ir::eval::model_surface_point_by_id(
            &ir,
            &section_plane,
            plane_uv.u,
            plane_uv.v,
        )
        .unwrap();
        assert!((cylinder_point.x - plane_point.x).abs() < 1.0e-8);
        assert!((cylinder_point.y - plane_point.y).abs() < 1.0e-8);
        assert!((cylinder_point.z - plane_point.z).abs() < 1.0e-8);
    }

    let tangent_cylinder = SurfaceId("synthetic:tangent-cylinder".into());
    let tangent_plane = SurfaceId("synthetic:tangent-plane".into());
    ir.model.surfaces.extend([
        Surface {
            id: tangent_cylinder.clone(),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(0.0, 0.0, 1.0),
                axis: Vector3::new(0.0, 1.0, 0.0),
                ref_direction: Vector3::new(0.0, 0.0, -1.0),
                radius: 1.0,
            },
            source_object: None,
        },
        Surface {
            id: tangent_plane.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
    ]);
    let tangent_chart = [0.0, 1.0, 3.0, 6.0].map(|y| Point3::new(0.0, y, 0.0));
    let tangent_lanes = crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&tangent_cylinder, &tangent_plane],
        &tangent_chart,
        1.0e-8,
    )
    .unwrap();
    for (ordinal, y) in [0.0, 1.0, 3.0, 6.0].into_iter().enumerate() {
        assert!((tangent_lanes[0][ordinal].v - y).abs() < 1.0e-10);
        assert!((tangent_lanes[1][ordinal].v - y).abs() < 1.0e-10);
    }

    let seam_chart = [3.0_f64, 3.1, 3.2, 3.3]
        .map(|angle| Point3::new(2.0 * angle.cos(), 2.0 * angle.sin(), 1.0e-5));
    let seam_lanes = crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&cylinder, &section_plane],
        &seam_chart,
        1.0e-3,
    )
    .unwrap();
    assert!(seam_lanes[0].windows(2).all(|pair| pair[0].u < pair[1].u));
    assert!(seam_lanes[0].last().unwrap().u > std::f64::consts::PI);

    let periodic_nurbs = SurfaceId("synthetic:periodic-nurbs-prism".into());
    let nurbs_section = SurfaceId("synthetic:periodic-nurbs-section".into());
    let periodic_geometry = cadmpeg_ir::geometry::NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 5,
        v_count: 2,
        control_points: [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0), (1.0, 0.0)]
            .into_iter()
            .flat_map(|(x, y)| [Point3::new(x, y, 0.0), Point3::new(x, y, 1.0)])
            .collect(),
        weights: None,
        u_periodic: true,
        v_periodic: false,
    };
    ir.model.surfaces.extend([
        Surface {
            id: periodic_nurbs.clone(),
            geometry: SurfaceGeometry::Nurbs(periodic_geometry.clone()),
            source_object: None,
        },
        Surface {
            id: nurbs_section.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.5),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
    ]);
    let nurbs_chart = [3.8, 3.9, 4.1, 4.2]
        .map(|u| cadmpeg_ir::eval::nurbs_surface_point(&periodic_geometry, u, 0.5).unwrap());
    let nurbs_lanes = crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&periodic_nurbs, &nurbs_section],
        &nurbs_chart,
        1.0e-8,
    )
    .unwrap();
    assert!(nurbs_lanes[0].windows(2).all(|pair| pair[0].u < pair[1].u));
    assert!(nurbs_lanes[0].last().unwrap().u > 4.0);
}

#[test]
fn periodic_surface_lookup_rejects_a_cyclic_offset_graph() {
    use cadmpeg_ir::geometry::{ProceduralSurface, Surface};
    use cadmpeg_ir::ids::{ProceduralSurfaceId, SurfaceId};

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let surfaces = [SurfaceId("cycle-a".into()), SurfaceId("cycle-b".into())];
    let constructions = [
        ProceduralSurfaceId("cycle-construction-a".into()),
        ProceduralSurfaceId("cycle-construction-b".into()),
    ];
    for side in 0..2 {
        ir.model.surfaces.push(Surface {
            id: surfaces[side].clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: constructions[side].clone(),
            },
            source_object: None,
        });
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: constructions[side].clone(),
            surface: surfaces[side].clone(),
            definition: ProceduralSurfaceDefinition::Offset {
                support: surfaces[1 - side].clone(),
                distance: 1.0,
                u_sense: Some(0),
                v_sense: Some(0),
                extension_flags: Vec::new(),
                revision_form: None,
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        });
    }

    assert_eq!(
        crate::decode::surface_parameter_periods(&ir, &surfaces[0]),
        [None, None]
    );
}

#[test]
fn nurbs_parameter_solver_rejects_a_remote_local_minimum_seed() {
    let mut control_points = Vec::new();
    for (x, z) in [
        (-10.0, 0.0),
        (0.0, 0.0),
        (10.0, 2.0),
        (0.0, 4.0),
        (-10.0, 4.0),
    ] {
        control_points.extend([
            cadmpeg_ir::math::Point3::new(x, 0.0, z),
            cadmpeg_ir::math::Point3::new(x, 10.0, z),
        ]);
    }
    let surface = cadmpeg_ir::geometry::NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 5,
        v_count: 2,
        control_points,
        weights: None,
        u_periodic: false,
        v_periodic: false,
    };
    let expected = Point2::new(0.125, 0.3);
    let point = cadmpeg_ir::eval::nurbs_surface_point(&surface, expected.u, expected.v).unwrap();

    let actual =
        crate::decode::nurbs_parameters(&surface, point, Some(Point2::new(0.875, 0.3))).unwrap();

    assert!((actual.u - expected.u).abs() < 1.0e-10);
    assert!((actual.v - expected.v).abs() < 1.0e-10);
}

#[test]
fn nurbs_curve_closest_parameter_does_not_trust_a_remote_seed() {
    use cadmpeg_ir::geometry::{Curve, NurbsCurve};
    use cadmpeg_ir::ids::CurveId;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let curve = CurveId("synthetic:piecewise-spine".into());
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Nurbs(NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 0.5, 1.0, 1.0],
            control_points: vec![
                cadmpeg_ir::math::Point3::new(-10.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(10.0, 10.0, 0.0),
            ],
            weights: None,
            periodic: false,
        }),
        source_object: None,
    });

    let actual = crate::decode::closest_spine_parameter(
        &ir,
        &curve,
        cadmpeg_ir::math::Point3::new(-5.0, 2.0, 0.0),
        Some(0.9),
    )
    .unwrap();

    assert!((actual - 0.25).abs() < 1.0e-10);
}

#[test]
fn spine_contact_pcurve_inverts_linear_and_rational_support_parameters() {
    let pcurve = PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![2.0, 2.0, 5.0, 9.0, 9.0],
        control_points: vec![
            Point2::new(-1.0, 3.0),
            Point2::new(2.0, 6.0),
            Point2::new(6.0, 4.0),
        ],
        weights: None,
        periodic: false,
    };

    let first = crate::decode::closest_pcurve_parameter(&pcurve, Point2::new(0.5, 4.5)).unwrap();
    let second = crate::decode::closest_pcurve_parameter(&pcurve, Point2::new(5.0, 4.5)).unwrap();

    assert!((first - 3.5).abs() < 1.0e-12);
    assert!((second - 8.0).abs() < 1.0e-12);

    let rational = PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        weights: Some(vec![1.0, 2.0]),
        periodic: false,
    };
    let rational_parameter =
        crate::decode::closest_pcurve_parameter(&rational, Point2::new(0.5, 0.0)).unwrap();
    assert!((rational_parameter - 1.0 / 3.0).abs() < 1.0e-10);

    let quadratic = PcurveGeometry::Nurbs {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        control_points: vec![
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 1.0),
            Point2::new(2.0, 0.0),
        ],
        weights: None,
        periodic: false,
    };
    let quadratic_parameter =
        crate::decode::closest_pcurve_parameter(&quadratic, Point2::new(1.0, 0.5)).unwrap();
    assert!((quadratic_parameter - 0.5).abs() < 1.0e-10);
}

#[test]
fn blend_contact_offset_requires_the_radius_magnitude() {
    assert!(crate::decode::blend_contact_offset_matches(2.0, 5.0, 3.0));
    assert!(crate::decode::blend_contact_offset_matches(2.0, -1.0, 3.0));
    assert!(crate::decode::blend_contact_offset_matches(
        2.0,
        f64::from_bits(5.0f64.to_bits() + 1),
        3.0,
    ));
    assert!(!crate::decode::blend_contact_offset_matches(
        2.0, 5.001, 3.0
    ));
}

#[test]
fn blend_contact_matches_separate_analytic_offset_carriers() {
    use cadmpeg_ir::geometry::Surface;
    use cadmpeg_ir::ids::SurfaceId;
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let support = SurfaceId("synthetic:support-cylinder".into());
    let offset = SurfaceId("synthetic:offset-cylinder".into());
    let cylinder = |id, radius| Surface {
        id,
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(-46.75, 0.0, -112.06),
            axis: Vector3::new(1.0, 0.0, 0.0),
            ref_direction: Vector3::new(0.0, 0.0, -1.0),
            radius,
        },
        source_object: None,
    };
    ir.model.surfaces.extend([
        cylinder(support.clone(), 294.0),
        cylinder(offset.clone(), 299.0),
    ]);

    assert_eq!(
        crate::decode::constant_surface_offset_between(&ir, &support, &offset, 0),
        Some(5.0)
    );
    let SurfaceGeometry::Cylinder { origin, .. } = &mut ir.model.surfaces[1].geometry else {
        unreachable!()
    };
    origin.y = 1.0;
    assert!(crate::decode::constant_surface_offset_between(&ir, &support, &offset, 0).is_none());

    let support_plane = SurfaceId("synthetic:support-plane".into());
    let offset_plane = SurfaceId("synthetic:offset-plane".into());
    let plane = |id, origin| Surface {
        id,
        geometry: SurfaceGeometry::Plane {
            origin,
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    };
    ir.model.surfaces.extend([
        plane(support_plane.clone(), Point3::new(10.0, 20.0, 30.0)),
        plane(offset_plane.clone(), Point3::new(10.0, 20.0, 35.0)),
    ]);
    assert_eq!(
        crate::decode::constant_surface_offset_between(&ir, &support_plane, &offset_plane, 0),
        Some(5.0)
    );
    let SurfaceGeometry::Plane { origin, .. } = &mut ir.model.surfaces[3].geometry else {
        unreachable!()
    };
    origin.x += 1.0;
    assert!(
        crate::decode::constant_surface_offset_between(&ir, &support_plane, &offset_plane, 0)
            .is_none()
    );
}

#[test]
fn blend_contact_matches_concentric_blend_carriers() {
    use cadmpeg_ir::geometry::{BlendSupport, ProceduralSurface, Surface};
    use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let first = SurfaceId("synthetic:first".into());
    let second = SurfaceId("synthetic:second".into());
    let first_offset = SurfaceId("synthetic:first-offset".into());
    let second_offset = SurfaceId("synthetic:second-offset".into());
    let plane = |id, origin, normal, u_axis| Surface {
        id,
        geometry: SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        },
        source_object: None,
    };
    ir.model.surfaces.extend([
        plane(
            first.clone(),
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ),
        plane(
            second.clone(),
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ),
        plane(
            first_offset.clone(),
            Point3::new(3.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ),
        plane(
            second_offset.clone(),
            Point3::new(0.0, 3.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ),
    ]);

    let spine = CurveId("synthetic:shared-spine".into());
    let inner = SurfaceId("synthetic:inner-blend".into());
    let outer = SurfaceId("synthetic:outer-blend".into());
    for (surface, supports, radius) in [
        (inner.clone(), [first, second], 0.7),
        (outer.clone(), [first_offset, second_offset], 3.7),
    ] {
        let construction = ProceduralSurfaceId(format!("{}:construction", surface.0));
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: construction.clone(),
            },
            source_object: None,
        });
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: construction,
            surface,
            definition: ProceduralSurfaceDefinition::Blend {
                supports: supports.map(|surface| {
                    Some(BlendSupport {
                        surface,
                        reversed: false,
                    })
                }),
                spine: Some(spine.clone()),
                radius: BlendRadiusLaw::Constant {
                    signed_radius: radius,
                },
                cross_section: BlendCrossSection::Circular,
                native: None,
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        });
    }

    assert_eq!(
        crate::decode::constant_surface_offset_between(&ir, &inner, &outer, 0),
        Some(3.0)
    );
    let outer_definition = ir
        .model
        .procedural_surfaces
        .iter_mut()
        .find(|candidate| candidate.surface == outer)
        .unwrap();
    let ProceduralSurfaceDefinition::Blend { supports, .. } = &mut outer_definition.definition
    else {
        unreachable!()
    };
    supports[0].as_mut().unwrap().reversed = true;
    assert!(crate::decode::constant_surface_offset_between(&ir, &inner, &outer, 0).is_none());
}

#[test]
fn closest_spine_parameter_inverts_periodic_analytic_curves() {
    use cadmpeg_ir::geometry::Curve;
    use cadmpeg_ir::ids::CurveId;
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let ellipse = CurveId("synthetic:ellipse-spine".into());
    let geometry = CurveGeometry::Ellipse {
        center: Point3::new(2.0, 3.0, 4.0),
        axis: Vector3::new(0.0, 1.0, 0.0),
        major_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 12.0,
        minor_radius: 5.0,
    };
    let parameter = 1.2;
    let mut point = cadmpeg_ir::eval::curve_point(&geometry, parameter).unwrap();
    point.y += 3.0;
    ir.model.curves.push(Curve {
        id: ellipse.clone(),
        geometry,
        source_object: None,
    });

    let first = crate::decode::closest_spine_parameter(&ir, &ellipse, point, None).unwrap();
    let continued = crate::decode::closest_spine_parameter(
        &ir,
        &ellipse,
        point,
        Some(parameter + std::f64::consts::TAU),
    )
    .unwrap();

    assert!((first - parameter).abs() < 1.0e-8, "{first}");
    assert!(
        (continued - parameter - std::f64::consts::TAU).abs() < 1.0e-8,
        "{continued}"
    );
}

#[test]
fn rolling_ball_blend_parameters_invert_the_canal_surface_law() {
    use cadmpeg_ir::geometry::{
        BlendSupport, Curve, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve,
        ProceduralCurveDefinition, ProceduralSurface, Surface,
    };
    use cadmpeg_ir::ids::{
        CurveId, EdgeId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::topology::Edge;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let first = SurfaceId("synthetic:first-plane".into());
    let second = SurfaceId("synthetic:second-plane".into());
    ir.model.surfaces.extend([
        Surface {
            id: first.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(1.0, 0.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
        Surface {
            id: second.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
    ]);
    let first_spine_side = SurfaceId("synthetic:first-spine-side".into());
    let second_spine_side = SurfaceId("synthetic:second-spine-side".into());
    ir.model.surfaces.extend([
        Surface {
            id: first_spine_side.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: cadmpeg_ir::math::Point3::new(2.0, 0.0, 0.0),
                normal: Vector3::new(1.0, 0.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
        Surface {
            id: second_spine_side.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: cadmpeg_ir::math::Point3::new(0.0, 2.0, 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
    ]);
    let spine = CurveId("synthetic:spine".into());
    ir.model.curves.push(Curve {
        id: spine.clone(),
        geometry: CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(2.0, 2.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    });
    let surface = SurfaceId("synthetic:blend".into());
    let construction = ProceduralSurfaceId("synthetic:blend-construction".into());
    ir.model.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: construction.clone(),
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface {
        id: construction,
        surface: surface.clone(),
        definition: ProceduralSurfaceDefinition::Blend {
            supports: [
                Some(BlendSupport {
                    surface: first.clone(),
                    reversed: false,
                }),
                Some(BlendSupport {
                    surface: second.clone(),
                    reversed: false,
                }),
            ],
            spine: Some(spine.clone()),
            radius: BlendRadiusLaw::Constant { signed_radius: 2.0 },
            cross_section: BlendCrossSection::Circular,
            native: None,
        },
        cache_fit_tolerance: None,
        record_bounds: None,
    });
    let expected = Point2::new(8.0, 0.35);
    let point = crate::decode::blend_surface_point(&ir, &surface, expected.u, expected.v).unwrap();

    assert_eq!(
        crate::decode::blend_spine_cache_fit_tolerance(&ir, &surface, 0.25),
        0.25
    );
    ir.model.procedural_curves.push(ProceduralCurve {
        id: ProceduralCurveId("synthetic:spine-construction".into()),
        curve: spine.clone(),
        definition: ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext {
                sides: [
                    IntcurveSupportSide {
                        surface: Some(first_spine_side),
                        pcurve_parameter_range: None,
                        pcurve: Some(PcurveGeometry::Line {
                            origin: Point2::new(0.0, -2.0),
                            direction: Point2::new(1.0, 0.0),
                        }),
                    },
                    IntcurveSupportSide {
                        surface: Some(second_spine_side),
                        pcurve_parameter_range: None,
                        pcurve: Some(PcurveGeometry::Line {
                            origin: Point2::new(0.0, 2.0),
                            direction: Point2::new(1.0, 0.0),
                        }),
                    },
                ],
                parameter_range: [0.0, 10.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: Some(0.75),
    });
    assert_eq!(
        crate::decode::blend_spine_cache_fit_tolerance(&ir, &surface, 0.25),
        1.0
    );

    let actual = crate::decode::blend_surface_parameters(&ir, &surface, point, None).unwrap();

    assert!((actual.u - expected.u).abs() < 1.0e-8);
    assert!((actual.v - expected.v).abs() < 1.0e-8);
    let continued = crate::decode::blend_surface_parameters_for_fit(
        &ir,
        &surface,
        point,
        Some(Point2::new(expected.u + 0.1, expected.v - 0.05)),
        1.0e-8,
    )
    .unwrap();
    assert!((continued.u - expected.u).abs() < 1.0e-8);
    assert!((continued.v - expected.v).abs() < 1.0e-8);

    let boundary_curve = CurveId("synthetic:blend-boundary-curve".into());
    ir.model.procedural_curves.push(ProceduralCurve {
        id: ProceduralCurveId("synthetic:blend-boundary".into()),
        curve: boundary_curve.clone(),
        definition: ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext {
                sides: [
                    IntcurveSupportSide {
                        surface: Some(first.clone()),
                        pcurve_parameter_range: None,
                        pcurve: Some(PcurveGeometry::Line {
                            origin: Point2::new(0.0, -2.0),
                            direction: Point2::new(1.0, 0.0),
                        }),
                    },
                    IntcurveSupportSide {
                        surface: Some(surface.clone()),
                        pcurve_parameter_range: None,
                        pcurve: None,
                    },
                ],
                parameter_range: [0.0, 1.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: None,
    });
    ir.model.edges.push(Edge {
        id: EdgeId("synthetic:blend-boundary-edge".into()),
        curve: Some(boundary_curve),
        start: VertexId("synthetic:blend-boundary-start".into()),
        end: VertexId("synthetic:blend-boundary-end".into()),
        param_range: Some([0.0, 1.0]),
        tolerance: Some(1.0e-8),
    });
    crate::decode::complete_intersection_pcurves_from_opposite_charts(&mut ir);
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves.last().unwrap().definition
    else {
        unreachable!()
    };
    let PcurveGeometry::Nurbs { control_points, .. } = context.sides[1].pcurve.as_ref().unwrap()
    else {
        unreachable!()
    };
    assert_eq!(control_points.first(), Some(&Point2::new(0.0, 0.0)));
    assert_eq!(control_points.last(), Some(&Point2::new(1.0, 0.0)));
    assert_eq!(
        crate::decode::blend_boundary_parameter_from_support_spine(
            &ir,
            &surface,
            &first,
            cadmpeg_ir::math::Point3::new(0.0, 2.0, 0.0),
            None,
            1.0e-8,
        ),
        Some(Point2::new(0.0, 0.0))
    );
    ir.model
        .procedural_curves
        .iter_mut()
        .find(|procedural| procedural.curve == spine)
        .unwrap()
        .definition = ProceduralCurveDefinition::Unknown {
        native_kind: None,
        record: None,
    };
    assert_eq!(
        crate::decode::blend_boundary_parameter_from_support_spine(
            &ir,
            &surface,
            &first,
            cadmpeg_ir::math::Point3::new(0.0, 2.0, 0.0),
            None,
            1.0e-8,
        ),
        Some(Point2::new(0.0, 0.0))
    );

    ir.model
        .curves
        .iter_mut()
        .find(|curve| curve.id == spine)
        .unwrap()
        .geometry = CurveGeometry::Nurbs(cadmpeg_ir::geometry::NurbsCurve {
        degree: 1,
        knots: vec![0.0, 0.0, 10.0, 10.0],
        control_points: vec![
            cadmpeg_ir::math::Point3::new(2.0, 2.0, 0.0),
            cadmpeg_ir::math::Point3::new(2.0, 2.0, 10.0),
        ],
        weights: None,
        periodic: false,
    });
    let coarse = crate::decode::coarse_blend_surface_parameters(&ir, &surface, point, 0).unwrap();
    let coarse_point =
        crate::decode::blend_surface_point(&ir, &surface, coarse.u, coarse.v).unwrap();
    assert!(
        ((coarse_point.x - point.x).powi(2)
            + (coarse_point.y - point.y).powi(2)
            + (coarse_point.z - point.z).powi(2))
        .sqrt()
            < 1.0
    );

    let refined = crate::decode::refine_blend_surface_parameters(
        &ir,
        &surface,
        point,
        Point2::new(expected.u + 0.5, expected.v + 0.1),
        0,
    )
    .unwrap();
    let refined_point =
        crate::decode::blend_surface_point(&ir, &surface, refined.u, refined.v).unwrap();
    let refined_error = ((refined_point.x - point.x).powi(2)
        + (refined_point.y - point.y).powi(2)
        + (refined_point.z - point.z).powi(2))
    .sqrt();
    assert!(refined_error < 1.0e-9);

    let third = SurfaceId("synthetic:third-plane".into());
    ir.model.surfaces.push(Surface {
        id: third.clone(),
        geometry: SurfaceGeometry::Plane {
            origin: cadmpeg_ir::math::Point3::new(0.0, 8.0, 0.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
            u_axis: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    });
    let outer_spine = CurveId("synthetic:outer-spine".into());
    ir.model.curves.push(Curve {
        id: outer_spine.clone(),
        geometry: CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(4.0, 6.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    });
    let outer = SurfaceId("synthetic:outer-blend".into());
    let outer_construction = ProceduralSurfaceId("synthetic:outer-blend-construction".into());
    ir.model.surfaces.push(Surface {
        id: outer.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: outer_construction.clone(),
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface {
        id: outer_construction,
        surface: outer.clone(),
        definition: ProceduralSurfaceDefinition::Blend {
            supports: [
                Some(BlendSupport {
                    surface,
                    reversed: false,
                }),
                Some(BlendSupport {
                    surface: third,
                    reversed: false,
                }),
            ],
            spine: Some(outer_spine),
            radius: BlendRadiusLaw::Constant { signed_radius: 1.5 },
            cross_section: BlendCrossSection::Circular,
            native: None,
        },
        cache_fit_tolerance: None,
        record_bounds: None,
    });
    let expected = Point2::new(4.0, 0.2);
    let point = crate::decode::blend_surface_point(&ir, &outer, expected.u, expected.v).unwrap();
    let actual = crate::decode::blend_surface_parameters(&ir, &outer, point, None).unwrap();
    assert!((actual.u - expected.u).abs() < 1.0e-8);
    assert!((actual.v - expected.v).abs() < 1.0e-8);

    let outer_definition = ir
        .model
        .procedural_surfaces
        .iter_mut()
        .find(|candidate| candidate.surface == outer)
        .unwrap();
    let ProceduralSurfaceDefinition::Blend { supports, .. } = &mut outer_definition.definition
    else {
        panic!("blend definition");
    };
    supports[0].as_mut().unwrap().surface = outer.clone();
    assert!(crate::decode::blend_surface_point(&ir, &outer, expected.u, expected.v).is_none());
}

#[test]
fn decode_emits_both_intersection_support_pcurves() {
    let stream = two_support_charted_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    assert!(context.sides[0].surface.is_some());
    assert!(context.sides[0].pcurve.is_some());
    assert!(context.sides[1].surface.is_some());
    assert!(context.sides[1].pcurve.is_some());
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_emits_inline_descriptor_intersection_witnesses() {
    let stream = inline_descriptor_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(matches!(
        result.ir.model.procedural_curves[0].definition,
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { .. }
    ));
    assert!(matches!(
        result
            .ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == result.ir.model.procedural_curves[0].curve)
            .expect("intersection curve")
            .geometry,
        CurveGeometry::Nurbs(_)
    ));
}

#[test]
fn decode_emits_topology_when_record_xmt_uses_extended_encoding() {
    let stream = large_xmt_headers(&topology_partition_stream());
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_maps_parasolid_tolerance_sentinel_to_none() {
    let stream = topology_with_missing_tolerances();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.vertices[0].tolerance, None);
    assert_eq!(result.ir.model.edges[0].tolerance, None);
    assert_eq!(result.ir.model.faces[0].tolerance, None);
}

#[test]
fn decode_dual_writes_inline_entity_metadata_to_annotations() {
    let mut cur = Cursor::new(topology_part_prt());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let ir = &result.ir;
    let annotations = &result.source_fidelity.annotations;

    macro_rules! assert_arena_annotations {
        ($arena:expr) => {
            for entity in $arena {
                let provenance = annotations
                    .provenance
                    .get(&entity.id.to_string())
                    .expect("annotation provenance");
                assert!(annotations.streams[provenance.stream as usize].starts_with("nx:"));
                assert!(provenance.tag.is_some());
            }
        };
    }

    assert_arena_annotations!(&ir.model.bodies);
    assert_arena_annotations!(&ir.model.regions);
    assert_arena_annotations!(&ir.model.shells);
    assert_arena_annotations!(&ir.model.faces);
    assert_arena_annotations!(&ir.model.loops);
    assert_arena_annotations!(&ir.model.coedges);
    assert_arena_annotations!(&ir.model.edges);
    assert_arena_annotations!(&ir.model.vertices);
    assert_arena_annotations!(&ir.model.points);
    assert_arena_annotations!(&ir.model.surfaces);
    assert_arena_annotations!(&ir.model.curves);
    let unknowns = ir.native_unknowns("nx").unwrap();
    assert_arena_annotations!(&unknowns);

    let point_note = &annotations.exactness[&ir.model.points[0].id.to_string()];
    assert_eq!(point_note.entity, Exactness::ByteExact);
    assert_eq!(point_note.fields["position"], Exactness::Derived);
    let surface_note = &annotations.exactness[&ir.model.surfaces[0].id.to_string()];
    assert_eq!(surface_note.fields["geometry"], Exactness::Derived);
    let curve_note = &annotations.exactness[&ir.model.curves[0].id.to_string()];
    assert_eq!(curve_note.fields["geometry"], Exactness::Derived);
    for id in [
        ir.model.vertices[0].id.to_string(),
        ir.model.edges[0].id.to_string(),
        ir.model.faces[0].id.to_string(),
    ] {
        assert_eq!(
            annotations.exactness[&id].fields["tolerance"],
            Exactness::Derived
        );
    }
}

#[test]
fn decode_transfers_bspline_surface_and_curve() {
    let stream = bspline_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let surface = result
        .ir
        .model
        .surfaces
        .iter()
        .find_map(|surface| match &surface.geometry {
            SurfaceGeometry::Nurbs(surface) => Some(surface),
            _ => None,
        })
        .expect("B-spline surface");
    assert_eq!(surface.u_knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.control_points.len(), 4);
    assert!((surface.control_points[1].y - 20.0).abs() < 1e-9);
    let curve = result
        .ir
        .model
        .curves
        .iter()
        .find_map(|curve| match &curve.geometry {
            CurveGeometry::Nurbs(curve) => Some(curve),
            _ => None,
        })
        .expect("B-spline curve");
    assert_eq!(curve.knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(curve.control_points.len(), 2);
    assert!((curve.control_points[1].x - 20.0).abs() < 1e-9);
}

#[test]
fn nurbs_decodes_extended_xmt_arrays_payload_and_long_surface_descriptor() {
    let surfaces = crate::nurbs::surfaces(&extended_bspline_surface_stream());
    assert_eq!(surfaces.len(), 1);
    let SurfaceGeometry::Nurbs(surface) = &surfaces[0].geometry else {
        panic!("expected NURBS surface");
    };
    assert_eq!(surface.u_knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.v_knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.control_points.len(), 4);
    assert_eq!(surface.control_points[3].y, 20.0);
}

#[test]
fn nurbs_decodes_escaped_curve_descriptor_and_payload_count() {
    let mut stream = bspline_partition_stream();
    let descriptor = stream
        .windows(4)
        .position(|window| window == [0, 136, 0, 40])
        .expect("curve descriptor");
    stream.insert(descriptor + 2, 0xff);
    let payload = stream
        .windows(4)
        .position(|window| window == [0, 135, 0, 41])
        .expect("curve payload");
    stream.insert(payload + 2, 0xff);
    stream.insert(payload + 10, 0xff);

    let curves = crate::nurbs::curves(&stream);
    assert_eq!(curves.len(), 1);
    let CurveGeometry::Nurbs(curve) = &curves[0].geometry else {
        panic!("expected NURBS curve");
    };
    assert_eq!(curve.control_points.len(), 2);
    assert_eq!(curve.control_points[1].x, 20.0);
}

#[test]
fn decode_replaces_partition_bspline_surface_wrapper_from_deltas() {
    let partition = bspline_surface_replacement_partition_stream();
    let deltas = deltas_bspline_surface_wrapper_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        &surface.geometry,
        SurfaceGeometry::Nurbs(nurbs)
            if nurbs.control_points.iter().any(|point| point.y == 30.0)
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_bspline_curve_wrapper_from_deltas() {
    let partition = bspline_curve_replacement_partition_stream();
    let deltas = deltas_bspline_curve_wrapper_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.curves.iter().any(|curve| matches!(
        &curve.geometry,
        CurveGeometry::Nurbs(nurbs)
            if nurbs.control_points.iter().any(|point| point.y == 10.0)
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_uses_partner_fin_vertex_for_edge_endpoint() {
    let mut cur = Cursor::new(prt_with_partition(
        &partnered_trimmed_topology_partition_stream(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let edge = result.ir.model.edges.first().expect("edge");
    assert_ne!(edge.start, edge.end);
    assert_eq!(edge.param_range, Some([0.25, 0.75]));
    assert_eq!(result.ir.model.coedges.len(), 2);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_resolves_forward_trimmed_curve_chain() {
    let mut cur = Cursor::new(prt_with_partition(&forward_trimmed_curve_chain_stream()));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let edge = result.ir.model.edges.first().expect("edge");
    assert_eq!(edge.curve.as_ref(), Some(&result.ir.model.curves[0].id));
    assert_eq!(edge.param_range, Some([0.25, 0.75]));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_retains_a_curve_when_its_trim_range_misses_edge_vertices() {
    let mut cur = Cursor::new(prt_with_partition(
        &mismatched_trimmed_topology_partition_stream(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let edge = result.ir.model.edges.first().expect("edge");
    let carrier = edge
        .curve
        .as_ref()
        .and_then(|id| result.ir.model.curves.iter().find(|curve| curve.id == *id))
        .expect("edge carrier");
    assert!(matches!(carrier.geometry, CurveGeometry::Line { .. }));
    assert_eq!(edge.param_range, None);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_omits_overflowing_line_trim_range() {
    let mut stream = trimmed_topology_partition_stream();
    let trim = stream
        .windows(4)
        .position(|window| window == [0, 133, 0, 12])
        .expect("trimmed curve");
    put_f64(&mut stream, trim + 69, f64::MAX);

    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.edges[0].param_range, None);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_resolves_extended_xmt_reference_inside_edge_record() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_extended_edge_curve_reference(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.curves[0].id)
    );
}

#[test]
fn decode_tracks_extended_face_reference_shift() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_extended_face_attribute_reference(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.faces[0].tolerance, Some(0.2));
    assert_eq!(
        result.ir.model.faces[0].surface,
        result.ir.model.surfaces[0].id
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_tracks_extended_edge_reference_shift() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_extended_edge_attribute_reference(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.edges[0].tolerance, Some(0.3));
    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.curves[0].id)
    );
}

#[test]
fn decode_tracks_all_extended_topology_reference_shifts() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_extended_internal_topology_references(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.shells.len(), 1);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert_eq!(result.ir.model.vertices[0].tolerance, Some(0.1));
    assert_eq!(result.ir.model.points[0].position.x, 10.0);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_tracks_fully_extended_geometry_header_shift() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_fully_extended_geometry_headers(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 1);
    assert!(matches!(
        result.ir.model.surfaces[0].geometry,
        SurfaceGeometry::Plane { .. }
    ));
    assert!(matches!(
        result.ir.model.curves[0].geometry,
        CurveGeometry::Line { .. }
    ));
}

#[test]
fn decode_tracks_geometry_envelope_escape_shift() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_escaped_geometry_envelopes(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(matches!(
        result.ir.model.surfaces[0].geometry,
        SurfaceGeometry::Plane { .. }
    ));
    assert!(matches!(
        result.ir.model.curves[0].geometry,
        CurveGeometry::Line { .. }
    ));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn cylinder_gate_rejects_denormal_radius() {
    // A coincidental byte alignment can present a unit axis and a model-scale
    // origin alongside a denormal (near-zero) double at the radius slot; the radius
    // floor must reject it rather than emit a fabricated zero-radius cylinder.
    let mut cy = record(0x33, 99);
    put_vec3(&mut cy, 19, [0.003_175, 0.0, 0.0]);
    put_vec3(&mut cy, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cy, 67, f64::from_bits(1)); // smallest positive subnormal
    put_vec3(&mut cy, 75, [1.0, 0.0, 0.0]);
    assert!(crate::geometry::surfaces(&cy).is_empty());
}

#[test]
fn graph_owned_analytic_geometry_has_no_scanner_magnitude_limit() {
    let mut cylinder = record(0x33, 99);
    put_vec3(&mut cylinder, 19, [1_001.0, 0.0, 0.0]);
    put_vec3(&mut cylinder, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cylinder, 67, f64::from_bits(1));
    put_vec3(&mut cylinder, 75, [1.0, 0.0, 0.0]);

    assert!(crate::geometry::surfaces(&cylinder).is_empty());
    let geometry =
        crate::geometry::decode_surface_record(&cylinder, 0x33, 0).expect("graph-owned cylinder");
    let SurfaceGeometry::Cylinder { origin, radius, .. } = geometry else {
        panic!("cylinder")
    };
    assert_eq!(origin.x, 1_001_000.0);
    assert_eq!(radius, f64::from_bits(1) * 1000.0);

    put_f64(&mut cylinder, 67, f64::INFINITY);
    assert!(crate::geometry::decode_surface_record(&cylinder, 0x33, 0).is_none());
}

#[test]
fn ellipse_requires_ordered_serialized_radii() {
    let mut ellipse = record(0x20, 107);
    put_vec3(&mut ellipse, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut ellipse, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut ellipse, 67, [1.0, 0.0, 0.0]);
    put_f64(&mut ellipse, 91, 0.01);
    put_f64(&mut ellipse, 99, 0.01 + 5.0e-10);

    assert!(crate::geometry::curves(&ellipse).is_empty());
    assert!(crate::geometry::decode_curve_record(&ellipse, 0x20, 0).is_none());

    put_f64(&mut ellipse, 99, 0.01);
    assert_eq!(crate::geometry::curves(&ellipse).len(), 1);
}

#[test]
fn graph_owned_point_has_no_scanner_magnitude_limit() {
    let mut stream = topology_partition_stream();
    let point = stream
        .windows(4)
        .position(|window| window == [0, 29, 0, 11])
        .expect("point record");
    put_vec3(&mut stream, point + 16, [1_001.0, f64::from_bits(1), 0.0]);

    assert!(crate::geometry::points(&stream).is_empty());
    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(
        graph
            .get(29, 11)
            .and_then(crate::topology::Node::point_position),
        Some(cadmpeg_ir::math::Point3::new(
            1_001_000.0,
            f64::from_bits(1) * 1000.0,
            0.0,
        ))
    );

    put_vec3(&mut stream, point + 16, [f64::INFINITY, 0.0, 0.0]);
    assert!(crate::topology::Graph::parse(&stream).get(29, 11).is_none());
}

#[test]
fn decoded_tolerance_has_no_model_magnitude_limit() {
    assert_eq!(crate::decode::decoded_tolerance(1_001.0), Some(1_001_000.0));
    assert_eq!(crate::decode::decoded_tolerance(0.0), None);
    assert_eq!(crate::decode::decoded_tolerance(f64::INFINITY), None);
    assert_eq!(crate::decode::decoded_tolerance(f64::MAX), None);
}

#[test]
fn analytic_frame_gate_rejects_nonorthogonal_reference_direction() {
    let mut plane = record(0x32, 91);
    put_vec3(&mut plane, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut plane, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut plane, 67, [0.0, 0.0, 1.0]);
    assert!(crate::geometry::surfaces(&plane).is_empty());

    put_vec3(&mut plane, 67, [1.0, 0.0, 0.0]);
    assert_eq!(crate::geometry::surfaces(&plane).len(), 1);
}

#[test]
fn cone_gate_rejects_nonfinite_or_degenerate_half_angle() {
    let mut cone = record(0x34, 115);
    put_vec3(&mut cone, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut cone, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cone, 67, 0.0);
    put_f64(&mut cone, 75, std::f64::consts::FRAC_1_SQRT_2);
    put_f64(&mut cone, 83, std::f64::consts::FRAC_1_SQRT_2);
    put_vec3(&mut cone, 91, [1.0, 0.0, 0.0]);
    assert_eq!(crate::geometry::surfaces(&cone).len(), 1);

    for (sine, cosine) in [(f64::NAN, 1.0), (0.0, 1.0), (1.0, 0.0)] {
        put_f64(&mut cone, 75, sine);
        put_f64(&mut cone, 83, cosine);
        assert!(crate::geometry::surfaces(&cone).is_empty());
    }
}

#[test]
fn analytic_scanners_include_extended_reference_shifts_in_record_ownership() {
    let mut surfaces = vec![0; 182];
    surfaces[1] = 0x32;
    put_vec3(&mut surfaces, 21, [0.0, 0.0, 0.0]);
    put_vec3(&mut surfaces, 45, [0.0, 0.0, 1.0]);
    put_vec3(&mut surfaces, 69, [1.0, 0.0, 0.0]);
    surfaces[91] = 0;
    surfaces[92] = 0x32;
    put_vec3(&mut surfaces, 110, [0.0, 0.0, 0.0]);
    put_vec3(&mut surfaces, 134, [0.0, 0.0, 1.0]);
    put_vec3(&mut surfaces, 158, [1.0, 0.0, 0.0]);
    assert_eq!(crate::geometry::surfaces(&surfaces).len(), 1);

    let mut curves = vec![0; 134];
    curves[1] = 0x1e;
    put_vec3(&mut curves, 21, [0.0, 0.0, 0.0]);
    put_vec3(&mut curves, 45, [1.0, 0.0, 0.0]);
    curves[67] = 0;
    curves[68] = 0x1e;
    put_vec3(&mut curves, 86, [0.0, 0.0, 0.0]);
    put_vec3(&mut curves, 110, [1.0, 0.0, 0.0]);
    assert_eq!(crate::geometry::curves(&curves).len(), 1);
}

#[test]
fn analytic_record_ownership_is_shared_across_carrier_families() {
    let mut stream = vec![0; 158];
    stream[1] = 0x1e;
    put_vec3(&mut stream, 21, [0.0, 0.0, 0.0]);
    put_vec3(&mut stream, 45, [1.0, 0.0, 0.0]);

    stream[67] = 0;
    stream[68] = 0x32;
    put_vec3(&mut stream, 86, [0.0, 0.0, 0.0]);
    put_vec3(&mut stream, 110, [0.0, 0.0, 1.0]);
    put_vec3(&mut stream, 134, [1.0, 0.0, 0.0]);

    assert_eq!(crate::geometry::curves(&stream).len(), 1);
    assert!(crate::geometry::surfaces(&stream).is_empty());
    assert!(crate::geometry::points(&stream).is_empty());
}

#[test]
fn decode_assembly_reports_external_dependency() {
    let mut cur = Cursor::new(assembly_prt());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert!(!result.report.geometry_transferred);
    assert!(result
        .report
        .losses
        .iter()
        .any(|l| l.message.contains("assembly")));
}

#[test]
fn external_reference_string_table_is_end_anchored() {
    let table = b"prefix\x01\x02\x00\x00\x00\x09\x00child.prt\x0c\x00nested/b.prt";
    let (_, strings) = crate::container::parse_extref_string_table(table).expect("string table");
    assert_eq!(
        strings
            .into_iter()
            .map(|(_, value)| value)
            .collect::<Vec<_>>(),
        ["child.prt", "nested/b.prt"]
    );

    let mut trailed = table.to_vec();
    trailed.push(0);
    assert!(crate::container::parse_extref_string_table(&trailed).is_none());
    assert!(crate::container::parse_extref_string_table(b"\x01\xff\xff\xff\xff").is_none());
}

#[test]
fn external_reference_record_parser_requires_sorted_doubled_handle_set() {
    let mut payload = b"EXTREFSTREAM".to_vec();
    payload.extend_from_slice(&3u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.push(0);
    payload.extend_from_slice(&6u32.to_le_bytes());
    payload.extend_from_slice(&41u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    assert_eq!(payload.len(), 41);
    payload.extend_from_slice(&[1, 0, 0, 0]);
    payload.extend_from_slice(&2u16.to_be_bytes());
    payload.push(1);
    for value in [8u32, 11, 12, 4] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(&[1, 4]);
    for handle in [0x1020_3040u32, 0x2030_4050, 0x2030_4050] {
        payload.push(0xe0);
        payload.extend_from_slice(&handle.to_be_bytes());
    }
    payload.push(4);
    payload.extend_from_slice(b"\x01\x01\x00\x00\x00\x09\x00child.prt");

    let records = crate::container::parse_extref_records(&payload);
    let indexed = crate::container::parse_extref_record_index(&payload).expect("record index");
    assert_eq!(indexed.len(), 1);
    assert_eq!(indexed[0].record_id, 6);
    assert_eq!(indexed[0].offset, 41);
    assert_eq!(indexed[0].byte_len, 41);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].record_id, 6);
    assert_eq!(records[0].declared_count, 2);
    assert_eq!(records[0].id_slots, [8, 11, 12, 4]);
    assert_eq!(records[0].handles, [0x1020_3040, 0x2030_4050]);
    assert!(records[0].closing_duplicate);
    assert_eq!(records[0].tail_byte_len, 0);

    let duplicate = payload
        .windows(5)
        .rposition(|window| window == [0xe0, 0x20, 0x30, 0x40, 0x50])
        .expect("closing duplicate");
    payload[duplicate + 1] = 0x10;
    assert!(crate::container::parse_extref_records(&payload).is_empty());
    assert_eq!(
        crate::container::parse_extref_record_index(&payload)
            .expect("opaque indexed record")
            .len(),
        1
    );
}

#[test]
fn external_reference_empty_record_parser_requires_the_complete_form() {
    assert_eq!(
        crate::container::parse_extref_empty_record(&[1, 0, 0, 0, 0, 1]),
        Some(false)
    );
    assert_eq!(
        crate::container::parse_extref_empty_record(&[1, 0, 0, 0, 0, 1, 1]),
        Some(true)
    );
    assert_eq!(
        crate::container::parse_extref_empty_record(&[1, 0, 0, 0, 0, 1, 0]),
        None
    );
    assert_eq!(
        crate::container::parse_extref_empty_record(&[1, 0, 0, 0, 0]),
        None
    );
}

#[test]
fn external_reference_tail_pairs_require_adjacent_complete_tokens() {
    let bytes = [
        0xff, 0xe0, 0x12, 0x34, 0x56, 0x78, 0xca, 0xbc, 0xde, 0xf0, 0xe0, 0x00, 0x00, 0x00, 0x01,
        0x00,
    ];
    assert_eq!(
        crate::container::parse_extref_reference_pairs(&bytes),
        vec![(1, 0x1234_5678, 0x0abc_def0)]
    );
    assert!(crate::container::parse_extref_reference_pairs(&bytes[10..]).is_empty());
}

#[test]
fn container_reads_rmfastload_active_ids() {
    let container = container::scan_bytes(rmfastload_prt()).unwrap();
    let (entry, table) = container
        .rmfastload_object_id_table()
        .expect("RMFastLoad object-id table");
    assert_eq!(entry.name, "/Root/FastLoad/RMFastLoad");
    assert_eq!(table.registry_offset, 0);
    assert_eq!(table.count_offset, b"UGS::Solid::Topol".len());
    assert_eq!(table.raw_count, 50u32.to_le_bytes());
    assert_eq!(
        table
            .object_ids
            .iter()
            .map(|object_id| object_id.value)
            .collect::<Vec<_>>(),
        (1..=50).collect::<Vec<_>>()
    );
    assert_eq!(table.object_ids[0].offset, table.count_offset + 4);
    assert_eq!(table.object_ids[0].raw, 1u32.to_le_bytes());
    assert_eq!(table.object_ids[49].offset, table.count_offset + 4 + 49 * 4);
    assert_eq!(table.object_ids[49].raw, 50u32.to_le_bytes());
}

#[test]
fn decode_retains_every_rmfastload_active_body() {
    let mut cur = Cursor::new(prt_with_two_active_bodies_and_rmfastload());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.bodies.len(), 2);
    assert_eq!(result.ir.model.faces.len(), 100);
    assert_eq!(
        result
            .ir
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("rmfastload_active_body_count"))
            .map(String::as_str),
        Some("2")
    );
    assert!(result
        .report
        .losses
        .iter()
        .all(|loss| !loss.message.contains("sub-body partition")));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_selects_active_shell_when_body_record_is_absent() {
    let mut cur = Cursor::new(prt_with_missing_active_body_record());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.bodies.len(), 1);
    assert!(result.ir.model.bodies[0].id.0.starts_with("nx:s0:"));
    assert_eq!(result.ir.model.faces.len(), 50);
    assert!(result
        .report
        .losses
        .iter()
        .all(|loss| !loss.message.contains("sub-body partition")));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_keeps_bodies_when_rmfastload_overlap_is_weak() {
    let mut cur = Cursor::new(prt_with_weak_rmfastload_overlap());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.bodies.len(), 2);
    assert!(result
        .ir
        .source
        .as_ref()
        .is_none_or(|source| !source.attributes.contains_key("active_body_selector")));
    assert!(result
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("sub-body partition")));
}

#[test]
fn container_only_preserves_streams_without_geometry() {
    let mut cur = Cursor::new(single_part_prt());
    let opts = options_in(DecodeMode::Salvage, true);
    let result = NxCodec.decode(&mut cur, &opts).unwrap();
    assert!(!result.report.geometry_transferred);
    assert!(result.report.container_only);
    assert_eq!(result.ir.native_unknowns("nx").unwrap().len(), 1);
    assert!(result.ir.model.points.is_empty());
}

#[test]
fn inspect_enumerates_streams_and_names_schema() {
    let mut cur = Cursor::new(single_part_prt());
    let summary = NxCodec
        .inspect(&mut cur, &InspectOptions::default())
        .unwrap();
    assert_eq!(summary.format, "nx");
    assert_eq!(summary.container_kind, "splmsstr");
    assert!(summary.entries.iter().any(|e| e.role == "parasolid-stream"));
    assert!(summary.notes.iter().any(|n| n.contains("partition")));
}

#[test]
fn design_intent_losses_distinguish_native_and_sketch_gaps() {
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::features::{
        ConfigurationBodies, ConfigurationId, DesignConfiguration, Feature, FeatureDefinition,
        FeatureId,
    };

    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    for (ordinal, kind) in ["DELETE", "DELETE"].into_iter().enumerate() {
        ir.model.features.push(Feature {
            id: FeatureId(format!("test:feature#{ordinal}")),
            ordinal: ordinal as u64,
            name: None,
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Native {
                kind: kind.to_string(),
                parameters: Default::default(),
                properties: Default::default(),
            },
            native_ref: None,
        });
    }
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#sketch".into()),
        ordinal: 3,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Unresolved,
            sketch: None,
        },
        native_ref: None,
    });
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#incomplete-delete".into()),
        ordinal: 10,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::DeleteBody {
            bodies: cadmpeg_ir::features::BodySelection::Unresolved,
            mode: cadmpeg_ir::features::BodyRetentionMode::DeleteSelected,
        },
        native_ref: None,
    });
    for (ordinal, definition) in [
        FeatureDefinition::DatumPlaneUnresolved,
        FeatureDefinition::DatumCoordinateSystemUnresolved,
        FeatureDefinition::LoftUnresolved,
        FeatureDefinition::FreeformSurfaceUnresolved,
        FeatureDefinition::LoftUnresolved,
    ]
    .into_iter()
    .enumerate()
    {
        ir.model.features.push(Feature {
            id: FeatureId(format!("test:feature#unresolved-{ordinal}")),
            ordinal: ordinal as u64 + 4,
            name: None,
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: None,
        });
    }
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#incomplete-block".into()),
        ordinal: 9,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Block {
            dimensions: None,
            placement: None,
        },
        native_ref: None,
    });
    ir.model.configurations.extend([
        DesignConfiguration {
            id: ConfigurationId("test:configuration#0".into()),
            ordinal: 0,
            active: true,
            source_index: Some(0),
            name: "Model".into(),
            material: None,
            properties: Default::default(),
            parameter_overrides: Default::default(),
            suppressed_features: Vec::new(),
            bodies: ConfigurationBodies::Resolved(Vec::new()),
            parameter_values: Default::default(),
            feature_states: Default::default(),
            native_ref: None,
        },
        DesignConfiguration {
            id: ConfigurationId("test:configuration#1".into()),
            ordinal: 1,
            active: false,
            source_index: Some(1),
            name: "Arrangement".into(),
            material: None,
            properties: Default::default(),
            parameter_overrides: Default::default(),
            suppressed_features: Vec::new(),
            bodies: ConfigurationBodies::Unresolved,
            parameter_values: Default::default(),
            feature_states: Default::default(),
            native_ref: None,
        },
    ]);

    let mut losses = Vec::new();
    crate::decode::append_design_intent_losses(&ir, &mut losses);

    assert_eq!(losses.len(), 6);
    assert_eq!(losses[0].category, LossCategory::DesignIntent);
    assert!(losses[0].message.contains("9 NX feature history operation"));
    assert_eq!(losses[1].category, LossCategory::DesignIntent);
    assert!(losses[1].message.contains("1 NX design configuration"));
    assert_eq!(losses[2].category, LossCategory::DesignIntent);
    assert!(losses[2].message.contains("DELETE (2)"));
    assert_eq!(losses[3].category, LossCategory::DesignIntent);
    assert!(losses[3].message.contains("datum coordinate system (1)"));
    assert!(losses[3].message.contains("datum plane (1)"));
    assert!(losses[3].message.contains("freeform surface (1)"));
    assert!(losses[3].message.contains("loft (2)"));
    assert_eq!(losses[4].category, LossCategory::DesignIntent);
    assert!(losses[4].message.contains("block (1)"));
    assert!(losses[4].message.contains("delete body (1)"));
    assert!(losses[4].message.contains("sketch (1)"));
    assert_eq!(losses[5].category, LossCategory::DesignIntent);
    assert!(losses[5].message.contains("1 NX sketch history feature"));
    assert!(losses[5].message.contains("1 have no neutral sketch graph"));

    let sketch_id = cadmpeg_ir::sketches::SketchId("test:sketch#0".into());
    ir.model.sketches.push(cadmpeg_ir::sketches::Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            u_axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    });
    ir.model.features[2].definition = FeatureDefinition::Sketch {
        space: cadmpeg_ir::features::SketchSpace::Planar,
        sketch: Some(sketch_id),
    };
    losses.clear();
    crate::decode::append_design_intent_losses(&ir, &mut losses);

    assert_eq!(losses.len(), 6);
    assert!(!losses[4].message.contains("sketch"));
    assert!(losses[5].message.contains("no sketch constraints"));
}

#[test]
fn extraction_uses_ug_part_bounds_and_all_standard_zlib_headers() {
    let part = zlib_compress_at_level(&partition_stream(), 6);
    assert_eq!(&part[..2], b"\x78\x9c");

    let mut decoy_stream = partition_stream();
    let schema = b"SCH_TEST_1_9999";
    let decoy = b"SCH_FAKE_1_9999";
    let pos = decoy_stream
        .windows(schema.len())
        .position(|w| w == schema)
        .unwrap();
    decoy_stream[pos..pos + schema.len()].copy_from_slice(decoy);
    let decoy = zlib_compress(&decoy_stream);

    let mut file = Vec::new();
    file.extend_from_slice(MAGIC);
    file.push(0x06);
    file.extend_from_slice(&[0; 3 + 4 + 1 + 6 + 2]);
    file.extend_from_slice(b"HEADER");
    let entries = [
        (b"/Root/UG_PART/UG_PART".as_slice(), part.len()),
        (b"/Root/FastLoad/JT".as_slice(), decoy.len()),
    ];
    let directory_len: usize = entries.iter().map(|(name, _)| 4 + name.len() + 16).sum();
    let mut next_offset = file.len() + directory_len;
    for (name, size) in &entries {
        file.extend_from_slice(&(name.len() as u32).to_le_bytes());
        file.extend_from_slice(name);
        file.extend_from_slice(&(next_offset as u64).to_le_bytes());
        file.extend_from_slice(&(*size as u64).to_le_bytes());
        next_offset += size;
    }
    file.extend_from_slice(&part);
    file.extend_from_slice(&decoy);

    let streams = extract_streams(&file);
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].schema.as_deref(), Some("SCH_TEST_1_9999"));
}

/// Phase 0 golden serialized-output snapshots.
///
/// These freeze the NX codec's complete observable output before the native-tier
/// refactor begins. For each fixture the harness runs `NxCodec::decode` and
/// `NxCodec::inspect`, then serializes the full [`DecodeResult`] (the decoded
/// `CadIr` including the `nx` native-namespace arenas, the [`DecodeReport`], and
/// the [`SourceFidelity`] sidecar carrying provenance/exactness annotations) plus
/// the [`ContainerSummary`] into one deterministic pretty-JSON document, compared
/// byte-for-byte against a committed golden file under `tests/golden/`.
///
/// Serialization goes through `serde_json::to_value` (whose object maps are
/// `BTreeMap`, so keys sort) and then `to_string_pretty`. Every IR container that
/// reaches the wire is `BTreeMap`- or `Vec`-backed and codec output is sorted by
/// id, so the bytes are stable across runs; `golden_output_is_deterministic`
/// asserts that directly.
///
/// Regenerate after an intended output change with:
///   `UPDATE_GOLDEN=1 cargo test-fast golden`
/// then review the golden diff before committing. Regenerate with the workspace
/// feature set (`test-fast` / `--workspace`), NOT `-p cadmpeg-codec-nx`: the
/// fixtures zlib-compress their streams through `flate2`, and Cargo feature
/// unification selects the `zlib-rs` backend for the full-workspace build but
/// `miniz_oxide` for an isolated crate build. The two backends emit different
/// compressed bytes, so the container byte length, `sha256`, and byte-ledger
/// totals in these snapshots are only stable under the workspace build (the one
/// the commit hook and CI run). This is a build-config sensitivity of the
/// fixtures, not codec nondeterminism: `golden_output_is_deterministic` confirms
/// decode output is a pure function of the input bytes.
mod golden {
    use std::collections::BTreeSet;
    use std::io::Cursor;
    use std::path::{Path, PathBuf};

    use cadmpeg_ir::codec::{CodecEntry, DecodeOptions};

    use super::*;

    /// Every arena name production writes via `set_arena` in `decode.rs`, extracted
    /// mechanically. This is the coverage denominator; `arena_coverage_is_a_subset`
    /// fails if production introduces an arena name this list does not know, which
    /// keeps the denominator honest as the code evolves.
    const KNOWN_ARENAS: &[&str] = &[
        "class_definitions",
        "configuration_attribute_uses",
        "configurations",
        "data_block_abr_reference_lanes",
        "data_block_column_index_tables",
        "data_block_control_class_references",
        "data_block_control_handle_pairs",
        "data_block_control_index_values",
        "data_block_control_references",
        "data_block_control_values",
        "data_block_counted_index_lanes",
        "data_block_index_rows",
        "data_block_linked_index_rows",
        "data_block_object_frames",
        "data_block_references",
        "data_block_target_index_rows",
        "data_blocks",
        "display_jt_base_node_data",
        "display_jt_compressed_element_sequences",
        "display_jt_compressed_elements",
        "display_jt_coordinate_array_headers",
        "display_jt_documents",
        "display_jt_geometric_transform_attributes",
        "display_jt_group_node_data",
        "display_jt_indices",
        "display_jt_initial_face_degree_symbols",
        "display_jt_instance_nodes",
        "display_jt_partition_nodes",
        "display_jt_polygon_meshes",
        "display_jt_range_lod_nodes",
        "display_jt_segments",
        "display_jt_shape_lod_bindings",
        "display_jt_shape_lod_elements",
        "display_jt_string_property_atoms",
        "display_jt_topology_packet_sequences",
        "display_jt_tri_strip_lod_headers",
        "display_jt_tri_strip_shape_nodes",
        "display_jt_vertex_colors",
        "display_jt_vertex_coordinates",
        "display_jt_vertex_flags",
        "display_jt_vertex_normals",
        "display_jt_vertex_records_headers",
        "display_jt_vertex_texture_coordinates",
        "expression_declarations",
        "expressions",
        "external_reference_empty_records",
        "external_reference_indexed_records",
        "external_reference_record_children",
        "external_reference_record_string_uses",
        "external_reference_records",
        "external_reference_tail_reference_pairs",
        "external_references",
        "feature_block_construction_payloads",
        "feature_block_construction_references",
        "feature_block_constructions",
        "feature_block_dimensions",
        "feature_block_payload_named_records",
        "feature_block_payload_names",
        "feature_block_payload_point_groups",
        "feature_block_payload_points",
        "feature_block_payload_scalars",
        "feature_body_reference_occurrences",
        "feature_body_references",
        "feature_body_segment_uses",
        "feature_boolean_operations",
        "feature_datum_csys_block_uses",
        "feature_datum_csys_constructions",
        "feature_datum_csys_descriptors",
        "feature_datum_csys_payload_fixed_pairs",
        "feature_datum_csys_payload_scalar_pairs",
        "feature_datum_csys_payload_scalars",
        "feature_datum_csys_payloads",
        "feature_datum_plane_block_uses",
        "feature_datum_plane_csys_identity_uses",
        "feature_datum_plane_descriptors",
        "feature_datum_plane_headers",
        "feature_datum_plane_payload_scalar_pairs",
        "feature_datum_plane_payloads",
        "feature_draft_construction_binary32_lanes",
        "feature_draft_construction_fixed_lanes",
        "feature_draft_construction_graph_payloads",
        "feature_draft_construction_graph_strings",
        "feature_draft_construction_identity_frames",
        "feature_draft_construction_index_lanes",
        "feature_draft_construction_payloads",
        "feature_draft_construction_references",
        "feature_draft_construction_terminal_lanes",
        "feature_extrude_32_constructions",
        "feature_extrude_construction_profiles",
        "feature_extrude_payload_32_branches",
        "feature_extrude_payload_footers",
        "feature_extrude_payload_headers",
        "feature_extrude_profile_references",
        "feature_input_block_identity_groups",
        "feature_input_blocks",
        "feature_input_column_row_uses",
        "feature_input_column_targets",
        "feature_operation_body_11_continuations",
        "feature_operation_body_members",
        "feature_operation_body_operands",
        "feature_operation_body_reference_lanes",
        "feature_operation_body_scalar_triples",
        "feature_operation_labels",
        "feature_operation_records",
        "feature_parameter_bindings",
        "feature_parameter_uses",
        "feature_pattern_construction_fixed_lanes",
        "feature_pattern_construction_payloads",
        "feature_pattern_construction_strings",
        "feature_pattern_references",
        "feature_pattern_transform_lanes",
        "feature_payload_strings",
        "feature_point_construction_headers",
        "feature_point_construction_scalar_lanes",
        "feature_projected_curve_construction_payloads",
        "feature_projected_curve_construction_strings",
        "feature_projected_curve_references",
        "feature_simple_hole_construction_groups",
        "feature_simple_hole_repeated_scalar_lane_block_references",
        "feature_simple_hole_repeated_scalar_lanes",
        "feature_simple_hole_templates",
        "feature_sketch_construction_inputs",
        "feature_sketch_construction_payloads",
        "feature_sketch_datum_csys_dependencies",
        "feature_sketch_fixed_points",
        "feature_sketch_named_point_block_uses",
        "feature_sketch_payload_coordinate_pairs",
        "feature_sketch_payload_fixed_pairs",
        "feature_sketch_payload_named_records",
        "feature_sketch_payload_names",
        "feature_sketch_payload_scalars",
        "feature_sketch_point_groups",
        "feature_sketch_point_uses",
        "feature_sketch_points",
        "feature_sketch_preceding_named_point_uses",
        "feature_sketch_records",
        "feature_sketch_references",
        "feature_surface_construction_branches",
        "feature_surface_construction_payloads",
        "feature_surface_construction_references",
        "feature_surface_construction_scalar_pairs",
        "feature_surface_construction_strings",
        "field_definitions",
        "material_texture_assets",
        "material_texture_catalog_entries",
        "object_records",
        "object_references",
        "offset_store_named_points",
        "om_record_areas",
        "parasolid_attribute_class_uses",
        "parasolid_attribute_definitions",
        "parasolid_blend_bound_records",
        "parasolid_blend_surface_records",
        "parasolid_chart_records",
        "parasolid_entity_51_numeric_uses",
        "parasolid_entity_51_records",
        "parasolid_entity_51_string_uses",
        "parasolid_entity_52_integer_records",
        "parasolid_entity_53_double_records",
        "parasolid_entity_54_string_records",
        "parasolid_intersection_records",
        "parasolid_offset_surface_records",
        "parasolid_support_uv_records",
        "parasolid_surface_curve_records",
        "parasolid_term_use_records",
        "parasolid_topology_attribute_class_uses",
        "parasolid_topology_attribute_list_references",
        "parasolid_trimmed_curve_records",
        "part_attributes",
        "persistent_handles",
        "rmfastload_object_id_tables",
        "rmfastload_object_ids",
        "segment_body_bindings",
        "segment_body_lineage_statuses",
        "segment_index_rows",
        "segment_om_links",
        "segment_stream_links",
        "store_headers",
        "string_values",
    ];

    /// A floor on distinct arenas the golden fixtures collectively populate.
    /// Frozen from the generated snapshots; if a refactor drops an arena from
    /// every fixture, `arena_coverage_meets_floor` fails. Raise it (never lower
    /// it) when new covering fixtures are added.
    const ARENA_COVERAGE_FLOOR: usize = 122;

    /// Build the covering fixture set: `(golden name, full `.prt` bytes)`. Each
    /// stream builder is wrapped exactly as its originating white-box test wraps
    /// it (`prt_with_partition` for a lone partition, `prt_with_streams` for a
    /// partition paired with an equal-schema deltas stream, `prt_with_named_payloads`
    /// for an OM record area), so the bytes exercise the real decode path.
    fn fixtures() -> Vec<(&'static str, Vec<u8>)> {
        let mut f: Vec<(&'static str, Vec<u8>)> = Vec::new();

        // Self-contained `.prt` images.
        f.push(("single_part_prt", single_part_prt()));
        f.push(("topology_part_prt", topology_part_prt()));
        f.push(("prt_with_arrangements", prt_with_arrangements()));
        f.push((
            "prt_with_arrangement_attribute_none",
            prt_with_arrangement_attribute(None),
        ));
        f.push(("prt_with_indexed_om_section", prt_with_indexed_om_section()));
        f.push((
            "prt_with_size_framed_om_section",
            prt_with_size_framed_om_section(),
        ));
        f.push(("assembly_prt", assembly_prt()));
        f.push((
            "assembly_with_external_paths",
            assembly_with_external_paths(),
        ));
        f.push(("rmfastload_prt", rmfastload_prt()));
        f.push((
            "prt_with_two_bodies_and_rmfastload",
            prt_with_two_bodies_and_rmfastload(),
        ));
        f.push((
            "prt_with_two_active_bodies_and_rmfastload",
            prt_with_two_active_bodies_and_rmfastload(),
        ));
        f.push((
            "prt_with_missing_active_body_record",
            prt_with_missing_active_body_record(),
        ));
        f.push((
            "prt_with_weak_rmfastload_overlap",
            prt_with_weak_rmfastload_overlap(),
        ));

        // Parasolid neutral-binary attribute/entity records in a partition stream.
        f.push((
            "parasolid_entity_records",
            prt_with_partition(&parasolid_entity_records_stream()),
        ));

        // Embedded DisplayJT stream: outer index, one JT document, one segment.
        f.push((
            "display_jt_basic",
            prt_with_named_payloads(&[
                ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
                ("/Root/UG_PART/DisplayJT", display_jt_basic_stream()),
            ]),
        ));
        f.push((
            "display_jt_scene_graph",
            prt_with_named_payloads(&[
                ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
                ("/Root/UG_PART/DisplayJT", display_jt_scene_graph_stream()),
            ]),
        ));
        f.push((
            "display_jt_shape_lod",
            prt_with_named_payloads(&[
                ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
                ("/Root/UG_PART/DisplayJT", display_jt_shape_lod_stream()),
            ]),
        ));
        f.push((
            "display_jt_string_property",
            prt_with_named_payloads(&[
                ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
                (
                    "/Root/UG_PART/DisplayJT",
                    display_jt_string_property_stream(),
                ),
            ]),
        ));

        // Offset-store control blocks: the plain form resolves class-registry
        // ordinals; the handle form carries two adjacent persistent handles.
        f.push((
            "data_block_control_class_references",
            prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", offset_only_indexed_om_section())]),
        ));
        f.push((
            "offset_store_named_point",
            prt_with_named_payloads(&[(
                "/Root/UG_PART/UG_PART",
                offset_only_indexed_om_section_with_named_point(),
            )]),
        ));
        f.push((
            "data_block_control_index_values",
            prt_with_named_payloads(&[(
                "/Root/UG_PART/UG_PART",
                offset_only_indexed_om_section_with_index_values(),
            )]),
        ));
        // EXTREFSTREAM index, string table, and handle-set records.
        f.push((
            "external_reference_stream",
            prt_with_named_payloads(&[
                ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
                ("/Root/ExternalReferences", external_reference_stream()),
            ]),
        ));

        f.push(("data_block_control_handles", {
            let mut control = Vec::new();
            control.extend_from_slice(&[0xe0, 0, 0, 0, 1]);
            control.extend_from_slice(&[0xe0, 0, 0, 0, 2]);
            prt_with_named_payloads(&[(
                "/Root/UG_PART/UG_PART",
                offset_only_indexed_om_section_with_control(&control),
            )])
        }));

        // OM record areas / feature history, wrapped as a named UG_PART payload.
        f.push((
            "om_record_area",
            prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_om_record_area_payload())]),
        ));
        f.push((
            "om_record_area_input_store",
            prt_with_named_payloads(&[(
                "/Root/UG_PART/UG_PART",
                segment_om_record_area_with_input_store_payload(),
            )]),
        ));
        f.push((
            "multi_section_feature_history",
            prt_with_named_payloads(&[(
                "/Root/UG_PART/UG_PART",
                multi_section_feature_history_payload(),
            )]),
        ));
        f.push(("composed_feature_history", composed_feature_history_prt()));
        f.push((
            "segment_index_rows",
            prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_index_payload())]),
        ));
        f.push((
            "segment_stream_links",
            prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_stream_payload())]),
        ));
        f.push((
            "segment_body_bindings",
            prt_with_named_payloads(&[(
                "/Root/UG_PART/UG_PART",
                segment_body_binding_payload("partition"),
            )]),
        ));
        f.push((
            "material_texture_assets",
            prt_with_named_payloads(&[
                ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
                (
                    "/Root/materialsTif/AISI Steel 4340",
                    vec![b'I', b'I', 42, 0, 8, 0, 0, 0, 0, 0],
                ),
                (
                    "/Root/materialsTif/Truncated",
                    vec![b'I', b'I', 42, 0, 40, 0, 0, 0, 0, 0],
                ),
            ]),
        ));
        f.push(("material_texture_catalog", prt_with_named_payloads(&[
            ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
            ("/Root/materialsTif/unmap$1", vec![b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 0]),
            ("/Root/qafmetadata", br#"<?xml version="1.0" encoding="UTF-8"?>
<folderContents>
<folderProperties location="images/preview" unmappedLocation="images/preview"><createTime>2026-07-15T08:00:00</createTime><modifyTime>2026-07-15T08:00:01</modifyTime></folderProperties>
<folderProperties location="materialsTif/unmap$1" unmappedLocation="materialsTif/Carbon Fiber Harness Satin Coated"><createTime>2026-07-15T08:01:00</createTime><modifyTime>2026-07-15T08:02:00</modifyTime></folderProperties>
</folderContents>"#.to_vec()),
        ])));
        f.push(("om_repeated_operations", {
            let section = size_framed_om_section_with_repeated_operations(12);
            let mut payload = Vec::new();
            for word in [24_u32, 9, 11, 1, 1, 24] {
                payload.extend_from_slice(&word.to_le_bytes());
            }
            payload.extend_from_slice(&section);
            prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)])
        }));

        // Lone partition streams, each wrapped with `prt_with_partition`.
        let partitions: Vec<(&'static str, Vec<u8>)> = vec![
            ("topology_partition_stream", topology_partition_stream()),
            (
                "topology_with_missing_tolerances",
                topology_with_missing_tolerances(),
            ),
            ("partition_stream", partition_stream()),
            (
                "offset_surface_topology_partition_stream",
                offset_surface_topology_partition_stream(),
            ),
            (
                "offset_surface_with_fully_extended_common_header",
                offset_surface_with_fully_extended_common_header(),
            ),
            (
                "surface_curve_topology_partition_stream",
                surface_curve_topology_partition_stream(),
            ),
            (
                "pcurve_topology_partition_stream",
                pcurve_topology_partition_stream(),
            ),
            (
                "shared_region_shells_partition_stream",
                shared_region_shells_partition_stream(),
            ),
            (
                "blend_surface_topology_partition_stream",
                blend_surface_topology_partition_stream(),
            ),
            (
                "blend_surface_with_extended_support_reference",
                blend_surface_with_extended_support_reference(),
            ),
            (
                "blend_surface_with_intersection_spine",
                blend_surface_with_intersection_spine(),
            ),
            (
                "blend_surface_with_forward_blend_support",
                blend_surface_with_forward_blend_support(),
            ),
            (
                "intersection_curve_topology_partition_stream",
                intersection_curve_topology_partition_stream(),
            ),
            (
                "charted_intersection_curve_topology_partition_stream",
                charted_intersection_curve_topology_partition_stream(),
            ),
            (
                "charted_intersection_with_edge_endpoint_witnesses_stream",
                charted_intersection_with_edge_endpoint_witnesses_stream(),
            ),
            (
                "charted_intersection_without_uv_stream",
                charted_intersection_without_uv_stream(),
            ),
            (
                "charted_intersection_with_approximated_term_stream",
                charted_intersection_with_approximated_term_stream(),
            ),
            (
                "ext11_charted_intersection_curve_stream",
                ext11_charted_intersection_curve_stream(),
            ),
            (
                "two_support_ext11_charted_intersection_curve_stream",
                two_support_ext11_charted_intersection_curve_stream(false),
            ),
            (
                "two_support_ext11_charted_intersection_curve_stream_ambiguous",
                two_support_ext11_charted_intersection_curve_stream(true),
            ),
            (
                "partial_ext11_charted_intersection_curve_stream",
                partial_ext11_charted_intersection_curve_stream(),
            ),
            (
                "two_support_charted_intersection_curve_stream",
                two_support_charted_intersection_curve_stream(),
            ),
            (
                "blend_bound_charted_intersection_curve_stream",
                blend_bound_charted_intersection_curve_stream(),
            ),
            (
                "inline_descriptor_intersection_curve_stream",
                inline_descriptor_intersection_curve_stream(),
            ),
            (
                "circle_topology_partition_stream",
                circle_topology_partition_stream(),
            ),
            (
                "ellipse_topology_partition_stream",
                ellipse_topology_partition_stream(),
            ),
            (
                "cylinder_topology_partition_stream",
                cylinder_topology_partition_stream(),
            ),
            (
                "cone_topology_partition_stream",
                cone_topology_partition_stream(),
            ),
            (
                "sphere_topology_partition_stream",
                sphere_topology_partition_stream(),
            ),
            (
                "torus_topology_partition_stream",
                torus_topology_partition_stream(),
            ),
            ("bspline_partition_stream", bspline_partition_stream()),
            (
                "extended_bspline_surface_stream",
                extended_bspline_surface_stream(),
            ),
            (
                "bspline_surface_replacement_partition_stream",
                bspline_surface_replacement_partition_stream(),
            ),
            (
                "bspline_curve_replacement_partition_stream",
                bspline_curve_replacement_partition_stream(),
            ),
            (
                "trimmed_topology_partition_stream",
                trimmed_topology_partition_stream(),
            ),
            (
                "mismatched_trimmed_topology_partition_stream",
                mismatched_trimmed_topology_partition_stream(),
            ),
            (
                "partnered_trimmed_topology_partition_stream",
                partnered_trimmed_topology_partition_stream(),
            ),
            (
                "forward_trimmed_curve_chain_stream",
                forward_trimmed_curve_chain_stream(),
            ),
            (
                "topology_with_extended_edge_curve_reference",
                topology_with_extended_edge_curve_reference(),
            ),
            (
                "topology_with_extended_face_attribute_reference",
                topology_with_extended_face_attribute_reference(),
            ),
            (
                "topology_with_extended_edge_attribute_reference",
                topology_with_extended_edge_attribute_reference(),
            ),
            (
                "topology_with_extended_internal_topology_references",
                topology_with_extended_internal_topology_references(),
            ),
            (
                "topology_with_fully_extended_geometry_headers",
                topology_with_fully_extended_geometry_headers(),
            ),
            (
                "topology_with_escaped_geometry_envelopes",
                topology_with_escaped_geometry_envelopes(),
            ),
            (
                "deltas_intersection_curve_stream",
                deltas_intersection_curve_stream(),
            ),
            ("status_framed_deltas_stream", status_framed_deltas_stream()),
            (
                "variable_status_framed_deltas_stream",
                variable_status_framed_deltas_stream(),
            ),
            (
                "status_framed_deltas_point_stream",
                status_framed_deltas_point_stream(),
            ),
            (
                "deltas_point_partition_stream",
                deltas_point_partition_stream(),
            ),
            ("many_face_partition_stream", many_face_partition_stream(1)),
            (
                "large_xmt_headers_topology",
                large_xmt_headers(&topology_partition_stream()),
            ),
        ];
        for (name, stream) in partitions {
            f.push((name, prt_with_partition(&stream)));
        }

        // Deltas streams paired with an equal-schema partition via `prt_with_streams`.
        let deltas_pairs: Vec<(&'static str, Vec<u8>, Vec<u8>)> = vec![
            (
                "deltas_edge",
                topology_partition_stream(),
                deltas_edge_partition_stream(),
            ),
            (
                "deltas_face_vertex",
                topology_partition_stream(),
                deltas_face_vertex_partition_stream(),
            ),
            (
                "deltas_loop",
                topology_partition_stream(),
                deltas_loop_partition_stream(),
            ),
            (
                "deltas_shell",
                topology_partition_stream(),
                deltas_shell_partition_stream(),
            ),
            (
                "deltas_fin",
                topology_partition_stream(),
                deltas_fin_partition_stream(),
            ),
            (
                "deltas_line",
                topology_partition_stream(),
                deltas_line_partition_stream(),
            ),
            (
                "deltas_plane",
                topology_partition_stream(),
                deltas_plane_partition_stream(),
            ),
            (
                "deltas_offset_surface",
                offset_surface_topology_partition_stream(),
                deltas_offset_surface_partition_stream(),
            ),
            (
                "deltas_blend_surface",
                blend_surface_topology_partition_stream(),
                deltas_blend_surface_partition_stream(),
            ),
            (
                "deltas_trimmed_curve",
                trimmed_topology_partition_stream(),
                deltas_trimmed_curve_partition_stream(),
            ),
            (
                "deltas_surface_curve",
                surface_curve_topology_partition_stream(),
                deltas_surface_curve_partition_stream(),
            ),
            (
                "deltas_circle",
                circle_topology_partition_stream(),
                deltas_circle_partition_stream(),
            ),
            (
                "deltas_ellipse",
                ellipse_topology_partition_stream(),
                deltas_ellipse_partition_stream(),
            ),
            (
                "deltas_cylinder",
                cylinder_topology_partition_stream(),
                deltas_cylinder_partition_stream(),
            ),
            (
                "deltas_cone",
                cone_topology_partition_stream(),
                deltas_cone_partition_stream(),
            ),
            (
                "deltas_sphere",
                sphere_topology_partition_stream(),
                deltas_sphere_partition_stream(),
            ),
            (
                "deltas_torus",
                torus_topology_partition_stream(),
                deltas_torus_partition_stream(),
            ),
            (
                "deltas_bspline_surface",
                bspline_surface_replacement_partition_stream(),
                deltas_bspline_surface_wrapper_stream(),
            ),
            (
                "deltas_bspline_curve",
                bspline_curve_replacement_partition_stream(),
                deltas_bspline_curve_wrapper_stream(),
            ),
        ];
        for (name, partition, delta) in deltas_pairs {
            f.push((name, prt_with_streams(&[&partition, &delta])));
        }

        f
    }

    /// Serialize the complete decode + inspect output for one fixture as stable
    /// pretty JSON. Decode/inspect errors are frozen too (a `.prt` that fails to
    /// decode is a real, contract-relevant behavior), so this never panics on
    /// codec output.
    fn snapshot(bytes: &[u8]) -> String {
        let decode =
            match NxCodec.decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default()) {
                Ok(result) => serde_json::json!({
                    "ir": serde_json::to_value(&result.ir).expect("serialize ir"),
                    "report": serde_json::to_value(&result.report).expect("serialize report"),
                    "source_fidelity": serde_json::to_value(&result.source_fidelity)
                        .expect("serialize source_fidelity"),
                }),
                Err(err) => serde_json::json!({ "decode_error": err.to_string() }),
            };
        let inspect =
            match NxCodec.inspect(&mut Cursor::new(bytes.to_vec()), &InspectOptions::default()) {
                Ok(summary) => serde_json::to_value(&summary).expect("serialize inspect"),
                Err(err) => serde_json::json!({ "inspect_error": err.to_string() }),
            };
        let combined = serde_json::json!({ "decode": decode, "inspect": inspect });
        let mut text = serde_json::to_string_pretty(&combined).expect("serialize snapshot");
        text.push('\n');
        text
    }

    fn golden_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
    }

    fn golden_path(name: &str) -> PathBuf {
        golden_dir().join(format!("{name}.json"))
    }

    /// First line that differs between two documents, 1-based, with both sides
    /// truncated for a readable failure. `None` when the shorter side is a prefix
    /// of the longer (length-only difference).
    fn first_line_diff(expected: &str, actual: &str) -> (usize, String, String) {
        let mut exp = expected.lines();
        let mut act = actual.lines();
        let mut line = 0usize;
        loop {
            line += 1;
            match (exp.next(), act.next()) {
                (Some(e), Some(a)) if e == a => {}
                (e, a) => {
                    let trunc = |s: Option<&str>| match s {
                        Some(s) if s.len() > 200 => format!("{}…", &s[..200]),
                        Some(s) => s.to_string(),
                        None => "<end of file>".to_string(),
                    };
                    return (line, trunc(e), trunc(a));
                }
            }
        }
    }

    fn first_byte_diff(expected: &str, actual: &str) -> String {
        let expected = expected.as_bytes();
        let actual = actual.as_bytes();
        let offset = expected
            .iter()
            .zip(actual)
            .position(|(expected, actual)| expected != actual)
            .unwrap_or_else(|| expected.len().min(actual.len()));
        let describe = |bytes: &[u8]| match bytes.get(offset) {
            Some(byte) => format!("0x{byte:02x}"),
            None => "<end of file>".to_string(),
        };
        format!(
            "first byte difference at offset {offset}: golden {}, actual {} (lengths: {} and {})",
            describe(expected),
            describe(actual),
            expected.len(),
            actual.len()
        )
    }

    fn update_requested() -> bool {
        std::env::var_os("UPDATE_GOLDEN").is_some()
    }

    #[test]
    fn golden_snapshots_are_byte_identical() {
        let update = update_requested();
        if update {
            std::fs::create_dir_all(golden_dir()).expect("create golden dir");
        }
        let mut failures: Vec<String> = Vec::new();
        for (name, bytes) in fixtures() {
            let actual = snapshot(&bytes);
            let path = golden_path(name);
            if update {
                std::fs::write(&path, actual.as_bytes())
                    .unwrap_or_else(|e| panic!("write golden {name}: {e}"));
                continue;
            }
            let expected = match std::fs::read_to_string(&path) {
                Ok(text) => text,
                Err(e) => {
                    failures.push(format!(
                        "fixture `{name}`: cannot read golden {} ({e}); run `UPDATE_GOLDEN=1 cargo test-fast golden`",
                        path.display()
                    ));
                    continue;
                }
            };
            if expected != actual {
                let (line, exp_line, act_line) = first_line_diff(&expected, &actual);
                failures.push(format!(
                    "fixture `{name}`: output diverged from golden at line {line}\n    golden: {exp_line}\n    actual: {act_line}\n    {}",
                    first_byte_diff(&expected, &actual)
                ));
            }
        }
        assert!(
            failures.is_empty(),
            "{} golden snapshot(s) drifted; if the change is intended run `UPDATE_GOLDEN=1 cargo test-fast golden` and review the diff:\n\n{}",
            failures.len(),
            failures.join("\n\n")
        );
    }

    /// Guards against nondeterministic codec output (`HashMap` iteration order,
    /// timestamps): decoding the same bytes twice must produce identical JSON.
    #[test]
    fn golden_output_is_deterministic() {
        for (name, bytes) in fixtures() {
            let first = snapshot(&bytes);
            let second = snapshot(&bytes);
            if first != second {
                let (line, a, b) = first_line_diff(&first, &second);
                panic!("fixture `{name}`: nondeterministic output at line {line}\n    run 1: {a}\n    run 2: {b}");
            }
        }
    }

    /// Union of `nx`-namespace arenas the fixture set populates.
    fn covered_arenas() -> BTreeSet<String> {
        let mut covered = BTreeSet::new();
        for (_, bytes) in fixtures() {
            let Ok(result) = NxCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            else {
                continue;
            };
            if let Some(namespace) = result.ir.native.namespace("nx") {
                for (arena, records) in &namespace.arenas {
                    if !records.is_empty() {
                        covered.insert(arena.clone());
                    }
                }
            }
        }
        covered
    }

    /// Every arena a fixture populates must be a name production actually writes.
    /// A failure here means `KNOWN_ARENAS` (the coverage denominator) is stale.
    #[test]
    fn arena_coverage_is_a_subset() {
        let known: BTreeSet<&str> = KNOWN_ARENAS.iter().copied().collect();
        let unknown: Vec<String> = covered_arenas()
            .into_iter()
            .filter(|a| a != "unknowns" && !known.contains(a.as_str()))
            .collect();
        assert!(
            unknown.is_empty(),
            "fixtures populated arenas absent from KNOWN_ARENAS (update the denominator): {unknown:?}"
        );
    }

    /// Freezes the collective arena coverage floor so a refactor cannot silently
    /// stop populating an arena across the whole fixture set. Prints the fraction
    /// under `--nocapture`.
    #[test]
    fn arena_coverage_meets_floor() {
        let covered = covered_arenas();
        let known: BTreeSet<&str> = KNOWN_ARENAS.iter().copied().collect();
        let hit = covered
            .iter()
            .filter(|a| known.contains(a.as_str()))
            .count();
        let uncovered: Vec<&str> = KNOWN_ARENAS
            .iter()
            .copied()
            .filter(|a| !covered.contains(*a))
            .collect();
        println!(
            "golden arena coverage: {hit}/{} known arenas ({:.1}%)\nuncovered: {uncovered:?}",
            KNOWN_ARENAS.len(),
            100.0 * hit as f64 / KNOWN_ARENAS.len() as f64,
        );
        assert!(
            hit >= ARENA_COVERAGE_FLOOR,
            "arena coverage regressed: {hit} < floor {ARENA_COVERAGE_FLOOR}"
        );
    }

    /// The catalogue is the single source of truth for arena names: every arena
    /// appears exactly once across `CATALOGUE`, there is one row per model field
    /// (179), and the catalogue's arena set is exactly `KNOWN_ARENAS`. The exact
    /// equality is the relationship the fixtures confirm — every arena a fixture
    /// can populate is a catalogue arena, and every catalogue arena is a name
    /// `KNOWN_ARENAS` tracks. A single production site (`native::attach`) emits
    /// arenas, all of them catalogue-driven, so no non-catalogued arena exists.
    #[test]
    fn catalogue_arenas_match_known_arenas() {
        use crate::native::catalogue::CATALOGUE;

        assert_eq!(CATALOGUE.len(), 179, "one catalogue row per model field");

        let mut catalogue_arenas = BTreeSet::new();
        for row in CATALOGUE {
            assert!(
                catalogue_arenas.insert(row.arena),
                "arena {:?} appears in more than one catalogue row",
                row.arena
            );
        }
        assert_eq!(
            catalogue_arenas.len(),
            CATALOGUE.len(),
            "every catalogue row owns a distinct arena"
        );

        let known: BTreeSet<&str> = KNOWN_ARENAS.iter().copied().collect();
        let catalogue_not_known: Vec<&str> = catalogue_arenas.difference(&known).copied().collect();
        let known_not_catalogue: Vec<&str> = known.difference(&catalogue_arenas).copied().collect();
        assert!(
            catalogue_not_known.is_empty(),
            "catalogue arenas absent from KNOWN_ARENAS: {catalogue_not_known:?}"
        );
        assert!(
            known_not_catalogue.is_empty(),
            "KNOWN_ARENAS entries absent from CATALOGUE: {known_not_catalogue:?}"
        );
    }
}
