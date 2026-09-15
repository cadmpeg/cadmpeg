// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::parameter_in_domain;
use crate::examples::unit_cube;
use crate::geometry::{CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry};
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
    let surface_id = face.surface.as_str().to_owned();
    for s in &mut ir.model.surfaces {
        if s.id.as_str() == surface_id {
            s.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record });
            break;
        }
    }
    surface_id
}

#[test]
fn face_on_unknown_surface_validates_clean() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    // Preserve a raw record and point the unknown surface at it.
    let rec = UnknownId::mint("synthetic:cube:unknown#0").expect("valid identity");
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
    let mut ir = unit_cube().expect("valid unit cube fixture");
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
    let mut ir = unit_cube().expect("valid unit cube fixture");
    // Link a record id that is not in the unknowns arena.
    make_first_face_surface_unknown(
        &mut ir,
        Some(UnknownId::mint("test:model:entity#missing").expect("valid identity")),
    );
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::ReferentialIntegrity
            && finding
                .message
                .contains("missing unknown record `test:model:entity#missing`")
    }));
}

#[test]
fn orphan_carrier_is_flagged() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    let mut orphan = ir.model.curves[0].clone();
    orphan.id = CurveId::mint("test:model:entity#zz:orphan").expect("valid identity");
    ir.model.curves.push(orphan);
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::CarrierReachability));
}

#[test]
fn malformed_unknown_does_not_erase_another_records_carrier_link() {
    let mut ir = unit_cube().unwrap();
    let mut carrier = ir.model.curves[0].clone();
    carrier.id = CurveId::mint("test:model:curve#native-only").unwrap();
    let carrier_id = carrier.id.as_str().to_owned();
    ir.model.curves.push(carrier);
    ir.model.finalize();
    let mut wire = serde_json::to_value(&ir).unwrap();
    wire["native"] = serde_json::json!({"test": {"unknowns": [
        {"id": "test:source:unknown#good", "links": [carrier_id]},
        {"id": "test:source:unknown#malformed", "links": [null]}
    ]}});
    let parsed = crate::CadIr::from_json(&wire.to_string()).unwrap();
    let findings = validate_neutral(&parsed, Vec::new()).findings;
    assert!(findings.iter().any(|finding| finding.check == Check::NativeLinks
        && finding.entity.as_deref() == Some("test:source:unknown#malformed")));
    assert!(!findings.iter().any(|finding| finding.check == Check::CarrierReachability
        && finding.entity.as_deref() == Some(carrier_id.as_str())), "{findings:?}");

    wire["native"]["test"]["unknowns"][0]["links"] = serde_json::json!([]);
    let orphan = crate::CadIr::from_json(&wire.to_string()).unwrap();
    assert!(validate_neutral(&orphan, Vec::new()).findings.iter().any(|finding|
        finding.check == Check::CarrierReachability
        && finding.entity.as_deref() == Some(carrier_id.as_str())));
}

#[test]
fn periodic_curve_parameter_domain_is_checked() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    let curve_id = ir.model.edges[0].curve().clone().unwrap();
    ir.model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .unwrap()
        .geometry = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        crate::geometry::CircleCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
        )
        .unwrap(),
    ));
    ir.model.edges[0].set_param_range(Some([0.0, 7.0])).unwrap();
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::ParameterDomain));

    ir.model.edges[0]
        .set_param_range(Some([-std::f64::consts::PI, std::f64::consts::PI]))
        .unwrap();
    assert!(!validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::ParameterDomain));
}
