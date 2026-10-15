// SPDX-License-Identifier: Apache-2.0
//! Values the writer computes from admitted geometry are admitted before
//! they are written: a 3DM archive states no non-finite number.

use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point3, Vector3};

/// The writer refuses `written` with a `NotImplemented` error that names
/// `field`, where it wrote an infinity.
fn assert_refused(written: Result<Vec<u8>, CodecError>, field: &str) {
    let Err(CodecError::NotImplemented(message)) = written else {
        panic!("{field}: {written:?}");
    };
    assert!(message.contains(field), "{message}");
}

/// A circle arc point past the largest finite `f64`.
#[test]
fn a_circle_whose_arc_point_overflows_is_refused() {
    assert_refused(
        super::super::circle_payload(
            Point3::new(1.7e308, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0e308,
        ),
        "circle arc point coordinate",
    );
}

/// A plane whose equation constant, the dot product of its normal and
/// origin, overflows.
#[test]
fn a_plane_whose_equation_constant_overflows_is_refused() {
    assert_refused(
        super::super::plane_surface_payload(
            Point3::new(1.7e308, 1.7e308, 0.0),
            Vector3::new(0.6, 0.8, 0.0),
            Vector3::new(0.8, -0.6, 0.0),
        ),
        "plane equation constant",
    );
}

/// A rational NURBS curve pole whose coordinate times its weight overflows.
#[test]
fn a_nurbs_curve_whose_homogeneous_pole_overflows_is_refused() {
    let curve = cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(1.0e300, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
        Some(vec![1.0e10, 1.0]),
        false,
    )
    .expect("finite poles and positive weights are admitted by the IR");
    assert_refused(
        super::super::nurbs_curve_payload(&curve),
        "NURBS curve homogeneous pole coordinate",
    );
}

/// A rational NURBS surface pole whose coordinate times its weight
/// overflows.
#[test]
fn a_nurbs_surface_whose_homogeneous_pole_overflows_is_refused() {
    use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};

    let surface = NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(1.0e300, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
            ],
            Some(vec![vec![1.0e10, 1.0], vec![1.0, 1.0]]),
        ),
        false,
    )
    .expect("finite poles and positive weights are admitted by the IR");
    assert_refused(
        super::super::nurbs_surface_payload(&surface, 4),
        "NURBS surface homogeneous pole coordinate",
    );
}

/// A line whose computed endpoint, here a sum of two finite coordinates,
/// overflows. The trim and projected-line routes compute their endpoints
/// from a parameterization or a plane projection.
#[test]
fn a_line_whose_computed_endpoint_overflows_is_refused() {
    let endpoint = 1.7e308 + 1.7e308;
    assert_refused(
        super::super::bounded_line_payload([0.0; 3], [endpoint, 0.0, 0.0], [0.0, 1.0], 2),
        "line endpoint coordinate",
    );
}
