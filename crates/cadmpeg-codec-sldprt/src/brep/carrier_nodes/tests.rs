// SPDX-License-Identifier: Apache-2.0

use super::analytic;
use crate::brep::topology::nodes::sequence;
use crate::brep::Carrier;
use crate::loss::SldprtLossCode;
use crate::test_support::parasolid::sequence::{sequence_of, F};
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};

fn prefix(node_id: u32, sense: u8) -> Vec<F> {
    vec![
        F::I(node_id),
        F::P(1),
        F::P(1),
        F::P(1),
        F::P(1),
        F::P(1),
        F::C(sense),
    ]
}

fn plane(sense: u8, normal: [f64; 3]) -> Vec<F> {
    let mut fields = prefix(11, sense);
    fields.extend([F::V([0.0, 0.0, 0.001]), F::V(normal), F::V([1.0, 0.0, 0.0])]);
    fields
}

fn circle(radius: f64) -> Vec<F> {
    let mut fields = prefix(12, b'+');
    fields.extend([
        F::V([0.0; 3]),
        F::V([0.0, 0.0, 1.0]),
        F::V([1.0, 0.0, 0.0]),
        F::R(radius),
    ]);
    fields
}

fn cone(sine: f64, cosine: f64) -> Vec<F> {
    let mut fields = prefix(13, b'+');
    fields.extend([
        F::V([0.0; 3]),
        F::V([0.0, 0.0, 1.0]),
        F::R(0.002),
        F::R(sine),
        F::R(cosine),
        F::V([1.0, 0.0, 0.0]),
    ]);
    fields
}

#[test]
fn analytic_nodes_decode_through_their_compact_form() {
    let (payload, body_offset) = sequence_of(&[
        (50, 3, plane(b'-', [0.0, 0.0, 1.0])),
        (31, 4, circle(0.002)),
    ]);
    let records = sequence(&payload).unwrap();
    let carriers = analytic(&records, body_offset, &mut Vec::new()).unwrap();
    assert_eq!(carriers.len(), 2);
    let Carrier::Surface(plane) = &carriers[0] else {
        panic!("PLANE is a surface")
    };
    assert!(matches!(
        plane.geometry,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
    ));
    assert_eq!(plane.end, payload.len() - 4 - body_offset - (circle_len()));
    let Carrier::Curve(circle) = &carriers[1] else {
        panic!("CIRCLE is a curve")
    };
    assert!(matches!(
        circle.geometry,
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(_))
    ));
    assert_eq!(circle.marker_reversed, Some(false));
    assert_eq!(circle.end, payload.len() - 4 - body_offset);
}

/// The encoded length of the CIRCLE node in these fixtures.
fn circle_len() -> usize {
    2 + 1 + 2 + 4 + 5 * 2 + 1 + 3 * 24 + 8
}

#[test]
fn values_outside_the_compact_checks_omit_only_that_carrier() {
    let (payload, body_offset) = sequence_of(&[
        (50, 3, plane(b'+', [0.0, 0.0, 2.0])),
        (31, 4, circle(-1.0)),
        (31, 5, circle(0.002)),
    ]);
    let records = sequence(&payload).unwrap();
    let carriers = analytic(&records, body_offset, &mut Vec::new()).unwrap();
    assert_eq!(carriers.len(), 1);
    assert!(matches!(carriers[0], Carrier::Curve(_)));
}

#[test]
fn repeated_analytic_indices_leave_the_stream_to_the_scanners() {
    let (payload, body_offset) = sequence_of(&[(31, 4, circle(0.002)), (31, 4, circle(0.003))]);
    let records = sequence(&payload).unwrap();
    assert!(analytic(&records, body_offset, &mut Vec::new()).is_none());
}

#[test]
fn a_cone_node_with_a_negative_half_angle_field_is_omitted_with_a_loss() {
    let root_half = std::f64::consts::FRAC_1_SQRT_2;
    let (payload, body_offset) = sequence_of(&[
        (52, 3, cone(root_half, root_half)),
        (52, 4, cone(-root_half, root_half)),
    ]);
    let records = sequence(&payload).unwrap();
    let mut refusals = Vec::new();
    let carriers = analytic(&records, body_offset, &mut refusals).unwrap();
    assert_eq!(carriers.len(), 1);
    assert_eq!(refusals.len(), 1);
    assert_eq!(
        refusals[0].code,
        SldprtLossCode::GeometryConeHalfAngleSignUnresolved.kind()
    );
}

#[test]
fn a_cone_node_off_the_unit_identity_by_1e7_is_omitted_with_a_loss() {
    let sine = 0.6_f64;
    let cosine = (1.0 + 1.0e-7 - sine * sine).sqrt();
    let (payload, body_offset) = sequence_of(&[(52, 3, cone(sine, cosine))]);
    let records = sequence(&payload).unwrap();
    let mut refusals = Vec::new();
    let carriers = analytic(&records, body_offset, &mut refusals).unwrap();
    assert!(carriers.is_empty());
    assert_eq!(refusals.len(), 1);
    assert_eq!(
        refusals[0].code,
        SldprtLossCode::GeometryConeHalfAngleIdentityRefused.kind()
    );
}
