// SPDX-License-Identifier: Apache-2.0
//! Edge forms a 3DM Brep holds exactly: a singular edge as a singular trim,
//! a compound edge curve as its joined NURBS form. A placed tolerance that
//! leaves the finite range, and a face past a writer limit, are named in a
//! loss (§3.4).

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::decode::nurbs_curve_point_at;
use cadmpeg_ir::geometry::analytic::HyperbolaCurve;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::procedural_curves::{
    CompoundComponent, CompoundCurveConstruction, ProceduralCurveDefinition,
};
use cadmpeg_ir::geometry::{Curve, CurveGeometry, ProceduralCurve, SolvedCurveGeometry};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::PositiveLength;
use cadmpeg_ir::topology::EdgeCarrier;

use super::{polygon_sheet, rectangular_nurbs_patch, written};
use crate::loss::RhinoLossCode;
use crate::test_support::test_archive::{
    archive, object_record, singular_seam_brep_payload, BREP_CLASS,
};
use crate::RhinoCodec;

/// Distance within which a decoded coordinate equals the written one.
const EPS_COORDINATE: f64 = 1e-12;

fn omitted<'a>(losses: &'a [String], id: &str) -> Vec<&'a String> {
    let code = RhinoLossCode::WriterRecordOmitted.kind().to_string();
    losses
        .iter()
        .filter(|loss| loss.starts_with(&code) && loss.contains(id))
        .collect()
}

fn curve(id: &str, geometry: SolvedCurveGeometry) -> Curve {
    Curve {
        parameter_range: None,
        id: CurveId::mint(id).expect("identity grammar"),
        geometry: CurveGeometry::Solved(geometry),
        source_object: None,
        native_ref: None,
    }
}

fn nurbs(degree: u32, knots: Vec<f64>, points: Vec<Point3>) -> SolvedCurveGeometry {
    SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(degree, knots, points, None, false).expect("valid fixture curve"),
    )
}

#[test]
fn a_singular_edge_is_written_as_a_singular_trim() {
    let source = RhinoCodec
        .decode(
            &mut Cursor::new(archive(&[object_record(
                0x10,
                BREP_CLASS,
                &singular_seam_brep_payload(false),
            )])),
            &DecodeOptions::default(),
        )
        .expect("the fixture decodes");
    let singular = |ir: &cadmpeg_ir::document::CadIr| {
        ir.geometry
            .edges
            .iter()
            .filter(|edge| {
                edge.curve().is_none()
                    && edge
                        .ends
                        .as_ref()
                        .is_some_and(|ends| ends.start == ends.end)
            })
            .count()
    };
    assert_eq!(singular(source.ir()), 2);

    let (decoded, losses) = written(source.ir());
    assert_eq!(decoded.geometry.faces.len(), 1, "{losses:?}");
    assert_eq!(decoded.geometry.coedges.len(), 4, "{losses:?}");
    assert_eq!(singular(&decoded), 2, "{:?}", decoded.geometry.edges);
    assert_eq!(decoded.geometry.edges.len(), 3);
    assert!(
        losses.iter().all(|loss| !loss.contains("has no curve")),
        "{losses:?}"
    );
}

/// A square sheet whose first edge runs from (0, 0) to (4, 0) over [0, 4]
/// along a compound whose second component starts at `second_start`.
fn compound_edge_sheet(second_start: Point3) -> cadmpeg_ir::CadIr {
    let mut ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(4.0, 0.0, 0.0),
        Point3::new(4.0, 4.0, 0.0),
        Point3::new(0.0, 4.0, 0.0),
    ]);
    // The first edge runs from (0, 0) to (4, 0) over [0, 4] along a compound:
    // a line over the compound's [0, 1.5] on its own domain [10, 12], then a
    // quadratic over [1.5, 4] on its own domain [0, 1].
    let first = curve(
        "cadir:model:curve#compound.0",
        nurbs(
            1,
            vec![10.0, 10.0, 12.0, 12.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
        ),
    );
    let second = curve(
        "cadir:model:curve#compound.1",
        nurbs(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![
                second_start,
                Point3::new(2.5, 2.0, 0.0),
                Point3::new(4.0, 0.0, 0.0),
            ],
        ),
    );
    set_compound_edge_curve(&mut ir, [0.0, 1.5, 4.0], first, second);
    ir
}

/// Makes the curve of the first edge of `ir` a compound of `first` over
/// `[parameters[0], parameters[1]]` and `second` over
/// `[parameters[1], parameters[2]]`.
fn set_compound_edge_curve(
    ir: &mut cadmpeg_ir::CadIr,
    parameters: [f64; 3],
    first: Curve,
    second: Curve,
) {
    let construction =
        ProceduralCurveId::mint("cadir:model:procedural-curve#compound").expect("identity grammar");
    ir.geometry.procedural_curves.push(ProceduralCurve::new(
        construction.clone(),
        ProceduralCurveDefinition::Compound(
            CompoundCurveConstruction::try_new(
                parameters.to_vec(),
                vec![
                    CompoundComponent {
                        parameter: parameters[0],
                        component: first.id.clone(),
                    },
                    CompoundComponent {
                        parameter: parameters[1],
                        component: second.id.clone(),
                    },
                ],
                None,
            )
            .expect("valid compound"),
        ),
    ));
    let edge_curve = ir.geometry.edges[0].curve().cloned().expect("edge curve");
    let record = ir
        .geometry
        .curves
        .iter_mut()
        .find(|record| record.id == edge_curve)
        .expect("the edge's curve");
    record.geometry = CurveGeometry::Procedural {
        construction,
        cache: Some(SolvedCurveGeometry::Unknown { record: None }),
    };
    ir.geometry.curves.extend([first, second]);
    ir.finalize();
}

