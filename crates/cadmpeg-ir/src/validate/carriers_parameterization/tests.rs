// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::parameter_in_domain;
use crate::examples::unit_cube;
use crate::geometry::{pcurve::PcurveGeometry, CurveGeometry, SolvedCurveGeometry};
use crate::ids::{CurveId, UnknownId};
use crate::math::{Point3, Vector3};
use crate::report::check::Check;
use crate::test_support::make_first_face_surface_unknown;
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
    assert!(findings
        .iter()
        .any(|finding| finding.check == Check::NativeLinks
            && finding.entity.as_deref() == Some("test:source:unknown#malformed")));
    assert!(
        !findings
            .iter()
            .any(|finding| finding.check == Check::CarrierReachability
                && finding.entity.as_deref() == Some(carrier_id.as_str())),
        "{findings:?}"
    );

    wire["native"]["test"]["unknowns"][0]["links"] = serde_json::json!([]);
    let orphan = crate::CadIr::from_json(&wire.to_string()).unwrap();
    assert!(validate_neutral(&orphan, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::CarrierReachability
            && finding.entity.as_deref() == Some(carrier_id.as_str())));
}

#[test]
fn periodic_curve_parameter_domain_is_checked() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    let curve_id = ir.model.edges[0].curve().cloned().unwrap();
    ir.model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .unwrap()
        .geometry = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        crate::geometry::analytic::CircleCurve::try_new(
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

fn nurbs_pcurve_leaf() -> PcurveGeometry {
    PcurveGeometry::Nurbs {
        nurbs: crate::geometry::pcurve::PcurveNurbs::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![
                crate::math::Point2::new(0.0, 0.0),
                crate::math::Point2::new(1.0, 1.0),
            ],
            None,
            false,
        )
        .unwrap(),
    }
}

fn placed_pcurve(placements: usize) -> PcurveGeometry {
    let mut geometry = nurbs_pcurve_leaf();
    for _ in 0..placements {
        geometry = PcurveGeometry::Transformed {
            basis: Box::new(geometry),
            transform: crate::transform::Transform2::identity(),
        };
    }
    geometry
}

#[test]
fn pcurve_bounded_domain_requirement_stops_at_the_admitted_nesting_depth() {
    let accepted = placed_pcurve(crate::geometry::MAX_GEOMETRY_NESTING);
    assert!(super::pcurve_requires_bounded_domain(&accepted));

    let refused = placed_pcurve(crate::geometry::MAX_GEOMETRY_NESTING + 1);
    assert!(!super::pcurve_requires_bounded_domain(&refused));
}

fn trimmed_over_the_nurbs_leaf() -> PcurveGeometry {
    PcurveGeometry::Trimmed(
        crate::geometry::pcurve::TrimmedPcurve::try_new(
            [0.25, 0.75],
            true,
            Box::new(nurbs_pcurve_leaf()),
        )
        .unwrap(),
    )
}

fn offset_over_the_nurbs_leaf() -> PcurveGeometry {
    PcurveGeometry::Offset(
        crate::geometry::pcurve::OffsetPcurve::try_new(0.5, Box::new(nurbs_pcurve_leaf())).unwrap(),
    )
}

/// A unit cube whose bottom coedge uses one nested pcurve over `range`.
fn cube_with_one_coedge_pcurve(
    geometry: PcurveGeometry,
    range: [f64; 2],
) -> crate::document::CadIr {
    let id = crate::ids::PcurveId::mint("synthetic:cube:pcurve#nested").expect("valid identity");
    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.model.pcurves.push(crate::geometry::pcurve::Pcurve {
        id: id.clone(),
        geometry,
        metadata: crate::geometry::pcurve::PcurveMetadata::default(),
    });
    let coedge = ir
        .model
        .coedges
        .iter_mut()
        .find(|coedge| {
            coedge.id.as_str().contains("bottom") && coedge.edge.as_str() == "synthetic:cube:edge#0"
        })
        .expect("bottom face uses edge #0");
    coedge.pcurves = vec![crate::topology::PcurveUse {
        pcurve: id,
        isoparametric: None,
        parameter_range: Some(crate::geometry::DirectedParameterRange::new(range).unwrap()),
    }];
    ir
}

fn coedge_pcurve_range_reported(ir: &crate::document::CadIr) -> bool {
    validate_neutral(ir, Vec::new())
        .findings
        .iter()
        .any(|finding| {
            finding.check == Check::ParameterDomain
                && finding.message.contains("coedge pcurve range")
        })
}

#[test]
fn a_coedge_range_outside_a_trimmed_pcurve_interval_is_out_of_domain() {
    assert!(!coedge_pcurve_range_reported(&cube_with_one_coedge_pcurve(
        trimmed_over_the_nurbs_leaf(),
        [0.25, 0.75]
    )));
    assert!(coedge_pcurve_range_reported(&cube_with_one_coedge_pcurve(
        trimmed_over_the_nurbs_leaf(),
        [0.0, 1.0]
    )));
}

#[test]
fn a_coedge_range_outside_an_offset_pcurve_basis_domain_is_out_of_domain() {
    assert!(!coedge_pcurve_range_reported(&cube_with_one_coedge_pcurve(
        offset_over_the_nurbs_leaf(),
        [0.0, 1.0]
    )));
    assert!(coedge_pcurve_range_reported(&cube_with_one_coedge_pcurve(
        offset_over_the_nurbs_leaf(),
        [0.0, 2.0]
    )));
}
