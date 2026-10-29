// SPDX-License-Identifier: Apache-2.0

use crate::test_support::test_fixtures::{
    acis_sphere_kernel_stream, content_at, primary_envelope_fixture_with_design, push_utf16,
};
use crate::InventorCodec;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{
    BodySelection, FaceMotion, FaceSelection, FeatureDefinition, FeatureEvaluation,
    FeatureOperation,
};
use cadmpeg_ir::math::Vector3;

fn guid(value: &str) -> [u8; 16] {
    std::array::from_fn(|i| {
        u8::from_str_radix(&value[i * 2..i * 2 + 2], 16).expect("valid synthetic motion")
    })
}

fn list(values: &[u32]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(2u16.to_le_bytes());
    b.extend(0x3000u16.to_le_bytes());
    b.extend(
        u32::try_from(values.len())
            .expect("valid synthetic motion")
            .to_le_bytes(),
    );
    if !values.is_empty() {
        b.extend(
            u32::try_from(values.len())
                .expect("valid synthetic motion")
                .to_le_bytes(),
        );
        b.extend(0u32.to_le_bytes());
    }
    for v in values {
        b.extend((if *v == 0 { 0 } else { v | 0x8000_0000 }).to_le_bytes());
    }
    b
}

fn face_motion_source() -> Vec<u8> {
    let mut feature = content_at(0, 18, &[]);
    feature.extend(69u32.to_le_bytes());
    feature.extend(0u32.to_le_bytes());
    feature.extend(list(&[3, 4, 0, 0, 0, 0, 0, 0, 5, 7, 0, 8, 9]));
    feature.extend(0u32.to_le_bytes());
    let mut label = vec![0; 14];
    label.extend(0x8000_0001u32.to_le_bytes());
    label.extend([0; 8]);
    label.extend(1u32.to_le_bytes());
    label.extend(list(&[]));
    push_utf16(&mut label, "Synthetic face motion");
    label.extend(guid("dea6a68f4c4b529188ca31b5630198c8"));
    let mut faces = content_at(2, 18, &[]);
    faces.extend(list(&[10]));
    let mut mode = content_at(3, 18, &[]);
    mode.extend(2i16.to_le_bytes());
    mode.extend(2u16.to_le_bytes());
    let mut selection = content_at(4, 18, &[]);
    selection.extend(list(&[6]));
    let mut body = content_at(5, 18, &[]);
    body.extend(0x8000_000bu32.to_le_bytes());
    let mut dimensions = content_at(6, 18, &[]);
    dimensions.extend(list(&[]));
    let mut flag = content_at(7, 18, &[]);
    push_utf16(&mut flag, "Synthetic flag");
    flag.extend(0u32.to_le_bytes());
    flag.push(1);
    let mut transform = content_at(8, 18, &[]);
    transform.extend([0; 4]);
    for row in [
        [1.0f64, 0.0, 0.0, 2.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ] {
        for value in row {
            transform.extend(value.to_le_bytes());
        }
    }
    let mut item = content_at(9, 18, &[]);
    let mut indices = list(&[71]);
    let end = indices.len();
    indices[end - 4..].copy_from_slice(&71u32.to_le_bytes());
    item.extend(indices);
    item.extend((-1i32).to_le_bytes());
    primary_envelope_fixture_with_design(
        &acis_sphere_kernel_stream(21_800),
        &[
            (crate::pmdc::inventor_id(0x9087_4d91), feature),
            (guid("2ba4482bd2115864600074b79b49ebb0"), label),
            (guid("f1b2bc24d01121dd0008d0bc0663dc09"), faces),
            (guid("fb7982ef3347f24f718133968efffa3f"), mode),
            (guid("83aadad54f4375dacd1003b260a8a422"), selection),
            (crate::pmdc::inventor_id(0x9087_4d47), body),
            (guid("71f23ed8d2115094a00049803603c8c9"), dimensions),
            (crate::pmdc::inventor_id(0x9087_4d28), flag),
            (crate::pmdc::inventor_id(0x9087_4d18), transform),
            (guid("ccb59f50d1114f6b0008a1ba32a3dc09"), item),
            ([0x42; 16], content_at(10, 18, &[])),
        ],
    )
}

#[test]
fn face_motion_records_keep_native_topology_indices_and_validate_their_selection() {
    let source = face_motion_source();
    let decoded = InventorCodec
        .decode(
            &mut std::io::Cursor::new(&source),
            &DecodeOptions::default(),
        )
        .expect("valid synthetic motion");
    let (ir, _, _) = decoded.into_parts();
    assert_eq!(ir.model.features.len(), 1);
    assert_eq!(ir.model.feature_result_topologies.len(), 1);
    let FeatureDefinition::Operation(FeatureOperation::MoveFace {
        faces: FaceSelection::Native(faces),
        motion: FaceMotion::Translate {
            direction,
            distance,
        },
    }) = ir.model.features[0].evaluation.definition()
    else {
        panic!("face translation")
    };
    assert_eq!(direction.get(), Vector3::new(1.0, 0.0, 0.0));
    assert_eq!(distance.literal().expect("literal distance").get(), 20.0);
    assert_ne!(
        Some(faces.as_str()),
        ir.model.feature_result_topologies[0].native_ref.as_deref()
    );
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service())
        .expect("valid synthetic motion");
    let findings = InventorCodec
        .validate_native(&ctx, &ir, None)
        .expect("valid synthetic motion");
    assert!(
        findings
            .iter()
            .all(|f| f.severity < cadmpeg_ir::report::Severity::Error),
        "{findings:?}"
    );
}

