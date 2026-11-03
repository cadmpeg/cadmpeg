// SPDX-License-Identifier: Apache-2.0

use super::withhold_unresolved_cone_origins;
use crate::surface::{
    ApexConeHalfAngle, InlineSurfaceCarrier, InlineType26MinorRadius, PositionalConeFrame,
    SurfaceBodyBoundary, SurfaceKind, SurfaceParameterCarrier, SurfaceParameterRecord,
};

fn body(reference: u8) -> Vec<u8> {
    vec![
        0x18, 0x10, 0x18, 0x10, 0x18, 0xe6, 0x10, 0x18, reference, 0xc6, 0x0f, 0xaf, 0xbf, 0xd9,
        0x73, 0x04, 0x2f, 0x34, 0x00, 0x74, 0x21, 0xfb, 0x54, 0x44, 0x2d, 0x19,
    ]
}

fn record(body: Vec<u8>) -> SurfaceParameterRecord {
    SurfaceParameterRecord {
        surface_id: 17,
        body,
        scalar_tokens: Vec::new(),
        opaque_spans: Vec::new(),
        scalar_frames: Vec::new(),
        carrier: SurfaceParameterCarrier::WithheldConeOrigin { suffix_start: 0 },
        type26_minor_radius: InlineType26MinorRadius::Absent,
        boundary: SurfaceBodyBoundary::CompoundClose,
        offset: 30,
        body_offset: 38,
    }
}

#[test]
fn compact_cone_origin_rejection_requires_recorded_owner() {
    let mut r = record(body(0x19));
    let angle = ApexConeHalfAngle::new(std::f64::consts::FRAC_PI_4).expect("angle");
    let frame =
        PositionalConeFrame::new([4.0, -1.0, 20.0], [0.0, 0.0, -1.0], [0.0, -1.0, 0.0], angle)
            .expect("frame");
    r.carrier = SurfaceParameterCarrier::Resolved(InlineSurfaceCarrier::Cone(frame));
    let before = r.clone();
    crate::decode::with_service_context(|ctx| {
        assert!(!r
            .withhold_unresolved_cone_origin(ctx)
            .expect("no owned marker"));
    });
    assert_eq!(r, before);
}

#[test]
fn compact_cone_origin_rejection_retains_raw_and_marks_incident_curves() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces
        .rows
        .push(crate::decode::tests::surface_row(17, 6, SurfaceKind::Cone));
    let raw = body(0x19);
    scan.surfaces.parameters.push(record(raw.clone()));
    scan.curves
        .topology_rows
        .push(crate::curve::CurveTopologyRow {
            id: 9,
            type_byte: 1,
            feature_id: 6,
            directions: [1, 1],
            faces: [std::num::NonZeroU32::new(17), std::num::NonZeroU32::new(18)],
            next_edges: [9, 9],
            offset: 60,
        });
    crate::decode::with_service_context(|ctx| {
        withhold_unresolved_cone_origins(ctx, &mut scan).expect("owned cone");
    });
    let r = &scan.surfaces.parameters[0];
    assert_eq!(r.body, raw);
    assert!(r.positional_cone_frame().is_none());
    assert!(r.scalar_tokens.is_empty());
    assert!(r.scalar_frames.is_empty());
    assert_eq!(r.opaque_spans.len(), 1);
    assert_eq!(r.opaque_spans[0].raw.as_slice(), raw);
    assert!(scan.surfaces.withheld_cone_origin_surface_ids.contains(&17));
    assert!(scan.surfaces.withheld_cone_origin_curve_ids.contains(&9));
}

#[test]
fn compact_cone_origin_rejection_requires_unique_cone_row_and_parameter() {
    for (kind, duplicate_row, duplicate_parameter) in [
        (SurfaceKind::Cylinder, false, false),
        (SurfaceKind::Cone, true, false),
        (SurfaceKind::Cone, false, true),
    ] {
        let mut scan = crate::test_support::empty_container_scan();
        let row = crate::decode::tests::surface_row(17, 6, kind);
        scan.surfaces.rows.push(row.clone());
        if duplicate_row {
            scan.surfaces.rows.push(row);
        }
        let r = record(body(0x19));
        scan.surfaces.parameters.push(r.clone());
        if duplicate_parameter {
            scan.surfaces.parameters.push(r);
        }
        let before = scan.surfaces.parameters.clone();
        crate::decode::with_service_context(|ctx| {
            withhold_unresolved_cone_origins(ctx, &mut scan).expect("unmatched owner");
        });
        assert_eq!(scan.surfaces.parameters, before);
        assert!(scan.surfaces.withheld_cone_origin_surface_ids.is_empty());
    }
}
