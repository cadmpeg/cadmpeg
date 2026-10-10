// SPDX-License-Identifier: Apache-2.0

use super::{parse_points, resource_test_property, DecodeArena, DecodeContext, DecodePolicy};
use crate::native::RetainedXml;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point3;

#[test]
fn failed_point_transform_is_malformed_before_row_publication() {
    let mut property = resource_test_property();
    property.xml = RetainedXml::from_text(
        "<Property><Points mtrx=\"1e300 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1\"/></Property>".into(), 0,
    ).expect("XML");
    for count in [1_u32, 2] {
        let mut bytes = count.to_le_bytes().to_vec();
        for _ in 0..count { for value in [1e30_f32, 0.0, 0.0] { bytes.extend_from_slice(&value.to_le_bytes()); } }
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let mut points = Vec::new();
        let mut admitted = 0;
        let error = parse_points(&ctx, &property, &bytes, 0, &mut admitted, None, &mut points).expect_err("transformed coordinate overflow");
        assert!(matches!(error, CodecError::Malformed(message)
            if message == "transformed point-cloud point contains a non-finite coordinate"));
        assert!(points.is_empty());
        assert_eq!(ctx.resource_refusal(), None);
        assert_eq!(admitted, u64::from(count));
    }
}

#[test]
fn invalid_point_source_is_malformed_before_row_publication() {
    let mut property = resource_test_property();
    property.owner = " ".into();
    let bytes = [1_u32.to_le_bytes().as_slice(), &[0; 12]].concat();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut points = Vec::new();
    let error = parse_points(&ctx, &property, &bytes, 0, &mut 0, None, &mut points).expect_err("blank source object");
    assert!(matches!(error, CodecError::Malformed(message) if message == "source object_id must not be empty"));
    assert!(points.is_empty());
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn accepted_point_rows_keep_identities_positions_and_source() {
    let property = resource_test_property();
    let mut bytes = 2_u32.to_le_bytes().to_vec();
    for value in [1.0_f32, 2.0, 3.0, -1.0, -2.0, -3.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("context");
    let mut points = Vec::new();
    parse_points(&ctx, &property, &bytes, 0, &mut 0, None, &mut points).expect("point rows");
    assert_eq!(points.len(), 2);
    for (point, id, position) in [
        (
            &points[0],
            "fcstd:model:point#Geometry:0",
            Point3::new(1.0, 2.0, 3.0),
        ),
        (
            &points[1],
            "fcstd:model:point#Geometry:1",
            Point3::new(-1.0, -2.0, -3.0),
        ),
    ] {
        assert_eq!(point.id.as_str(), id);
        assert_eq!(point.position().get(), position);
        let source = point.source_object.as_ref().expect("source object");
        assert_eq!(source.object_id.as_str(), property.owner);
        assert_eq!(source.name.as_deref(), Some("Geometry"));
    }

}

#[test]
fn point_source_refusal_keeps_original_limit() {
    let property = resource_test_property();
    let bytes = [1_u32.to_le_bytes().as_slice(), &[0; 12]].concat();
    let error = crate::test_support::refusal_at(
        ResourceDimension::RetainedBytes, &bytes, "FreeCAD geometry object identity",
        |ctx| {
            let mut points = Vec::new();
            let result = parse_points(ctx, &property, &bytes, 0, &mut 0, None, &mut points);
            if let Err(CodecError::ResourceLimit(original)) = &result {
                assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
                assert!(points.is_empty());
                assert_eq!(ctx.resource_refusal(), Some(*original));
                assert!(matches!(ctx.charge_retained(0, "later point publication"),
                    Err(CodecError::ResourceLimit(repeated)) if repeated == *original));
            }
            result
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(_)));
}

#[test]
fn point_trailing_payload_keeps_completed_row_and_retained_diagnostic() {
    let property = resource_test_property();
    let mut bytes = [1_u32.to_le_bytes().as_slice(), &[0; 12]].concat();
    bytes.push(0);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("context");
    let mut points = Vec::new();
    let error = parse_points(&ctx, &property, &bytes, 0, &mut 0, None, &mut points)
        .expect_err("trailing byte");
    let CodecError::Malformed(message) = error else {
        panic!("payload diagnostic")
    };
    assert_eq!(message, "point-cloud payload has 1 trailing bytes");
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].id.as_str(), "fcstd:model:point#Geometry:0");
    assert_eq!(points[0].position().get(), Point3::new(0.0, 0.0, 0.0));
    assert_eq!(ctx.resource_refusal(), None);

}

#[test]
fn empty_application_payload_finish_is_free_and_real_refusal_stays_sticky() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    super::Reader::new(&[])
        .finish(&ctx, "point-cloud payload")
        .expect("empty finish");
    assert_eq!(ctx.resource_refusal(), None);
    let Err(CodecError::ResourceLimit(original)) =
        super::Reader::new(b"x").finish(&ctx, "point-cloud payload")
    else {
        panic!("real diagnostic refuses")
    };
    // The core formatter admits its length pass before reserving text.
    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
    assert_eq!(original.operation, "FreeCAD application payload diagnostic");
    assert_eq!(original.used, 0);
    assert!(
        matches!(super::Reader::new(b"xx").finish(&ctx, "mesh payload"),
        Err(CodecError::ResourceLimit(repeated)) if repeated == original)
    );
}

#[test]
fn point_population_retained_limit_refuses_large_population() {
    for count in [1_u32, 128] {
        let mut bytes = count.to_le_bytes().to_vec();
        bytes.extend(vec![0; usize::try_from(count).unwrap() * 12]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 1024;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let result = parse_points(&ctx, &resource_test_property(), &bytes, 0, &mut 0, None, &mut Vec::new());
        if count == 1 { assert!(result.is_ok()); }
        else { assert!(matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes)); }
    }
}
