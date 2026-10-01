// SPDX-License-Identifier: Apache-2.0
//! `SWObjects` document-metadata decode and write-back tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::container::make_block;
use crate::test_support::container::outer_header;
use crate::test_support::container::sldprt_with_body;
use crate::test_support::container::sldprt_with_body_and_envelope;
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;

#[test]
fn metadata_from_nameless_block_keeps_annotation_owner() {
    let payload = br"<swSolidWorks><SW_UnitsLinear>1</SW_UnitsLinear></swSolidWorks>";
    let mut source = outer_header();
    source.extend(make_block(0x43, "", payload));
    let scan = crate::test_support::container::scan(&source);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service()).unwrap();
    let mut annotations = cadmpeg_ir::annotations::Annotations::default();
    let attributes = super::attributes(&ctx, &scan, &mut annotations).unwrap();

    assert!(attributes
        .iter()
        .any(|attribute| attribute.name == "source_linear_unit_code"));
    assert!(annotations
        .provenance
        .values()
        .any(|provenance| provenance.stream() == "block@8"));
}

#[test]
fn metadata_annotation_route_refuses_retained_text() {
    let payload = br"<swSolidWorks><SW_UnitsLinear>1</SW_UnitsLinear></swSolidWorks>";
    let mut source = outer_header();
    source.extend(make_block(0x43, "", payload));
    let scan = crate::test_support::container::scan(&source);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &policy).unwrap();
    let mut annotations = cadmpeg_ir::annotations::Annotations::default();
    let error = super::attributes(&ctx, &scan, &mut annotations)
        .expect_err("metadata annotation requires retained text");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes));
}

#[test]
fn metadata_annotation_route_refuses_collection_growth() {
    let payload = br"<swSolidWorks><SW_UnitsLinear>1</SW_UnitsLinear></swSolidWorks>";
    let mut source = outer_header();
    source.extend(make_block(0x43, "", payload));
    let scan = crate::test_support::container::scan(&source);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &policy).unwrap();
    let mut annotations = cadmpeg_ir::annotations::Annotations::default();
    let error = super::attributes(&ctx, &scan, &mut annotations)
        .expect_err("metadata attributes require collection admission");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems));
}

#[test]
fn metadata_annotation_route_refuses_work_at_minimum_admission() {
    let payload = br"<swSolidWorks><SW_UnitsLinear>1</SW_UnitsLinear></swSolidWorks>";
    let mut source = outer_header();
    source.extend(make_block(0x43, "", payload));
    let scan = crate::test_support::container::scan(&source);
    let arena = DecodeArena::new();
    let run = |policy: &DecodePolicy| {
        let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, policy).unwrap();
        let mut annotations = cadmpeg_ir::annotations::Annotations::default();
        super::attributes(&ctx, &scan, &mut annotations).map(|attributes| (attributes, annotations))
    };
    let expected = run(&DecodePolicy::service()).unwrap();
    assert!(!expected.0.is_empty());
    assert!(!expected.1.provenance.is_empty());
    let admitted = |policy: &DecodePolicy| match run(policy) {
        Ok(actual) => {
            assert_eq!(actual, expected);
            true
        }
        Err(CodecError::ResourceLimit(limit)) => {
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            false
        }
        Err(error) => panic!("unexpected metadata annotation error: {error}"),
    };
    let mut policy = DecodePolicy::service();
    let mut lower = 0;
    let mut upper = 1_u64;
    loop {
        policy.limits.max_work_units = upper;
        if admitted(&policy) {
            break;
        }
        upper = upper.checked_mul(2).unwrap();
    }
    while lower < upper {
        let middle = lower + (upper - lower) / 2;
        policy.limits.max_work_units = middle;
        if admitted(&policy) {
            upper = middle;
        } else {
            lower = middle + 1;
        }
    }
    assert!(upper > 0);
    policy.limits.max_work_units = upper;
    assert!(admitted(&policy));
    policy.limits.max_work_units = upper - 1;
    assert!(!admitted(&policy));
}