#[test]
fn a_compound_edge_curve_is_written_as_its_joined_nurbs_form() {
    let ir = compound_edge_sheet(Point3::new(1.0, 1.0, 0.0));

    let (decoded, losses) = written(&ir);
    assert_eq!(decoded.geometry.faces.len(), 1, "{losses:?}");
    assert!(
        omitted(&losses, "cadir:model:face#polygon").is_empty(),
        "{losses:?}"
    );
    let written_curve = decoded
        .geometry
        .edges
        .iter()
        .filter_map(|edge| {
            let id = edge.curve()?;
            decoded.geometry.curves.iter().find(|curve| curve.id == *id)
        })
        .find_map(|curve| match &curve.geometry {
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) if nurbs.degree() == 2 => {
                Some(nurbs)
            }
            _ => None,
        })
        .expect("the compound edge is written as one quadratic NURBS curve");
    for (parameter, expected) in [
        (0.0, Point3::new(0.0, 0.0, 0.0)),
        (0.75, Point3::new(0.5, 0.5, 0.0)),
        (1.5, Point3::new(1.0, 1.0, 0.0)),
        (2.75, Point3::new(2.5, 1.25, 0.0)),
        (4.0, Point3::new(4.0, 0.0, 0.0)),
    ] {
        let actual = nurbs_curve_point_at(EvaluationAdmission::Standard, written_curve, parameter)
            .expect("the written curve has a point")
            .get();
        assert!(
            actual.distance(expected) <= EPS_COORDINATE * 10.0,
            "{parameter}: {actual:?} != {expected:?}"
        );
    }
}

#[test]
fn compound_edge_whose_end_break_is_off_by_rounding_spans_the_edge_range() {
    let mut ir = compound_edge_sheet(Point3::new(1.0, 1.0, 0.0));
    let [first, second] = [
        "cadir:model:curve#compound.0",
        "cadir:model:curve#compound.1",
    ]
    .map(|id| {
        ir.geometry
            .curves
            .iter()
            .find(|curve| curve.id.as_str() == id)
            .cloned()
            .expect("the compound component")
    });
    ir.geometry
        .procedural_curves
        .retain(|curve| curve.id.as_str() != "cadir:model:procedural-curve#compound");
    ir.geometry
        .curves
        .retain(|curve| curve.id != first.id && curve.id != second.id);
    set_compound_edge_curve(&mut ir, [0.0, 1.5, 4.0 + 4.0e-12], first, second);

    let (decoded, losses) = written(&ir);
    assert_eq!(decoded.geometry.faces.len(), 1, "{losses:?}");
    assert!(
        omitted(&losses, "cadir:model:face#polygon").is_empty(),
        "{losses:?}"
    );
    let edge = decoded
        .geometry
        .edges
        .iter()
        .find(|edge| {
            edge.curve().is_some_and(|id| {
                decoded.geometry.curves.iter().any(|curve| {
                    curve.id == *id
                        && matches!(
                            &curve.geometry,
                            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs))
                                if nurbs.degree() == 2
                        )
                })
            })
        })
        .expect("the compound edge is written as one quadratic NURBS curve");
    assert_eq!(
        edge.param_range().map(cadmpeg_ir::units::FiniteVector::get),
        Some([0.0, 4.0])
    );
}

#[test]
fn compound_edge_pieces_apart_within_the_edge_tolerance_keep_the_face() {
    let mut ir = compound_edge_sheet(Point3::new(1.0, 1.0 + 1.0e-6, 0.0));
    ir.geometry.edges[0].tolerance = Some(PositiveLength::new(1.0e-3).expect("finite tolerance"));

    let (decoded, losses) = written(&ir);
    assert_eq!(decoded.geometry.faces.len(), 1, "{losses:?}");
    assert!(
        omitted(&losses, "cadir:model:face#polygon").is_empty(),
        "{losses:?}"
    );
    let code = RhinoLossCode::WriterEdgeGapJoined.kind().to_string();
    assert!(
        losses.iter().any(|loss| loss.starts_with(&code)),
        "{losses:?}"
    );
}

