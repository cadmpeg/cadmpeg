// SPDX-License-Identifier: Apache-2.0
//! Sampled procedural carrier tests.

#![allow(clippy::unwrap_used)]

use std::f64::consts::TAU;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::eval::{admission::EvaluationAdmission, model_surface_point_by_id};
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{CircleCurve, LineCurve};
use cadmpeg_ir::geometry::pcurve::{LinePcurve, Pcurve, PcurveGeometry};
use cadmpeg_ir::geometry::procedural_curves::DirectedParameterRange;
use cadmpeg_ir::geometry::procedural_surfaces::ProceduralSurfaceDefinition;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, ProceduralSurface, SolvedCurveGeometry, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    CoedgeId, CurveId, EdgeId, PcurveId, PointId, ProceduralSurfaceId, VertexId,
};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::topology::{
    BodyKind, Coedge, Edge, EdgeCarrier, IncreasingParameterInterval, LoopBoundary, LoopRing,
    PcurveUse, Point, Sense, Vertex,
};
use cadmpeg_ir::units::UnitVector3;
use cadmpeg_ir::CadIr;

use crate::export::write_step;
use crate::loss::StepLossCode;
use crate::{StepCodec, StepSchema, StepWriteOptions};
use cadmpeg_ir::codec::write::approximation::Approximator;

const BOTTOM_RADIUS: f64 = 10.0;
const TOP_RADIUS: f64 = 5.0;
const HEIGHT: f64 = 10.0;

fn circle(z: f64, radius: f64) -> SolvedCurveGeometry {
    SolvedCurveGeometry::Circle(
        CircleCurve::try_new(
            Point3::new(0.0, 0.0, z),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            radius,
        )
        .unwrap(),
    )
}

