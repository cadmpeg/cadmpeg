// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::parameter_in_domain;
use crate::examples::unit_cube;
use crate::geometry::{CurveGeometry, SurfaceGeometry};
use crate::ids::{CurveId, UnknownId};
use crate::math::{Point3, Vector3};
use crate::report::Check;
use crate::unknown::NativeUnknownRecord;
use crate::validate::validate_neutral;

#[test]
fn parameter_domain_accepts_serialization_rounding_at_a_boundary() {
    let lower = 0.1_f64;
    let upper = std::f64::consts::TAU;
    let one_ulp_below_lower = f64::from_bits(lower.to_bits() - 1);
    let one_ulp_above_upper = f64::from_bits(upper.to_bits() + 1);

    assert!(parameter_in_domain(one_ulp_below_lower, [lower, upper]));
    assert!(parameter_in_domain(one_ulp_above_upper, [lower, upper]));
    assert!(!parameter_in_domain(lower - 1.0e-8, [lower, upper]));
    assert!(!parameter_in_domain(upper + 1.0e-8, [lower, upper]));
}

/// Replace the surface of the cube's first face with an unknown surface,
/// optionally linking a preserved record, and return the face id and its
/// surface id. Leaves every loop/coedge/edge of the face intact.
fn make_first_face_surface_unknown(ir: &mut crate::CadIr, record: Option<UnknownId>) -> String {
    let face = &ir.model.faces[0];
    let surface_id = face.surface.0.clone();
    for s in &mut ir.model.surfaces {
        if s.id.0 == surface_id {
            s.geometry = SurfaceGeometry::Unknown { record };
            break;
        }
    }
    surface_id
}

#[test]
fn face_on_unknown_surface_validates_clean() {
    let mut ir = unit_cube();
    // Preserve a raw record and point the unknown surface at it.
    let rec = UnknownId("synthetic:cube:unknown#0".into());
    ir.set_native_unknowns(
        "synthetic",
        &[NativeUnknownRecord {
            id: rec.clone(),
            links: Vec::new(),
        }],
    )
    .unwrap();
    make_first_face_surface_unknown(&mut ir, Some(rec));

    let report = validate_neutral(&ir, Vec::new());
    assert!(
        report.is_ok(),
        "a face on an unknown surface is legal, got: {:?}",
        report.findings
    );
    // The face and its topology stay in the graph.
    assert_eq!(ir.model.faces.len(), 6);
    // The situation is surfaced as a count.
    assert_eq!(
        report.entity_counts.get("surfaces_unknown_geometry"),
        Some(&1)
    );
}

#[test]
fn unknown_surface_without_record_is_legal() {
    let mut ir = unit_cube();
    make_first_face_surface_unknown(&mut ir, None);
    let report = validate_neutral(&ir, Vec::new());
    assert!(
        report.is_ok(),
        "an unknown surface need not preserve bytes, got: {:?}",
        report.findings
    );
    assert_eq!(
        report.entity_counts.get("surfaces_unknown_geometry"),
        Some(&1)
    );
}

#[test]
fn unknown_surface_dangling_record_is_flagged() {
    let mut ir = unit_cube();
    // Link a record id that is not in the unknowns arena.
    make_first_face_surface_unknown(&mut ir, Some(UnknownId("missing".into())));
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::ReferentialIntegrity
            && finding.message.contains("missing unknown record `missing`")
    }));
}

#[test]
fn orphan_carrier_is_flagged() {
    let mut ir = unit_cube();
    let mut orphan = ir.model.curves[0].clone();
    orphan.id = CurveId("zz:orphan".into());
    ir.model.curves.push(orphan);
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::CarrierReachability));
}

#[test]
fn periodic_curve_parameter_domain_is_checked() {
    let mut ir = unit_cube();
    let curve_id = ir.model.edges[0].curve.clone().unwrap();
    ir.model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .unwrap()
        .geometry = CurveGeometry::Circle {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 1.0,
    };
    ir.model.edges[0].param_range = Some([0.0, 7.0]);
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::ParameterDomain));

    ir.model.edges[0].param_range = Some([-std::f64::consts::PI, std::f64::consts::PI]);
    assert!(!validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::ParameterDomain));
}
