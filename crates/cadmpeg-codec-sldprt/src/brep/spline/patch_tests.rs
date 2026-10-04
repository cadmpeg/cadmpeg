// SPDX-License-Identifier: Apache-2.0
//! Resource propagation through retained spline patches.

use crate::test_support::parasolid::{compact_counted_nurbs_surface_carrier, nurbs_curve_carrier};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};

#[test]
fn retained_nurbs_curve_patch_preserves_descriptor_refusal() {
    let bytes = nurbs_curve_carrier(10, 11);
    let baseline = cadmpeg_test_support::service_decode_context();
    let carrier = super::scan_curve_carriers(&baseline, &bytes, &mut Vec::new())
        .unwrap()
        .remove(&10)
        .unwrap();
    let Some(SolvedCurveGeometry::Nurbs(old)) = carrier.geometry.solved() else {
        panic!("NURBS curve");
    };
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "collect Parasolid curve descriptors",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
            let mut patched = bytes.clone();
            let result = super::patch_nurbs_curve(&ctx, &mut patched, 0, old, old, 0.001);
            if let Err(CodecError::ResourceLimit(limit)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
            }
            result
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(_)));
}

fn retained_surface_refusal(operation: &'static str) {
    let bytes = compact_counted_nurbs_surface_carrier(10, 11, 20);
    let baseline = cadmpeg_test_support::service_decode_context();
    let carrier = super::scan_surface_carriers(&baseline, &bytes, &mut Vec::new())
        .unwrap()
        .remove(&10)
        .unwrap();
    let Some(SolvedSurfaceGeometry::Nurbs(old)) = carrier.geometry.solved() else {
        panic!("NURBS surface");
    };
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        operation,
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
            let mut patched = bytes.clone();
            let result = super::patch_nurbs_surface(&ctx, &mut patched, 0, old, old, 0.001);
            if let Err(CodecError::ResourceLimit(limit)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
            }
            result
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(_)));
}

#[test]
fn retained_nurbs_surface_patch_preserves_descriptor_refusal() {
    retained_surface_refusal("collect Parasolid surface descriptors");
}

#[test]
fn retained_nurbs_surface_patch_preserves_compact_attribute_refusal() {
    retained_surface_refusal("index Parasolid patch compact attributes");
}

#[test]
fn retained_nurbs_surface_patch_preserves_array_index_refusal() {
    retained_surface_refusal("index Parasolid compact arrays");
}

#[test]
fn retained_nurbs_surface_patch_preserves_array_span_refusal() {
    retained_surface_refusal("collect Parasolid patch array spans");
}

#[test]
fn retained_nurbs_surface_patch_preserves_scalar_value_refusal() {
    retained_surface_refusal("read Parasolid patch scalar values");
}

#[test]
fn retained_nurbs_surface_patch_preserves_multiplicity_refusal() {
    retained_surface_refusal("read Parasolid patch multiplicities");
}

#[test]
fn retained_nurbs_surface_patch_preserves_knot_pair_refusal() {
    retained_surface_refusal("collect Parasolid patch knot span pairs");
}

#[test]
fn attribute_nurbs_patch_preserves_nested_descriptor_refusal() {
    let bytes = nurbs_curve_carrier(10, 11);
    let baseline = cadmpeg_test_support::service_decode_context();
    let carrier = super::scan_curve_carriers(&baseline, &bytes, &mut Vec::new())
        .unwrap()
        .remove(&10)
        .unwrap();
    let Some(SolvedCurveGeometry::Nurbs(old)) = carrier.geometry.solved() else {
        panic!("NURBS curve");
    };
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "collect Parasolid patch knot values",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
            let mut patched = bytes.clone();
            let result = crate::brep::patch_nurbs_by_attr(&ctx, &mut patched, 10, old);
            if let Err(CodecError::ResourceLimit(limit)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
            }
            result
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(_)));
}

fn assert_empty_scan_work_refusal(operation: &str) {
    let body = [0xff; 4096];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&body, &arena, &policy).unwrap();
    let (result, allocations) =
        crate::test_support::allocation::count_allocations(|| match operation {
            "scan Parasolid spline arrays" => super::scan_arrays(&ctx, &body, None).map(|_| ()),
            "scan Parasolid curve descriptors" => {
                super::scan_curve_descriptors(&ctx, &body).map(|_| ())
            }
            _ => super::scan_surface_descriptors(&ctx, &body).map(|_| ()),
        });
    let Err(CodecError::ResourceLimit(limit)) = result else {
        panic!("scan work refusal");
    };
    assert_eq!(limit.operation, operation);
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert_eq!(allocations, 0);
}

#[test]
fn spline_array_scan_admits_work_before_empty_pass() {
    assert_empty_scan_work_refusal("scan Parasolid spline arrays");
}

#[test]
fn curve_descriptor_scan_admits_work_before_empty_pass() {
    assert_empty_scan_work_refusal("scan Parasolid curve descriptors");
}

#[test]
fn surface_descriptor_scan_admits_work_before_empty_pass() {
    assert_empty_scan_work_refusal("scan Parasolid surface descriptors");
}