/// A ruled face between two circles: both rims, and the line seam that both
/// sides of the face share. `top_v` is the `v` of the top-rim trim pcurve;
/// any value but one puts that trim off its edge.
fn ruled(top_v: f64) -> CadIr {
    let mut ir = super::reports::edgeless_doc();
    ir.geometry.bodies[0].kind = BodyKind::Sheet;
    ir.geometry.points.clear();
    ir.geometry.vertices.clear();
    ir.geometry.edges.clear();
    ir.geometry.coedges.clear();
    let id = |kind: &str, i: usize| format!("test:model:{kind}#ruled{i}");
    let bottom = Point3::new(BOTTOM_RADIUS, 0.0, 0.0);
    let top = Point3::new(TOP_RADIUS, 0.0, HEIGHT);
    let vertices: [VertexId; 2] = std::array::from_fn(|i| VertexId::mint(id("vertex", i)).unwrap());
    for (i, position) in [bottom, top].into_iter().enumerate() {
        let point = PointId::mint(id("point", i)).unwrap();
        ir.geometry.points.push(Point::new(
            point.clone(),
            FinitePoint3::new(position).unwrap(),
            None,
        ));
        ir.geometry.vertices.push(Vertex {
            id: vertices[i].clone(),
            point,
            tolerance: None,
            native_ref: None,
        });
    }
    let seam_length = bottom.distance(top);
    let curves: [CurveId; 3] = std::array::from_fn(|i| CurveId::mint(id("curve", i)).unwrap());
    let geometries = [
        circle(0.0, BOTTOM_RADIUS),
        circle(HEIGHT, TOP_RADIUS),
        SolvedCurveGeometry::Line(LineCurve::new(
            FinitePoint3::new(bottom).unwrap(),
            UnitVector3::normalized(Vector3::new(TOP_RADIUS - BOTTOM_RADIUS, 0.0, HEIGHT)).unwrap(),
        )),
    ];
    for (i, geometry) in geometries.into_iter().enumerate() {
        ir.geometry.curves.push(Curve {
            id: curves[i].clone(),
            geometry: CurveGeometry::Solved(geometry),
            parameter_range: (i < 2)
                .then(|| IncreasingParameterInterval::new([0.0, TAU]))
                .flatten(),
            source_object: None,
            native_ref: None,
        });
    }
    let edges: [EdgeId; 3] = std::array::from_fn(|i| EdgeId::mint(id("edge", i)).unwrap());
    for (i, (start, end, range)) in [
        (0, 0, [0.0, TAU]),
        (1, 1, [0.0, TAU]),
        (0, 1, [0.0, seam_length]),
    ]
    .into_iter()
    .enumerate()
    {
        ir.geometry.edges.push(Edge {
            id: edges[i].clone(),
            carrier: EdgeCarrier::new(Some(curves[i].clone()), Some(range)).unwrap(),
            ends: Some(cadmpeg_ir::topology::EdgeEnds::new(
                vertices[start].clone(),
                vertices[end].clone(),
            )),
            tolerance: None,
            native_ref: None,
        });
    }
    let loop_id = ir.geometry.loops[0].id.clone();
    let seam_step = 1.0 / seam_length;
    let uses = [
        (0, Sense::Forward, [0.0, 0.0], [1.0, 0.0], [0.0, TAU]),
        (
            2,
            Sense::Forward,
            [TAU, 0.0],
            [0.0, seam_step],
            [0.0, seam_length],
        ),
        (1, Sense::Reversed, [0.0, top_v], [1.0, 0.0], [TAU, 0.0]),
        (
            2,
            Sense::Reversed,
            [0.0, 0.0],
            [0.0, seam_step],
            [seam_length, 0.0],
        ),
    ];
    let coedges: [CoedgeId; 4] = std::array::from_fn(|i| CoedgeId::mint(id("coedge", i)).unwrap());
    for (i, (edge, sense, origin, direction, range)) in uses.into_iter().enumerate() {
        let pcurve = PcurveId::mint(id("pcurve", i)).unwrap();
        ir.geometry.pcurves.push(Pcurve {
            id: pcurve.clone(),
            geometry: PcurveGeometry::Line(
                LinePcurve::try_new(
                    Point2::new(origin[0], origin[1]),
                    Point2::new(direction[0], direction[1]),
                )
                .unwrap(),
            ),
            metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::default(),
            native_ref: None,
        });
        let radial_next = match i {
            1 => 3,
            3 => 1,
            _ => i,
        };
        ir.geometry.coedges.push(Coedge {
            id: coedges[i].clone(),
            owner_loop: loop_id.clone(),
            edge: edges[edge].clone(),
            radial_next: coedges[radial_next].clone(),
            sense,
            pcurves: vec![PcurveUse {
                pcurve,
                isoparametric: Some(true),
                parameter_range: Some(DirectedParameterRange::new(range).unwrap()),
            }],
            use_curve: None,
            native_ref: None,
        });
    }
    ir.geometry.loops[0].boundary =
        LoopBoundary::Ring(LoopRing::new(coedges.to_vec(), Vec::new()).unwrap());
    let surface = ir.geometry.surfaces[0].id.clone();
    let construction = ProceduralSurfaceId::mint("test:model:construction#ruled").unwrap();
    ir.geometry.surfaces[0].geometry = SurfaceGeometry::Procedural {
        construction: construction.clone(),
        cache: None,
    };
    ir.geometry
        .add_procedural_surface(
            &surface,
            ProceduralSurface::new(
                construction,
                ProceduralSurfaceDefinition::Ruled {
                    first: curves[0].clone(),
                    second: curves[1].clone(),
                    cache: None,
                },
                None,
            ),
        )
        .unwrap();
    ir
}

