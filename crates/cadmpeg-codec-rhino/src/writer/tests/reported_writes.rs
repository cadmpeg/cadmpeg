// SPDX-License-Identifier: Apache-2.0
//! Documents with content a 3DM archive cannot state as given are written:
//! the writer restates each value it can state exactly, leaves out the
//! smallest unit it cannot, and names each in a loss (§3.4).

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::ids::{CurveId, PointId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{Point, Shell};

use super::{adjacent_quad_sheet, polygon_sheet, written};
use crate::loss::RhinoLossCode;

/// The tolerance of a decoded coordinate that the archive stores as `f64`.
const EPS_COORDINATE: f64 = 1.0e-12;

fn has_loss(losses: &[String], code: &str, needle: &str) -> bool {
    losses
        .iter()
        .any(|loss| loss.starts_with(code) && loss.contains(needle))
}

fn free_point(ir: &mut CadIr, name: &str, position: Point3) {
    ir.geometry.points.push(Point::new(
        PointId::mint(format!("cadir:model:point#{name}")).expect("identity grammar"),
        cadmpeg_ir::features::geometry::FinitePoint3::new(position)
            .expect("a finite position is a point"),
        None,
    ));
}

#[test]
fn a_body_with_two_shells_is_written_as_one_brep_per_shell() {
    let mut ir = adjacent_quad_sheet();
    let region = ir.geometry.regions[0].id.clone();
    let first = ir.geometry.shells[0].id.clone();
    let second =
        cadmpeg_ir::ids::ShellId::mint("cadir:model:shell#second").expect("identity grammar");
    ir.geometry
        .shells
        .push(Shell::new(second.clone(), region.clone()));
    ir.geometry.regions[0].set_shells(vec![first, second.clone()]);
    ir.geometry.faces[1].shell = second;
    let body = ir.geometry.bodies[0].id.as_str().to_owned();

    let (decoded, losses) = written(&ir);
    assert_eq!(decoded.geometry.bodies.len(), 2);
    assert_eq!(decoded.geometry.faces.len(), 2);
    assert!(
        has_loss(
            &losses,
            &RhinoLossCode::WriterBodySplit.kind().to_string(),
            &body
        ),
        "{losses:?}"
    );
}

#[test]
fn a_rigidly_placed_sheet_is_written_at_its_placed_position() {
    let mut ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    ir.geometry.bodies[0].transform = Some(
        cadmpeg_ir::transform::Transform::affine([
            [0.0, -1.0, 0.0, 10.0],
            [1.0, 0.0, 0.0, 20.0],
            [0.0, 0.0, 1.0, 30.0],
        ])
        .expect("finite rows"),
    );

    let (decoded, _) = written(&ir);
    assert!(decoded.geometry.bodies[0].transform.is_none());
    for expected in [
        Point3::new(10.0, 20.0, 30.0),
        Point3::new(10.0, 22.0, 30.0),
        Point3::new(8.0, 20.0, 30.0),
    ] {
        assert!(
            decoded.geometry.points.iter().any(|point| {
                Point3::distance(point.position().get(), expected) < EPS_COORDINATE
            }),
            "{expected:?}: {:?}",
            decoded.geometry.points
        );
    }
}

#[test]
fn a_scaled_body_is_omitted_while_other_records_are_written() {
    let mut ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    ir.geometry.bodies[0].transform = Some(
        cadmpeg_ir::transform::Transform::affine([
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 2.0, 0.0, 0.0],
            [0.0, 0.0, 2.0, 0.0],
        ])
        .expect("finite rows"),
    );
    free_point(&mut ir, "kept", Point3::new(5.0, 6.0, 7.0));
    let body = ir.geometry.bodies[0].id.as_str().to_owned();

    let (decoded, losses) = written(&ir);
    assert!(decoded.geometry.faces.is_empty());
    assert_eq!(decoded.geometry.points.len(), 1);
    assert!(
        has_loss(
            &losses,
            &RhinoLossCode::WriterRecordOmitted.kind().to_string(),
            &body
        ),
        "{losses:?}"
    );
}

#[test]
fn a_face_name_is_written_without_and_named() {
    let mut ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    ir.geometry.faces[0].name = Some("cover".into());
    let face = ir.geometry.faces[0].id.as_str().to_owned();

    let (decoded, losses) = written(&ir);
    assert_eq!(decoded.geometry.faces.len(), 1);
    assert!(
        has_loss(&losses, "export/value.unwritable", &face),
        "{losses:?}"
    );
}

#[test]
fn a_pcurve_no_coedge_uses_is_named_as_not_written() {
    let mut ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    ir.geometry
        .pcurves
        .push(cadmpeg_ir::geometry::pcurve::Pcurve {
            id: "cadir:model:pcurve#orphan"
                .try_into()
                .expect("valid identity"),
            geometry: cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(
                cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                    cadmpeg_ir::math::Point2::new(0.0, 0.0),
                    cadmpeg_ir::math::Point2::new(1.0, 0.0),
                )
                .unwrap(),
            ),
            metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(None, None, None),
            native_ref: None,
        });

    let (decoded, losses) = written(&ir);
    assert_eq!(decoded.geometry.faces.len(), 1);
    assert!(
        has_loss(
            &losses,
            &RhinoLossCode::WriterRecordOmitted.kind().to_string(),
            "cadir:model:pcurve#orphan"
        ),
        "{losses:?}"
    );
}