#[test]
fn compound_edge_pieces_apart_beyond_the_edge_tolerance_are_written_as_an_edge_each() {
    let ir = compound_edge_sheet(Point3::new(1.0, 2.0, 0.0));

    let (decoded, losses) = written(&ir);
    assert_eq!(decoded.geometry.faces.len(), 1, "{losses:?}");
    assert!(
        omitted(&losses, "cadir:model:face#polygon").is_empty(),
        "{losses:?}"
    );
    assert!(
        !losses
            .iter()
            .any(|loss| loss.starts_with("export/record.invalid")),
        "{losses:?}"
    );
    let code = RhinoLossCode::WriterEdgeSplit.kind().to_string();
    assert!(
        losses.iter().any(|loss| loss.starts_with(&code)),
        "{losses:?}"
    );
    let geometry = &decoded.geometry;
    assert_eq!(geometry.edges.len(), ir.geometry.edges.len() + 1);
    let midpoint = Point3::new(1.0, 1.5, 0.0);
    let vertex = geometry
        .vertices
        .iter()
        .find(|vertex| {
            geometry.points.iter().any(|point| {
                point.id == vertex.point
                    && point.position().get().distance(midpoint) <= EPS_COORDINATE
            })
        })
        .expect("a vertex at the midpoint of the gap");
    assert!(
        vertex
            .tolerance
            .is_some_and(|tolerance| tolerance.get() >= 0.5),
        "{vertex:?}"
    );
    assert_eq!(
        geometry
            .edges
            .iter()
            .filter(|edge| edge.vertices().any(|id| *id == vertex.id))
            .count(),
        2
    );
}

#[test]
fn a_gapped_compound_edge_on_a_nurbs_face_keeps_a_pcurve_on_each_piece() {
    let mut ir = rectangular_nurbs_patch();
    // The first edge runs from (0, 0, 0) to (3, 0, 0) over [20, 23]; its
    // pieces leave a gap from x = 1.5 to x = 1.7 along the patch boundary.
    let line = |id: &str, from: Point3, to: Point3| {
        curve(id, nurbs(1, vec![0.0, 0.0, 1.0, 1.0], vec![from, to]))
    };
    let first = line(
        "cadir:model:curve#compound.0",
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.5, 0.0, 0.0),
    );
    let second = line(
        "cadir:model:curve#compound.1",
        Point3::new(1.7, 0.0, 0.0),
        Point3::new(3.0, 0.0, 0.0),
    );
    set_compound_edge_curve(&mut ir, [20.0, 21.5, 23.0], first, second);

    let (decoded, losses) = written(&ir);
    assert_eq!(decoded.geometry.faces.len(), 1, "{losses:?}");
    let code = RhinoLossCode::WriterEdgeSplit.kind().to_string();
    assert!(
        losses.iter().any(|loss| loss.starts_with(&code)),
        "{losses:?}"
    );
    let geometry = &decoded.geometry;
    assert_eq!(geometry.edges.len(), ir.geometry.edges.len() + 1);
    assert_eq!(geometry.coedges.len(), geometry.edges.len());
    assert!(
        geometry
            .coedges
            .iter()
            .all(|coedge| coedge.pcurves.len() == 1),
        "{:?}",
        geometry.coedges
    );
}

#[test]
fn a_placed_tolerance_past_the_finite_range_is_dropped_with_a_loss() {
    let mut ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    let wide = PositiveLength::new(1e300).expect("finite tolerance");
    ir.geometry.edges[0].tolerance = Some(wide);
    ir.geometry.vertices[0].tolerance = Some(wide);
    let edge = ir.geometry.edges[0].id.as_str().to_owned();
    let vertex = ir.geometry.vertices[0].id.as_str().to_owned();
    ir.geometry.bodies[0].transform = Some(
        cadmpeg_ir::transform::Transform::affine([
            [1e10, 0.0, 0.0, 0.0],
            [0.0, 1e10, 0.0, 0.0],
            [0.0, 0.0, 1e10, 0.0],
        ])
        .expect("finite rows"),
    );

    let (decoded, losses) = written(&ir);
    assert_eq!(decoded.geometry.faces.len(), 1, "{losses:?}");
    assert!(
        losses.iter().any(|loss| {
            loss.starts_with("export/value.unwritable")
                && loss.contains("tolerance")
                && loss.contains(&edge)
                && loss.contains(&vertex)
        }),
        "{losses:?}"
    );
}

#[test]
fn a_face_past_a_writer_limit_is_left_out_with_the_limit_named() {
    let mut ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    let edge_curve = ir.geometry.edges[0].curve().cloned().expect("edge curve");
    let record = ir
        .geometry
        .curves
        .iter_mut()
        .find(|record| record.id == edge_curve)
        .expect("the edge's curve");
    // Rational quadratic arcs of a hyperbola cover at most one unit of its
    // parameter each, so this range needs more arcs than the writer builds.
    record.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(
        HyperbolaCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
            1.0,
        )
        .expect("hyperbola fixture"),
    ));
    ir.geometry.edges[0].carrier =
        EdgeCarrier::new(Some(edge_curve), Some([0.0, 5000.0])).expect("ordered range");
    ir.finalize();

    let (decoded, losses) = written(&ir);
    assert!(decoded.geometry.faces.is_empty(), "{losses:?}");
    let face = omitted(&losses, "cadir:model:face#polygon");
    assert!(
        face.iter()
            .any(|loss| loss.contains("more arcs than the writer's limit of 4096")),
        "{losses:?}"
    );
}
