// SPDX-License-Identifier: Apache-2.0
//! Decimal carrier text and temporary identity ordinals.

use crate::brep::{parse_binary_edge_representation, BinaryCursor, BinaryGeometryCounts,
    TextEdgeRepresentation, transfer_text_geometry};
use crate::test_support::{refusal_at, with_service_context};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn continuity_bytes(kind: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    if kind == 3 {
        bytes.extend_from_slice(&1_i32.to_le_bytes());
        bytes.extend_from_slice(&2_i32.to_le_bytes());
    }
    bytes.push(255);
    for value in [1_i32, 0] { bytes.extend_from_slice(&value.to_le_bytes()); }
    if kind == 3 {
        for value in [0_f64, 1.0] { bytes.extend_from_slice(&value.to_le_bytes()); }
    } else {
        for value in [1_i32, 0] { bytes.extend_from_slice(&value.to_le_bytes()); }
    }
    bytes
}

fn check_continuity(kind: u8) {
    let bytes = continuity_bytes(kind);
    let counts = BinaryGeometryCounts { curves: 0, curves2d: 2, surfaces: 1,
        locations: 0, polygons3d: 0, indexed_polygons: 0, triangulations: 0 };
    for cap in [2, 3] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let mut cursor = BinaryCursor::new(&ctx, &bytes);
        let result = parse_binary_edge_representation(&mut cursor, 1, kind, counts);
        if cap == 3 {
            let (TextEdgeRepresentation::PcurvePair { continuity, .. }
                | TextEdgeRepresentation::Regularity { continuity, .. }) = result.unwrap()
                else { panic!("continuity representation") };
            assert_eq!(continuity, "255");
            assert_eq!(cursor.remaining(), 0);
            assert_eq!(ctx.resource_refusal(), None);
        } else {
            let Err(CodecError::ResourceLimit(original)) = result
                else { panic!("three decimal bytes exceed two-byte cap") };
            assert_eq!(original.operation, "FreeCAD binary edge continuity");
            assert_eq!((original.used, original.additional), (0, 3));
            let mut cursor = BinaryCursor::new(&ctx, &bytes);
            assert!(matches!(parse_binary_edge_representation(&mut cursor, 1, kind, counts),
                Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
    }
}

#[test]
fn binary_pcurve_pair_decimal_continuity_is_retained_once() { check_continuity(3); }

#[test]
fn binary_regularity_decimal_continuity_is_retained_once() { check_continuity(4); }

fn ordinal_boundary(operation: &str) {
    let payload = super::allocation_tests::shape_payload();
    let mut consumer = payload.clone();
    if operation == "FreeCAD transferred surface ordinal" {
        let crate::brep::ShapePayload::Text { facts, .. } = &mut consumer.payload
            else { panic!("text carrier") };
        facts.curves.clear();
    }
    let property = super::allocation_tests::shape_property("<Property/>");
    refusal_at(ResourceDimension::MaterializedBytes, &[], operation, |ctx| {
        transfer_text_geometry(ctx, std::slice::from_ref(&consumer), std::slice::from_ref(&property))
    });
    with_service_context(&[], |ctx| {
        let (curves, surfaces) = transfer_text_geometry(ctx,
            std::slice::from_ref(&payload), std::slice::from_ref(&property)).unwrap();
        assert_eq!(curves.curves[0].id.as_str(),
            "fcstd:model:curve#Payload:1");
        assert_eq!(surfaces.surfaces[0].id.as_str(),
            "fcstd:model:surface#Payload:1");
    });
}

#[test]
fn transferred_curve_decimal_ordinal_is_scoped() {
    ordinal_boundary("FreeCAD transferred curve ordinal");
}

#[test]
fn transferred_surface_decimal_ordinal_is_scoped() {
    ordinal_boundary("FreeCAD transferred surface ordinal");
}