#[test]
fn transformed_reference_plane_requires_fixed_prefix() {
    let mut source = sldprt_with_body(&triangle_body());
    let mut payload = b"moTransRefPlaneData_c".to_vec();
    payload.extend_from_slice(&[0; 8]);
    for value in [0.01f64, 0.02, 0.03, 0.1, 0.2, 1.0, 0.0, -1.0, 0.5] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    source.extend(make_block(0x43, "SWObjects", &payload));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert!(!decoded
        .ir()
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "transformed_reference_plane"));
}

#[test]
fn semantic_writer_preserves_transformed_reference_plane_prefix() {
    use cadmpeg_ir::attributes::AttributeValue;

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_envelope(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir = decoded.ir_mut();
        let transformed = ir
            .model
            .attributes
            .iter_mut()
            .find(|attribute| attribute.name == "transformed_reference_plane")
            .unwrap();
        let AttributeValue::Vector(center) = &mut transformed.values[0] else {
            panic!("transformed plane center");
        };
        center[0] = cadmpeg_ir::scalar::FiniteReal::new(25.0).unwrap();
    }

    let mut written = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut written,
    )
    .unwrap();

    let scan = crate::test_support::container::scan(&written);
    let payload = scan
        .blocks
        .iter()
        .find(|block| {
            block
                .payload
                .windows(b"moTransRefPlaneData_c".len())
                .any(|bytes| bytes == b"moTransRefPlaneData_c")
        })
        .map(|block| block.payload.as_slice())
        .unwrap();
    let token = b"moTransRefPlaneData_c";
    let offset = payload
        .windows(token.len())
        .position(|bytes| bytes == token)
        .unwrap()
        + token.len();
    assert_eq!(&payload[offset..offset + 8], &[0xff; 8]);
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    let transformed = regenerated
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "transformed_reference_plane")
        .unwrap();
    assert!(transformed.id.as_str().ends_with(":147"));
}

#[test]
fn decode_does_not_scan_past_unit_name_record_start() {
    let mut source = sldprt_with_body(&triangle_body());
    let mut payload = b"moLengthUserUnits_c".to_vec();
    payload.extend_from_slice(&[0; 8]);
    payload.extend_from_slice(&[0xff, 0xfe, 0xff, 4, b'I', 0, b'N', 0]);
    source.extend(make_block(0x43, "SWObjects", &payload));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(!decoded
        .ir()
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "source_linear_unit_name"));
}

#[test]
fn semantic_writer_preserves_document_metadata() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_envelope(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let moved = decoded.ir_mut().model.points[0].position().get();
    decoded.ir_mut().model.points[0].set_position(
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            moved.x,
            moved.y,
            moved.z + 1.0,
        ))
        .expect("a finite position is a point"),
    );

    let expected = decoded
        .ir()
        .model
        .attributes
        .iter()
        .map(|attribute| (attribute.name.clone(), attribute.values.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let actual = regenerated
        .ir()
        .model
        .attributes
        .iter()
        .map(|attribute| (attribute.name.clone(), attribute.values.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();

    assert_eq!(actual, expected);
}

#[test]
fn decode_extracts_document_envelope() {
    use cadmpeg_ir::attributes::AttributeValue;
    let mut cur = Cursor::new(sldprt_with_body_and_envelope(&triangle_body()));
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    let envelope = result
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "bounding_envelope")
        .expect("envelope");
    let AttributeValue::Vector(values) = &envelope.values[0] else {
        panic!("vector")
    };
    assert_eq!(
        cadmpeg_ir::scalar::FiniteReal::raw_lane(values),
        [10.0, 20.0, -30.0, 40.0]
    );
    let plane = result
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "default_reference_plane")
        .expect("reference plane");
    let AttributeValue::Vector(origin) = &plane.values[0] else {
        panic!("origin")
    };
    let AttributeValue::Vector(frame) = &plane.values[1] else {
        panic!("frame")
    };
    assert_eq!(
        cadmpeg_ir::scalar::FiniteReal::raw_lane(origin),
        [1.0, 2.0, 3.0]
    );
    assert_eq!(frame[2].get(), 1.0);
    let transformed = result
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "transformed_reference_plane")
        .expect("transformed reference plane");
    assert!(transformed.id.as_str().ends_with(":147"));
    assert_eq!(
        transformed.values,
        vec![
            AttributeValue::vector([10.0, 20.0, 30.0]).unwrap(),
            AttributeValue::vector([100.0, 200.0]).unwrap(),
            AttributeValue::vector([1.0, 0.0, -1.0]).unwrap(),
            AttributeValue::float(500.0).unwrap(),
        ]
    );
    let part = result
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "part_record")
        .unwrap();
    assert_eq!(
        part.values,
        vec![AttributeValue::Integer(42), AttributeValue::Integer(2026)]
    );
    let configuration = result
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "configuration_manager")
        .unwrap();
    assert_eq!(configuration.values[1], AttributeValue::Integer(3));
    let units = result
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "source_linear_unit_code")
        .unwrap();
    assert_eq!(units.values, vec![AttributeValue::Integer(0)]);
    let unit_name = result
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "source_linear_unit_name")
        .unwrap();
    assert_eq!(unit_name.values, vec![AttributeValue::String("IN".into())]);
}