fn body_motion_source(class: &str, body_reference: u32) -> Vec<u8> {
    let mut feature = content_at(0, 18, &[]);
    feature.extend(69u32.to_le_bytes());
    feature.extend(0u32.to_le_bytes());
    feature.extend(list(&[3, 4, 7]));
    feature.extend(0u32.to_le_bytes());
    let mut label = vec![0; 14];
    label.extend(0x8000_0001u32.to_le_bytes());
    label.extend([0; 8]);
    label.extend(1u32.to_le_bytes());
    label.extend(list(&[]));
    push_utf16(&mut label, "Synthetic motion");
    label.extend(guid(class));
    let mut selection = content_at(2, 18, &[]);
    selection.extend(list(&[5]));
    let mut transforms = content_at(3, 18, &[]);
    transforms.extend(list(&[6]));
    let mut body = content_at(4, 18, &[]);
    body.extend(body_reference.to_le_bytes());
    let mut transform = content_at(5, 18, &[]);
    transform.extend([0; 4]);
    for row in [
        [0.0f64, -1.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, -2.0],
        [0.0, 0.0, 1.0, 3.0],
        [0.0, 0.0, 0.0, 1.0],
    ] {
        for value in row {
            transform.extend(value.to_le_bytes());
        }
    }
    let mut dimensions = content_at(6, 18, &[]);
    dimensions.extend(list(&[]));
    primary_envelope_fixture_with_design(
        &acis_sphere_kernel_stream(21_800),
        &[
            (crate::pmdc::inventor_id(0x9087_4d91), feature),
            (guid("2ba4482bd2115864600074b79b49ebb0"), label),
            (guid("ae70680ed14a1e86d76248b03c2a96e1"), selection),
            (guid("29fe0008d511dd3510003697ab9f0ab5"), transforms),
            (crate::pmdc::inventor_id(0x9087_4d47), body),
            (crate::pmdc::inventor_id(0x9087_4d18), transform),
            (guid("71f23ed8d2115094a00049803603c8c9"), dimensions),
            ([0x42; 16], content_at(7, 18, &[])),
        ],
    )
}

#[test]
fn body_motion_records_project_and_validate_their_rigid_transform_and_result() {
    for class in [
        "dea6a68f4c4b529188ca31b5630198c8",
        "e99f832b7a4295a1d9763383bea17cce",
        "e6da3f26924b5dab8e1bc5a93fead7de",
    ] {
        let source = body_motion_source(class, 0x8000_0008);
        let decoded = InventorCodec
            .decode(
                &mut std::io::Cursor::new(&source),
                &DecodeOptions::default(),
            )
            .expect("valid synthetic motion");
        let (mut ir, _, _) = decoded.into_parts();
        assert_eq!(ir.model.features.len(), 1);
        assert_eq!(ir.model.feature_result_topologies.len(), 1);
        let FeatureDefinition::Operation(FeatureOperation::MoveBody {
            bodies: BodySelection::Local { bodies, .. },
            translation,
            rotation: Some(rotation),
            copies: cadmpeg_ir::features::Dimension::Literal(0),
        }) = ir.model.features[0].evaluation.definition()
        else {
            panic!("body motion")
        };
        assert_eq!(bodies.len(), 1);
        assert_eq!(
            translation
                .each_ref()
                .map(|component| component.literal().expect("literal translation").get()),
            [10.0, -20.0, 30.0]
        );
        assert!(
            (rotation.angle.literal().expect("literal angle").get() - std::f64::consts::FRAC_PI_2)
                .abs()
                < 1e-12
        );
        assert_eq!(
            ir.model.feature_result_topologies[0]
                .bodies()
                .iter()
                .map(cadmpeg_core::text::NonBlankString::as_str)
                .collect::<Vec<_>>(),
            bodies.iter().map(String::as_str).collect::<Vec<_>>()
        );
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service())
            .expect("valid synthetic motion");
        let findings = InventorCodec
            .validate_native(&ctx, &ir, None)
            .expect("valid synthetic motion");
        assert!(
            findings
                .iter()
                .all(|f| f.severity < cadmpeg_ir::report::Severity::Error),
            "{findings:?}"
        );
        let FeatureDefinition::Operation(mut operation) =
            ir.model.features[0].evaluation.definition().clone()
        else {
            panic!("operation")
        };
        if let FeatureOperation::MoveBody { copies, .. } = &mut operation {
            *copies = 1.into();
        }
        ir.model.features[0].evaluation =
            FeatureEvaluation::from_definition(FeatureDefinition::Operation(operation));
        assert!(InventorCodec
            .validate_native(&ctx, &ir, None)
            .expect("valid synthetic motion")
            .iter()
            .any(|f| f.message.contains("neutral motion does not match")));
    }
}

#[test]
fn surface_body_references_resolve_with_either_qualifier() {
    for (reference, resolves) in [(8, true), (0x8000_0008, true), (0x8000_0065, false)] {
        let source = body_motion_source("dea6a68f4c4b529188ca31b5630198c8", reference);
        let decoded = InventorCodec
            .decode(
                &mut std::io::Cursor::new(&source),
                &DecodeOptions::default(),
            )
            .expect("synthetic body motion");
        let (ir, _, _) = decoded.into_parts();
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service())
            .expect("synthetic body motion");
        let findings = InventorCodec
            .validate_native(&ctx, &ir, None)
            .expect("synthetic body motion");
        assert_eq!(
            findings
                .iter()
                .all(|finding| finding.severity < cadmpeg_ir::report::Severity::Error),
            resolves,
            "{findings:?}"
        );
    }
}
