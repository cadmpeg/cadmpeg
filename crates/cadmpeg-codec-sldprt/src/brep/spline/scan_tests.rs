// SPDX-License-Identifier: Apache-2.0
//! Source scanner admission before inspection.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn assert_empty_scan_work_refusal(operation: &str) {
    let body = [0xff; 4096];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&body, &arena, &policy).unwrap();
    let (result, allocations) =
        crate::test_support::allocation::count_allocations(|| match operation {
            "scan Parasolid spline arrays" => {
                super::scan_arrays(&ctx, &body, None, None).map(|_| ())
            }
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

