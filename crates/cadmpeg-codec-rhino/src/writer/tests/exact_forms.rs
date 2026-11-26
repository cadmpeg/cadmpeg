// SPDX-License-Identifier: Apache-2.0
//! Carriers that 3DM holds only as their rational NURBS forms are written in
//! those forms, and a face without one is left out of an open Brep.

use std::f64::consts::{PI, TAU};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::geometry::analytic::{CircleCurve, CylinderSurface, EllipseCurve};
use cadmpeg_ir::geometry::pcurve::{LinePcurve, Pcurve, PcurveGeometry, PcurveMetadata};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, PcurveId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::topology::{EdgeCarrier, PcurveUse};
use cadmpeg_ir::units::FiniteVector;

use super::{adjacent_quad_sheet, polygon_sheet, written};
use crate::loss::RhinoLossCode;

/// Distance within which a written NURBS form lies on its analytic source.
const EXACT_FORM_AGREEMENT: f64 = 1e-9;

const RADIUS: f64 = 2.0;
const HEIGHT: f64 = 3.0;

fn omitted(losses: &[String]) -> Vec<&String> {
    let code = RhinoLossCode::WriterRecordOmitted.kind().to_string();
    losses
        .iter()
        .filter(|loss| loss.starts_with(&code))
        .collect()
}

fn line_pcurve(id: &str, origin: Point2, direction: Point2, range: [f64; 2]) -> Pcurve {
    Pcurve {
        id: PcurveId::mint(id).expect("identity grammar"),
        geometry: PcurveGeometry::Line(
            LinePcurve::try_new(origin, direction).expect("line pcurve fixture"),
        ),
        metadata: PcurveMetadata::general(
            None,
            Some(FiniteVector::new(range).expect("finite fixture range")),
            None,
        ),
        native_ref: None,
    }
}

/// The half of the cylinder of radius 2 about the z axis over angles
/// `[0, π]` and heights `[0, 3]`, bounded by two half circles and two
/// rulings, each with its line pcurve on the cylinder.
fn half_cylinder_sheet() -> CadIr {
    let mut ir = polygon_sheet(&[
        Point3::new(RADIUS, 0.0, 0.0),
        Point3::new(-RADIUS, 0.0, 0.0),
        Point3::new(-RADIUS, 0.0, HEIGHT),
        Point3::new(RADIUS, 0.0, HEIGHT),
    ]);
    let z = Vector3::new(0.0, 0.0, 1.0);
    let x = Vector3::new(1.0, 0.0, 0.0);
    ir.geometry.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        CylinderSurface::try_new(Point3::new(0.0, 0.0, 0.0), z, x, RADIUS)
            .expect("cylinder fixture"),
    ));
    let arcs = [
        (0, Point3::new(0.0, 0.0, 0.0), z, x),
        (
            2,
            Point3::new(0.0, 0.0, HEIGHT),
            Vector3::new(0.0, 0.0, -1.0),
            Vector3::new(-1.0, 0.0, 0.0),
        ),
    ];
    for (index, center, axis, reference) in arcs {
        ir.geometry.curves[index].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            CircleCurve::try_new(center, axis, reference, RADIUS).expect("circle fixture"),
        ));
        ir.geometry.edges[index].carrier =
            EdgeCarrier::new(ir.geometry.edges[index].curve().cloned(), Some([0.0, PI]))
                .expect("edge carrier");
    }
    let pcurves = [
        (Point2::new(0.0, 0.0), Point2::new(1.0, 0.0), [0.0, PI]),
        (Point2::new(PI, 0.0), Point2::new(0.0, 1.0), [0.0, HEIGHT]),
        (Point2::new(PI, HEIGHT), Point2::new(-1.0, 0.0), [0.0, PI]),
        (
            Point2::new(0.0, HEIGHT),
            Point2::new(0.0, -1.0),
            [0.0, HEIGHT],
        ),
    ];
    for (index, (origin, direction, range)) in pcurves.into_iter().enumerate() {
        let pcurve = line_pcurve(
            &format!("cadir:model:pcurve#half-cylinder.{index}"),
            origin,
            direction,
            range,
        );
        ir.geometry.coedges[index].pcurves = vec![PcurveUse {
            pcurve: pcurve.id.clone(),
            isoparametric: None,
            parameter_range: None,
        }];
        ir.geometry.pcurves.push(pcurve);
    }
    ir.finalize();
    ir
}

fn assert_on_half_cylinder(point: Point3) {
    let radial = point.x.hypot(point.y);
    assert!((radial - RADIUS).abs() <= EXACT_FORM_AGREEMENT, "{point:?}");
    assert!(point.y >= -EXACT_FORM_AGREEMENT, "{point:?}");
    assert!(
        (-EXACT_FORM_AGREEMENT..=HEIGHT + EXACT_FORM_AGREEMENT).contains(&point.z),
        "{point:?}"
    );
}

