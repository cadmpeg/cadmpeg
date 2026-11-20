// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::{sample, Approximator, Axis, SEED_PATCHES};
use crate::eval::admission::EvaluationAdmission;
use crate::geometry::nurbs::NurbsCurve;
use crate::geometry::pcurve::{LinePcurve, PcurveGeometry, PlacedPcurve};
use crate::geometry::{
    Curve, CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve,
    ProceduralCurveDefinition, SupportPcurve, SurfaceCurveFamily, MAX_GEOMETRY_NESTING,
};
use crate::ids::{CurveId, ProceduralCurveId};
use crate::math::{Point2, Point3};
use crate::transform::Transform2;

#[test]
fn fitting_retains_an_evaluation_resource_refusal() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let mut pcurve = PcurveGeometry::Line(
        LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)).unwrap(),
    );
    for _ in 0..MAX_GEOMETRY_NESTING {
        pcurve = PcurveGeometry::Transformed(
            PlacedPcurve::try_new(Box::new(pcurve), Transform2::identity()).unwrap(),
        );
    }
    let curve = CurveId::mint("test:approximation:curve#deep").unwrap();
    let construction = ProceduralCurveId::mint("test:approximation:construction#deep").unwrap();
    let context = IntcurveSupportContext::try_new(
        [
            IntcurveSupportSide {
                surface: Some(ir.geometry.faces[0].surface.clone()),
                pcurve: Some(SupportPcurve::new(pcurve, None)),
            },
            IntcurveSupportSide {
                surface: None,
                pcurve: None,
            },
        ],
        [0.0, 1.0],
        [Vec::new(), Vec::new(), Vec::new()],
    )
    .unwrap();
    ir.geometry.procedural_curves.push(ProceduralCurve::new(
        construction.clone(),
        ProceduralCurveDefinition::SurfaceCurve {
            family: SurfaceCurveFamily::Parametric {
                context,
                tail: None,
            },
        },
    ));
    ir.geometry.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Procedural {
            construction,
            cache: None,
        },
        parameter_range: None,
        source_object: None,
        native_ref: None,
    });
    let mut approximator = Approximator::new(&ir);
    assert!(approximator.curve(curve.as_str()).is_none());
    let refusal = approximator
        .resource_limit()
        .expect("recursion refusal is retained");
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::RecursionDepth
    );
}

/// The sampled residual limit of a model whose linear tolerance is `1e-6`.
const RESIDUAL: f64 = 2.5e-7;

#[test]
fn breaks_at_carrier_knots_fit_each_quadratic_span_at_the_seed() {
    let square = NurbsCurve::from_lanes(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0, 3.0],
        vec![
            Point3::new(-10.0, -10.0, 0.0),
            Point3::new(10.0, -10.0, 0.0),
            Point3::new(10.0, 10.0, 0.0),
            Point3::new(-10.0, 10.0, 0.0),
            Point3::new(-10.0, -10.0, 0.0),
        ],
        None,
        false,
    )
    .unwrap();
    let mut evaluations = 0_usize;
    let mut extrusion = |u: f64, v: f64| {
        evaluations += 1;
        let point =
            crate::eval::decode::nurbs_curve_point_at(EvaluationAdmission::Standard, &square, u)
                .expect("the square has a point across its domain")
                .get();
        Some(Point3::new(point.x, point.y, point.z - v))
    };
    let mut samples = usize::MAX;
    let grid = sample(
        [
            Axis::new([0.0, 3.0], &[1.0, 2.0], SEED_PATCHES).unwrap(),
            Axis::new([-5.0, 0.0], &[], SEED_PATCHES).unwrap(),
        ],
        false,
        RESIDUAL,
        &mut samples,
        usize::MAX,
        &mut extrusion,
    )
    .expect("a cubic patch holds each quadratic span");
    assert_eq!(
        grid.knots[0],
        [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 3.0, 3.0, 3.0, 3.0]
    );
    assert_eq!(grid.rows.len(), 3 * 3 + 1);
    // One level: the (3 * 3 + 1) by (3 * 4 + 1) grid, and 25 probes in each
    // of the 3 by 4 patches.
    assert_eq!(evaluations, 10 * 13 + 3 * 4 * 25);
}
