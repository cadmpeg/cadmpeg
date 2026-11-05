// SPDX-License-Identifier: Apache-2.0

use super::{b_curve, b_surface, BCurve, CurveFields, Knots, Refusal, SurfaceFields};

const LINEAR_MULT: [u16; 2] = [2, 2];
const UNIT_KNOTS: [f64; 2] = [0.0, 1.0];

fn linear(vertex_dim: usize, rational: bool, vertices: &[f64]) -> CurveFields<'_> {
    CurveFields {
        degree: 1,
        vertex_count: 2,
        vertex_dim,
        rational,
        periodic: false,
        vertices,
        knots: Knots {
            count: 2,
            multiplicities: &LINEAR_MULT,
            values: &UNIT_KNOTS,
        },
    }
}

#[test]
fn a_spatial_curve_scales_its_control_points_to_millimetres() {
    let vertices = [0.0, 0.0, 0.0, 0.001, 0.002, 0.003];
    let Ok(BCurve::Spatial(curve)) = b_curve(&linear(3, false, &vertices)) else {
        panic!("spatial curve");
    };
    let points = curve.control_points();
    assert_eq!(points[1].get().x, 1.0);
    assert_eq!(points[1].get().z, 3.0);
}

#[test]
fn a_rational_curve_divides_by_its_weight() {
    let vertices = [0.0, 0.0, 0.0, 1.0, 0.002, 0.004, 0.006, 2.0];
    let Ok(BCurve::Spatial(curve)) = b_curve(&linear(4, true, &vertices)) else {
        panic!("rational curve");
    };
    assert_eq!(curve.control_points()[1].get().y, 2.0);
}

#[test]
fn a_two_dimensional_curve_keeps_its_parameters() {
    let vertices = [0.0, 0.0, 0.5, 0.25];
    let Ok(BCurve::Planar(pcurve)) = b_curve(&linear(2, false, &vertices)) else {
        panic!("planar curve");
    };
    assert_eq!(pcurve.control_points()[1].get().u, 0.5);
}

#[test]
fn contradictory_fields_have_no_geometry() {
    let homogeneous = [0.0, 0.0, 0.0, 1.0, 0.001, 0.0, 0.0, 1.0];
    assert!(matches!(
        b_curve(&linear(4, false, &homogeneous)),
        Ok(BCurve::Spatial(_))
    ));
    assert_eq!(
        b_curve(&linear(2, true, &homogeneous[..4])),
        Err(Refusal::Fields)
    );
    let negative = [0.0, 0.0, 0.0, 1.0, 0.001, 0.0, 0.0, -1.0];
    assert_eq!(b_curve(&linear(4, true, &negative)), Err(Refusal::Fields));
    let short = [0.0, 0.0, 0.0];
    assert_eq!(b_curve(&linear(3, false, &short)), Err(Refusal::Fields));
}

#[test]
fn entries_past_the_declared_knot_count_carry_zero_multiplicity() {
    let vertices = [0.0, 0.0, 0.0, 0.001, 0.0, 0.0];
    let mut fields = linear(3, false, &vertices);
    let trailing_zero = [2, 2, 0];
    let trailing_knots = [0.0, 1.0, 9.0];
    fields.knots = Knots {
        count: 2,
        multiplicities: &trailing_zero,
        values: &trailing_knots,
    };
    assert!(matches!(b_curve(&fields), Ok(BCurve::Spatial(_))));
    let trailing_nonzero = [2, 2, 1];
    fields.knots.multiplicities = &trailing_nonzero;
    assert_eq!(b_curve(&fields), Err(Refusal::Fields));
    let zero_inside = [2, 0];
    fields.knots.multiplicities = &zero_inside;
    assert_eq!(b_curve(&fields), Err(Refusal::Fields));
}

#[test]
fn a_multiplicity_past_the_knot_total_has_no_geometry() {
    let vertices = [0.0, 0.0, 0.0, 0.001, 0.0, 0.0];
    let mut fields = linear(3, false, &vertices);
    let oversized = [u16::MAX, u16::MAX];
    fields.knots.multiplicities = &oversized;
    assert_eq!(b_curve(&fields), Err(Refusal::Fields));
}

#[test]
fn a_bilinear_surface_keeps_its_u_major_grid_and_normal_sense() {
    let vertices = [
        0.0, 0.0, 0.0, 0.0, 0.001, 0.0, 0.001, 0.0, 0.0, 0.001, 0.001, 0.002,
    ];
    let knots = Knots {
        count: 2,
        multiplicities: &LINEAR_MULT,
        values: &UNIT_KNOTS,
    };
    let fields = SurfaceFields {
        degrees: [1, 1],
        vertex_counts: [2, 2],
        vertex_dim: 3,
        rational: false,
        periodic: [false, false],
        vertices: &vertices,
        knots: [knots, knots],
    };
    let surface = b_surface(&fields, true).expect("surface");
    let poles = surface.poles();
    assert_eq!(poles[1].get().y, 1.0);
    assert_eq!(poles[2].get().x, 1.0);
    assert_eq!(poles[3].get().z, 2.0);
    assert!(surface.normal_reversed());
}
