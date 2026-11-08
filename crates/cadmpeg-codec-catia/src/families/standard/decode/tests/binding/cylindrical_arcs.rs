// SPDX-License-Identifier: Apache-2.0

use std::f64::consts::{PI, TAU};

use cadmpeg_ir::geometry::analytic::{CylinderSurface, PlaneSurface};
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeLength, PositiveLength};

use crate::families::standard::decode::EmitStandardTopologyInputs;
use crate::families::standard::topology::{
    admitted::StandardTopology, BoundaryDraft, CoedgeUse, EdgeBoundaryLayout, EdgeRow,
    FaceTopologyDraft, StandardTopologyDraft,
};

use super::*;

fn circle_support(pos: usize, face: usize, z: f64) -> StandardCurveSupport {
    StandardCurveSupport {
        pos,
        tag: u32::try_from(pos).expect("test row tag"),
        faces: [face, face],
        geometry: StandardCurveGeometry::Circle {
            center: FinitePoint3::new(Point3::new(0.0, 0.0, z)).expect("finite center"),
            radius: PositiveLength::new(2.0).expect("positive radius"),
        },
    }
}

fn half_bounds(y: f64, z: f64) -> StandardFaceBounds {
    StandardFaceBounds {
        aabb_center: [0.0, y, z].map(|v| FiniteReal::new(v).expect("finite center")),
        aabb_half_extents: [2.1, 1.3, 3.1]
            .map(|v| NonNegativeLength::new(v).expect("nonnegative extent")),
        sphere_center: [FiniteReal::ZERO; 3],
        sphere_radius: NonNegativeLength::new(4.0).expect("nonnegative radius"),
    }
}

fn strip(boundary: [(usize, bool, usize, usize); 4]) -> FaceTopologyDraft {
    let coedges = boundary
        .map(|(edge_row, reversed, start_vertex, end_vertex)| CoedgeUse {
            edge_row,
            reversed,
            start_vertex,
            end_vertex,
        })
        .to_vec();
    FaceTopologyDraft {
        boundaries: vec![BoundaryDraft::new(coedges).expect("closed strip")],
    }
}