fn written(ir: &CadIr) -> (String, Vec<cadmpeg_ir::report::loss::LossNote>) {
    let mut bytes = Vec::new();
    let report = write_step(
        ir,
        &mut bytes,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();
    (String::from_utf8(bytes).unwrap(), report.losses)
}

#[test]
fn ruled_surface_is_written_as_a_trimmed_sampled_bspline_face() {
    let ir = ruled(1.0);
    let (text, losses) = written(&ir);
    assert!(text.contains("B_SPLINE_SURFACE_WITH_KNOTS"));
    assert!(text.contains("ADVANCED_FACE"));
    assert!(text.contains("PCURVE"));
    assert!(!losses
        .iter()
        .any(|loss| loss.message.contains("has no writable topology")));
    let sampled: Vec<_> = losses
        .iter()
        .filter(|loss| loss.code == StepLossCode::ProceduralSampledToBspline.kind())
        .collect();
    assert_eq!(sampled.len(), 1);
    assert!(sampled[0]
        .message
        .starts_with("0 procedural curve definition(s) and 1 procedural surface definition(s)"));
    assert!(!losses.iter().any(|loss| {
        loss.code == StepLossCode::ProceduralReducedToCarrier.kind()
            || loss.code == StepLossCode::ProceduralDefinitionNotWritten.kind()
    }));
    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(text.into_bytes()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(decoded.ir().geometry.faces.len(), 1);
    assert_eq!(decoded.ir().geometry.edges.len(), 3);
}

#[test]
fn sampled_ruled_surface_stays_within_the_residual_limit() {
    let ir = ruled(1.0);
    let surface_id = ir.geometry.surfaces[0].id.clone();
    let mut approximator = Approximator::new(&ir);
    let limit = approximator.residual_limit();
    let carrier = approximator.surface(surface_id.as_str()).unwrap();
    let exact_index = ModelIndex::new_model_only(&ir);
    let mut sampled = ir.clone();
    sampled.geometry.procedural_surfaces.clear();
    sampled.geometry.surfaces[0].geometry = SurfaceGeometry::Solved(carrier);
    let index = ModelIndex::new_model_only(&sampled);
    for i in 0..=40 {
        for j in 0..=40 {
            let u = (TAU * f64::from(i) / 40.0 + 0.013).min(TAU);
            let v = (f64::from(j) / 40.0 + 0.007).min(1.0);
            let point = |index: &ModelIndex<'_>| {
                model_surface_point_by_id(EvaluationAdmission::Standard, index, &surface_id, u, v)
                    .unwrap()
                    .get()
            };
            let (exact, fit) = (point(&exact_index), point(&index));
            assert!(
                exact.distance(fit) <= limit,
                "({u}, {v}) misses by {}",
                exact.distance(fit)
            );
        }
    }
}

#[test]
fn surface_with_a_trim_off_its_edge_keeps_the_omission_report() {
    let ir = ruled(0.5);
    let surface_id = ir.geometry.surfaces[0].id.clone();
    let mut approximator = Approximator::new(&ir);
    assert!(approximator.surface(surface_id.as_str()).is_none());
    assert!(approximator.surface(surface_id.as_str()).is_none());
    let (text, losses) = written(&ir);
    assert!(!text.contains("ADVANCED_FACE"));
    assert!(losses
        .iter()
        .any(|loss| loss.message.contains("has no writable topology")));
    assert!(!losses
        .iter()
        .any(|loss| loss.code == StepLossCode::ProceduralSampledToBspline.kind()));
    assert!(losses.iter().any(|loss| {
        loss.code == StepLossCode::ProceduralDefinitionNotWritten.kind()
            && loss
                .message
                .contains("had no emitted STEP construction or solved carrier")
    }));
}

/// A cube whose first edge lies on the intersection of a unit cylinder about the Z axis and the
/// bicubic spline surface `z = 0.3 x y`. The curve's coarse degree-one chart samples the
/// intersection. The linear tolerance is `1e-5`.
fn cylinder_on_spline_edge() -> CadIr {
    use cadmpeg_ir::geometry::analytic::CylinderSurface;
    use cadmpeg_ir::geometry::intersection_curves::{IntcurveSupportContext, IntcurveSupportSide};
    use cadmpeg_ir::geometry::nurbs::{
        NurbsCurve, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes,
    };
    use cadmpeg_ir::geometry::procedural_curves::ProceduralCurveDefinition;
    use cadmpeg_ir::geometry::{ProceduralCurve, SolvedSurfaceGeometry, Surface};
    use cadmpeg_ir::ids::{ProceduralCurveId, SurfaceId};

    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    ir.tolerances.linear = cadmpeg_ir::scalar::PositiveLength::new(1.0e-5).unwrap();
    let grid = [-2.0, -2.0 / 3.0, 2.0 / 3.0, 2.0];
    let spline = NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(3, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(3, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            grid.iter()
                .map(|&x| {
                    grid.iter()
                        .map(|&y| Point3::new(x, y, 0.3 * x * y))
                        .collect()
                })
                .collect(),
            None,
        ),
        false,
    )
    .unwrap();
    let cylinder = CylinderSurface::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        1.0,
    )
    .unwrap();
    let mut add = |name: &str, geometry: SolvedSurfaceGeometry| {
        let id = SurfaceId::mint(name).unwrap();
        ir.geometry.surfaces.push(Surface {
            id: id.clone(),
            geometry: SurfaceGeometry::Solved(geometry),
            source_object: None,
            native_ref: None,
        });
        id
    };
    let cylinder = add(
        "test:model:surface#fit-cylinder",
        SolvedSurfaceGeometry::Cylinder(cylinder),
    );
    let spline = add(
        "test:model:surface#fit-spline",
        SolvedSurfaceGeometry::Nurbs(spline),
    );
    let segments = 8_u32;
    let on_both = |i: u32| {
        let t = 0.3 + 1.6 * f64::from(i) / f64::from(segments);
        Point3::new(t.cos(), t.sin(), 0.15 * (2.0 * t).sin())
    };
    let mut knots = vec![0.0];
    knots.extend((0..=segments).map(f64::from));
    knots.push(f64::from(segments));
    let chart =
        NurbsCurve::from_lanes(1, knots, (0..=segments).map(on_both).collect(), None, false)
            .unwrap();
    let curve = CurveId::mint("test:model:curve#fit-intersection").unwrap();
    let construction = ProceduralCurveId::mint("test:model:procedural-curve#fit").unwrap();
    let side = |surface| IntcurveSupportSide {
        surface: Some(surface),
        pcurve: None,
    };
    let context = IntcurveSupportContext::try_new(
        [side(cylinder), side(spline)],
        [0.0, f64::from(segments)],
        [Vec::new(), Vec::new(), Vec::new()],
    )
    .unwrap();
    ir.geometry.procedural_curves.push(ProceduralCurve::new(
        construction.clone(),
        ProceduralCurveDefinition::Intersection {
            context,
            discontinuity_flag: false,
            cache: None,
        },
    ));
    ir.geometry.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Procedural {
            construction,
            cache: Some(SolvedCurveGeometry::Nurbs(chart)),
        },
        parameter_range: None,
        source_object: None,
        native_ref: None,
    });
    let edge = &mut ir.geometry.edges[0];
    edge.set_curve(Some(curve)).unwrap();
    edge.set_param_range(Some(
        cadmpeg_ir::topology::ParameterInterval::new([0.0, f64::from(segments)]).unwrap(),
    ));
    let ends: Vec<_> = edge.vertices().cloned().collect();
    for (vertex, point) in ends.iter().zip([on_both(0), on_both(segments)]) {
        let point_id = ir
            .geometry
            .vertices
            .iter()
            .find(|candidate| candidate.id == *vertex)
            .unwrap()
            .point
            .clone();
        ir.geometry
            .points
            .iter_mut()
            .find(|candidate| candidate.id == point_id)
            .unwrap()
            .set_position(FinitePoint3::new(point).unwrap());
    }
    ir
}