/// The metadata attributes a single `SWObjects` block holding `payload` yields.
fn scanned_metadata(payload: &[u8]) -> Vec<cadmpeg_ir::attributes::SourceAttribute> {
    let mut source = outer_header();
    source.extend(make_block(0x43, "SWObjects", payload));
    let scan = crate::test_support::container::scan(&source);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service()).unwrap();
    super::attributes(
        &ctx,
        &scan,
        &mut cadmpeg_ir::annotations::Annotations::default(),
    )
    .unwrap()
}

#[test]
fn a_bounding_envelope_that_overflows_in_millimetres_is_not_admitted() {
    let mut payload = b"moBBoxCenterData_c".to_vec();
    payload.extend_from_slice(&1u32.to_le_bytes());
    for value in [1.0e306f64, 0.02, -0.03, 0.04] {
        payload.extend_from_slice(&value.to_le_bytes());
    }

    let attributes = scanned_metadata(&payload);
    assert!(
        !attributes
            .iter()
            .any(|attribute| attribute.name == "bounding_envelope"),
        "{attributes:?}"
    );
}

#[test]
fn a_reference_plane_origin_that_overflows_in_millimetres_is_not_admitted() {
    let mut payload = b"moDefaultRefPlnData_c".to_vec();
    for value in [0.001f64, -1.0e306, 0.003, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }

    let attributes = scanned_metadata(&payload);
    assert!(
        !attributes
            .iter()
            .any(|attribute| attribute.name == "default_reference_plane"),
        "{attributes:?}"
    );
}

fn replacement_unit_source() -> (Vec<u8>, usize) {
    let mut payload = b"moLengthUserUnits_c".to_vec();
    payload.extend_from_slice(&[0xff, 0xfe, 0xff, 2, 0, 0xd8]);
    let length = payload.len();
    let mut source = outer_header();
    source.extend(make_block(0x43, "SWObjects", &payload));
    (source, length)
}

#[test]
fn unit_name_replacement_refuses_exact_retained_limit() {
    let (source, _) = replacement_unit_source();
    let scan = crate::test_support::container::scan(&source);
    let section = scan.sections().next().expect("unit-name section");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert!(matches!(super::scan_length_user_units(&ctx, section, &mut Vec::new(), &mut cadmpeg_ir::annotations::Annotations::default()), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes && limit.used == 0 && limit.additional == 3 && limit.operation == "retain SLDPRT linear unit name"));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut attributes = Vec::new();
    super::scan_length_user_units(&ctx, section, &mut attributes, &mut cadmpeg_ir::annotations::Annotations::default()).expect("replacement unit name");
    assert_eq!(attributes[0].values, vec![cadmpeg_ir::attributes::AttributeValue::String("�".into())]);
}

#[test]
fn unit_name_replacement_refuses_work_before_validation() {
    let (source, length) = replacement_unit_source();
    let scan = crate::test_support::container::scan(&source);
    let section = scan.sections().next().expect("unit-name section");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::try_from(length).expect("payload length");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert!(matches!(super::scan_length_user_units(&ctx, section, &mut Vec::new(), &mut cadmpeg_ir::annotations::Annotations::default()), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "validate SLDPRT linear unit name"));
}