fn emit_strip(tilt: f64, [first_strip, second_strip]: [bool; 2]) -> CadIr {
    let points = [
        [2.0, 0.0, 0.0],
        [-2.0, 0.0, 0.0],
        [2.0 * tilt.cos(), 2.0 * tilt.sin(), 3.0],
        [-2.0 * tilt.cos(), -2.0 * tilt.sin(), 3.0],
        [2.0, 0.0, -3.0],
        [-2.0, 0.0, -3.0],
    ];
    let mut ir = CadIr::empty();
    ir.model.points = points
        .iter()
        .enumerate()
        .map(|(i, &[x, y, z])| {
            Point::new(
                standard_id("point", format_args!("{i}"), PointId::mint).expect("point ID"),
                FinitePoint3::new(Point3::new(x, y, z)).expect("finite point"),
                None,
            )
        })
        .collect();
    let origin = Point3::new(0.0, 0.0, 0.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);
    let cylinder = CylinderSurface::try_new(origin, axis, reference, 2.0).expect("cylinder");
    let plane = PlaneSurface::try_new(origin, axis, reference).expect("plane");
    let mut bindings = Vec::new();
    let mut surface_indices = HashMap::new();
    for (i, geometry) in [
        SolvedSurfaceGeometry::Cylinder(cylinder),
        SolvedSurfaceGeometry::Plane(plane),
        SolvedSurfaceGeometry::Cylinder(cylinder),
    ]
    .into_iter()
    .enumerate()
    {
        let id = standard_id("surf", format_args!("{i}"), SurfaceId::mint).expect("surface ID");
        bindings.push((id.clone(), false, 0));
        surface_indices.insert(id.clone(), i);
        ir.model.surfaces.push(Surface {
            id: id.clone(),
            geometry: SurfaceGeometry::Solved(geometry),
            source_object: None,
        });
        ir.model.faces.push(Face {
            id: standard_id("face", format_args!("{i}"), FaceId::mint).expect("face ID"),
            shell: ShellId::mint("catia:test:shell#strip").expect("shell ID"),
            surface: id,
            sense: Sense::Forward,
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
            name: None,
            color: None,
            tolerance: None,
        });
    }
    let line = |pos, face| StandardCurveSupport {
        pos,
        tag: u32::try_from(pos).expect("test row tag"),
        faces: [face, face],
        geometry: StandardCurveGeometry::Line,
    };
    let supports = [
        StandardCurveSupport {
            pos: 1,
            tag: 1,
            faces: [0, 1],
            geometry: StandardCurveGeometry::Bspline,
        },
        line(2, 0),
        circle_support(3, 0, 3.0),
        line(4, 0),
        line(5, 2),
        circle_support(6, 2, -3.0),
        line(7, 2),
    ];
    let edges = [[0, 1], [1, 3], [2, 3], [2, 0], [0, 4], [4, 5], [5, 1]];
    let topology = StandardTopology::new(StandardTopologyDraft {
        faces: vec![
            if first_strip {
                strip([
                    (0, false, 0, 1),
                    (1, false, 1, 3),
                    (2, true, 3, 2),
                    (3, false, 2, 0),
                ])
            } else {
                FaceTopologyDraft {
                    boundaries: Vec::new(),
                }
            },
            FaceTopologyDraft {
                boundaries: Vec::new(),
            },
            if second_strip {
                strip([
                    (0, true, 1, 0),
                    (4, false, 0, 4),
                    (5, false, 4, 5),
                    (6, false, 5, 1),
                ])
            } else {
                FaceTopologyDraft {
                    boundaries: Vec::new(),
                }
            },
        ],
        edge_rows: edges
            .iter()
            .map(|&[a, b]| {
                EdgeRow::new(1, vec![a, b], EdgeBoundaryLayout::CompleteBoundaryRun)
                    .expect("valid row")
            })
            .collect(),
        vertex_points: points.to_vec(),
        logical_vertex_count: points.len(),
    })
    .expect("admitted strip topology");
    let mut annotations = AnnotationBuilder::new();
    crate::test_support::with_service_context(|ctx| {
        emit_standard_topology(
            ctx,
            EmitStandardTopologyInputs {
                ir: &mut ir,
                annotations: &mut annotations,
                bindings: &bindings,
                brep: &[],
                face_bounds: &[
                    Some(half_bounds(-0.9, 1.5)),
                    None,
                    Some(half_bounds(0.9, -1.5)),
                ],
                surface_indices: &surface_indices,
                supports: &supports,
                edge_vertices: &edges
                    .map(|pair| pair.map(|vertex| usize::try_from(vertex).expect("vertex index"))),
                point_assignment: &[0, 1, 2, 3, 4, 5],
                topology: &topology,
                native_edge_supports: &[None; 7],
                limit_curve_bindings: &[None; 7],
                limit_curves: &[],
                refusal: &mut crate::nurbs::LaneRefusals::new(),
                admission: &mut crate::families::FamilyEntityAdmission::new(ctx),
            },
        )
    })
    .expect("emit strip edges and coedges");
    ir
}

#[test]
fn cylindrical_arc_recovery_reverses_the_edge_and_its_occurrence() {
    let ir = emit_strip(0.0, [true, false]);
    let edge = &ir.model.edges[0];
    assert_eq!(edge.param_range().expect("recovered arc").get(), [PI, TAU]);
    assert_eq!(edge.start.as_str(), "catia:standard:v#1");
    assert_eq!(edge.end.as_str(), "catia:standard:v#0");
    assert_eq!(ir.model.coedges[0].sense, Sense::Reversed);
}

#[test]
fn cylindrical_arc_recovery_rejects_a_tilted_generator() {
    let ir = emit_strip(0.1, [true, false]);
    assert!(ir.model.edges[2].param_range().is_some());
    assert!(ir.model.edges[0].param_range().is_none());
}

#[test]
fn cylindrical_arc_recovery_rejects_conflicting_opposite_arcs() {
    let opposite = emit_strip(0.0, [false, true]);
    assert_eq!(
        opposite.model.edges[0]
            .param_range()
            .expect("opposite strip range")
            .get(),
        [0.0, PI]
    );
    let ir = emit_strip(0.0, [true, true]);
    assert!(ir.model.edges[2].param_range().is_some());
    assert!(ir.model.edges[5].param_range().is_some());
    assert!(ir.model.edges[0].param_range().is_none());
}