fn domain_samples(knots: &[f64], degree: u32) -> Vec<f64> {
    let degree = usize::try_from(degree).expect("fixture degree fits usize");
    let (lo, hi) = (knots[degree], knots[knots.len() - degree - 1]);
    (0..=8)
        .map(|step| lo + (hi - lo) * f64::from(step) / 8.0)
        .collect()
}

#[test]
fn a_cylinder_face_with_arc_edges_is_written_as_its_rational_nurbs_form() {
    let ir = half_cylinder_sheet();
    let (decoded, losses) = written(&ir);
    assert!(omitted(&losses).is_empty(), "{losses:?}");
    assert_eq!(decoded.geometry.faces.len(), 1, "{losses:?}");
    assert_eq!(decoded.geometry.edges.len(), 4, "{losses:?}");

    let Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface))) = decoded
        .geometry
        .surfaces
        .first()
        .map(|surface| &surface.geometry)
    else {
        panic!("the written face carries a NURBS surface");
    };
    for u in domain_samples(surface.u_knots(), surface.u_degree()) {
        for v in domain_samples(surface.v_knots(), surface.v_degree()) {
            let point = cadmpeg_ir::eval::decode::nurbs_surface_point(
                EvaluationAdmission::Standard,
                surface,
                u,
                v,
            )
            .expect("the surface has a point")
            .get();
            assert_on_half_cylinder(point);
        }
    }
    for curve in &decoded.geometry.curves {
        let Some(SolvedCurveGeometry::Nurbs(nurbs)) = curve.geometry.solved() else {
            continue;
        };
        for parameter in domain_samples(nurbs.knots(), nurbs.degree()) {
            let point = cadmpeg_ir::eval::decode::nurbs_curve_point_at(
                EvaluationAdmission::Standard,
                nurbs,
                parameter,
            )
            .expect("the edge curve has a point")
            .get();
            assert_on_half_cylinder(point);
        }
    }
    cadmpeg_measure::test_support::round_trip::measures_agree("half cylinder", &ir, &decoded)
        .expect("the round trip keeps its measured geometry");
}

#[test]
fn a_face_without_a_nurbs_form_is_left_out_of_an_open_brep() {
    let mut ir = adjacent_quad_sheet();
    let dropped = ir.geometry.faces[1].id.as_str().to_owned();
    let kept = ir.geometry.faces[0].id.as_str().to_owned();
    let surface = ir.geometry.faces[1].surface.clone();
    let record = ir
        .geometry
        .surfaces
        .iter_mut()
        .find(|record| record.id == surface)
        .expect("the face's surface");
    record.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            RADIUS,
        )
        .expect("cylinder fixture"),
    ));

    let (decoded, losses) = written(&ir);
    assert_eq!(decoded.geometry.faces.len(), 1, "{losses:?}");
    let omitted = omitted(&losses);
    assert!(
        omitted
            .iter()
            .any(|loss| loss.contains(&dropped)
                && loss.contains("its pcurves span no parameter area")),
        "{losses:?}"
    );
    assert!(
        omitted.iter().all(|loss| !loss.contains(&kept)),
        "{losses:?}"
    );
}

#[test]
fn a_free_ellipse_is_written_as_its_rational_nurbs_form() {
    let (major, minor) = (3.0, 1.0);
    let mut ir = CadIr::empty();
    ir.geometry.curves.push(Curve {
        parameter_range: None,
        id: CurveId::mint("cadir:model:curve#ellipse").expect("identity grammar"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
            EllipseCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                major,
                minor,
            )
            .expect("ellipse fixture"),
        )),
        source_object: None,
        native_ref: None,
    });

    let (decoded, losses) = written(&ir);
    assert!(omitted(&losses).is_empty(), "{losses:?}");
    let [curve] = decoded.geometry.curves.as_slice() else {
        panic!("one curve is written: {losses:?}");
    };
    let Some(SolvedCurveGeometry::Nurbs(nurbs)) = curve.geometry.solved() else {
        panic!("the ellipse is written as a NURBS curve");
    };
    let samples = domain_samples(nurbs.knots(), nurbs.degree());
    for parameter in &samples {
        let point = cadmpeg_ir::eval::decode::nurbs_curve_point_at(
            EvaluationAdmission::Standard,
            nurbs,
            *parameter,
        )
        .expect("the curve has a point")
        .get();
        let level = (point.x / major).powi(2) + (point.y / minor).powi(2);
        assert!((level - 1.0).abs() <= EXACT_FORM_AGREEMENT, "{point:?}");
        assert!(point.z.abs() <= EXACT_FORM_AGREEMENT, "{point:?}");
    }
    assert!((samples[8] - samples[0] - TAU).abs() <= EXACT_FORM_AGREEMENT);
}