#[test]
fn a_cylinder_spline_intersection_edge_is_written_within_the_document_tolerance() {
    let ir = cylinder_on_spline_edge();
    let (text, losses) = written(&ir);
    assert!(text.contains("B_SPLINE_CURVE_WITH_KNOTS"));
    // Every fit reaches the strict limit, so no curve is reported as sampled at a looser one.
    assert!(!losses
        .iter()
        .any(|loss| loss.message.contains("lie on free-form surfaces")));
    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(text.into_bytes()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let decoded = decoded.ir();
    let mut checked = 0;
    for curve in &decoded.geometry.curves {
        let Some(SolvedCurveGeometry::Nurbs(spline)) = curve.geometry.solved() else {
            continue;
        };
        if spline.degree() != 3 {
            continue;
        }
        let domain = cadmpeg_ir::eval::nurbs_curve_parameter_domain(spline)
            .map(IncreasingParameterInterval::endpoints)
            .unwrap();
        for i in 0..=2000 {
            let u = domain[0] + (domain[1] - domain[0]) * f64::from(i) / 2000.0;
            let p = cadmpeg_ir::eval::decode::nurbs_curve_point_at(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                spline,
                u,
            )
            .unwrap()
            .get();
            let off_cylinder = (p.x.hypot(p.y) - 1.0).abs();
            let off_spline = (p.z - 0.3 * p.x * p.y).abs();
            assert!(off_cylinder <= 1.0e-5, "{off_cylinder}");
            assert!(off_spline <= 1.0e-5, "{off_spline}");
        }
        checked += 1;
    }
    assert_eq!(checked, 1);
}