#[test]
fn a_free_bounded_line_round_trips() {
    let mut ir = CadIr::empty();
    ir.geometry.curves.push(Curve {
        parameter_range: cadmpeg_ir::topology::IncreasingParameterInterval::new([1.0, 3.0]),
        id: CurveId::mint("cadir:model:curve#line").expect("identity grammar"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(1.0, 2.0, 3.0),
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
        native_ref: None,
    });

    let (decoded, losses) = written(&ir);
    assert_eq!(decoded.geometry.curves.len(), 1, "{losses:?}");
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) =
        &decoded.geometry.curves[0].geometry
    else {
        panic!("a 3DM bounded line reads back as a degree-1 NURBS curve");
    };
    assert_eq!(nurbs.knots().as_slice(), [1.0, 1.0, 3.0, 3.0]);
    let poles = nurbs.control_points();
    assert_eq!(poles.len(), 2);
    for (pole, expected) in poles
        .iter()
        .zip([Point3::new(2.0, 2.0, 3.0), Point3::new(4.0, 2.0, 3.0)])
    {
        assert!(
            Point3::distance(pole.get(), expected) < EPS_COORDINATE,
            "{pole:?}"
        );
    }
}

#[test]
fn a_free_polyline_is_written_as_a_degree_one_nurbs_and_its_deflection_named() {
    let points = vec![
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(1.0, 1.0, 0.0),
    ];
    let mut ir = CadIr::empty();
    ir.geometry.curves.push(Curve {
        parameter_range: None,
        id: CurveId::mint("cadir:model:curve#polyline").expect("identity grammar"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Polyline(
            cadmpeg_ir::geometry::sampled::PolylineCurve::new(
                cadmpeg_ir::geometry::sampled::PolylineSamples::Unparameterized {
                    points: points
                        .clone()
                        .try_into()
                        .expect("nonempty polyline samples"),
                },
                0.25,
            )
            .expect("valid polyline"),
        )),
        source_object: None,
        native_ref: None,
    });

    let (decoded, losses) = written(&ir);
    assert_eq!(decoded.geometry.curves.len(), 1, "{losses:?}");
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) =
        &decoded.geometry.curves[0].geometry
    else {
        panic!("the polyline reads back as NURBS");
    };
    assert_eq!(nurbs.degree(), 1);
    let poles = nurbs.control_points();
    assert_eq!(poles.len(), points.len());
    for (pole, expected) in poles.iter().zip(&points) {
        assert!(Point3::distance(pole.get(), *expected) < EPS_COORDINATE);
    }
    assert!(
        has_loss(
            &losses,
            "export/value.unwritable",
            "cadir:model:curve#polyline"
        ),
        "{losses:?}"
    );
}

#[test]
fn free_nurbs_with_unclamped_knots_is_written_with_its_interior_knots() {
    let mut ir = CadIr::empty();
    ir.geometry.curves.push(Curve {
        parameter_range: None,
        id: CurveId::mint("cadir:model:curve#unclamped").expect("identity grammar"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
                2,
                vec![0.0, 0.5, 1.0, 3.0, 4.0, 4.5],
                vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1.0, 1.0, 0.0),
                    Point3::new(2.0, 0.0, 0.0),
                ],
                None,
                false,
            )
            .expect("valid unclamped curve"),
        )),
        source_object: None,
        native_ref: None,
    });

    let (decoded, losses) = written(&ir);
    assert_eq!(decoded.geometry.curves.len(), 1, "{losses:?}");
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) =
        &decoded.geometry.curves[0].geometry
    else {
        panic!("the curve reads back as NURBS");
    };
    let knots = nurbs.knots().as_slice();
    assert_eq!(knots[1..knots.len() - 1], [0.5, 1.0, 3.0, 4.0]);
}

#[test]
fn a_free_vertex_with_a_tolerance_is_written_as_a_point_and_named() {
    use cadmpeg_ir::topology::{Body, BodyKind, Region, Vertex};
    let mut ir = CadIr::empty();
    let body = cadmpeg_ir::ids::BodyId::mint("cadir:model:body#acorn").expect("identity grammar");
    let region =
        cadmpeg_ir::ids::RegionId::mint("cadir:model:region#acorn").expect("identity grammar");
    let shell =
        cadmpeg_ir::ids::ShellId::mint("cadir:model:shell#acorn").expect("identity grammar");
    let vertex =
        cadmpeg_ir::ids::VertexId::mint("cadir:model:vertex#acorn").expect("identity grammar");
    free_point(&mut ir, "acorn", Point3::new(4.0, 5.0, 6.0));
    ir.geometry.bodies.push(Body {
        id: body.clone(),
        kind: BodyKind::General,
        transform: None,
        name: None,
        color: None,
        visible: None,
        native_ref: None,
        unstated: None,
    });
    ir.geometry
        .regions
        .push(Region::new(region.clone(), body, vec![shell.clone()]));
    ir.geometry
        .shells
        .push(Shell::with_free_vertex(shell, region, vertex.clone()));
    ir.geometry.vertices.push(Vertex {
        id: vertex,
        point: ir.geometry.points[0].id.clone(),
        tolerance: Some(
            cadmpeg_ir::scalar::PositiveLength::new(0.125).expect("positive finite tolerance"),
        ),
        native_ref: None,
    });

    let (decoded, losses) = written(&ir);
    assert!(
        decoded.geometry.points.iter().any(|point| {
            Point3::distance(point.position().get(), Point3::new(4.0, 5.0, 6.0)) < EPS_COORDINATE
        }),
        "{:?}",
        decoded.geometry.points
    );
    assert!(
        has_loss(
            &losses,
            "export/value.unwritable",
            "cadir:model:vertex#acorn"
        ),
        "{losses:?}"
    );
}
