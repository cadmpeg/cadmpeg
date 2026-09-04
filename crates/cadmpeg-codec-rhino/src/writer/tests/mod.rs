// SPDX-License-Identifier: Apache-2.0
//! Writer unit tests.

use cadmpeg_ir::codec::write::TargetRequest;
use std::io::Cursor;

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::Point3;

pub(crate) use super::*;
use crate::{RhinoArchiveVersion, RhinoCodec};

mod encoding;
mod free_geometry;
mod nurbs;
mod planar;
mod targets;

pub(crate) fn assert_planar_sheet_round_trip(ir: &CadIr, loop_count: usize, edge_count: usize) {
    for version in [
        RhinoArchiveVersion::V5,
        RhinoArchiveVersion::V6,
        RhinoArchiveVersion::V7,
        RhinoArchiveVersion::V8,
    ] {
        let mut bytes = Vec::new();
        RhinoCodec
            .plan(
                cadmpeg_ir::codec::write::EncodeInput { ir, fidelity: None },
                TargetRequest::Explicit(version.descriptor().id.as_str()),
            )
            .and_then(|plan| plan.write_to(&mut bytes))
            .expect("required invariant");
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("required invariant");
        assert!(
            decoded
                .report()
                .losses
                .iter()
                .all(|loss| !loss.message.contains("Brep mesh cache degraded")),
            "{version:?}: {:?}",
            decoded.report().losses
        );
        assert_eq!(decoded.ir().model.bodies.len(), 1, "{version:?}");
        assert_eq!(
            decoded.ir().model.bodies[0].kind,
            cadmpeg_ir::topology::BodyKind::Sheet,
            "{version:?}"
        );
        assert_eq!(decoded.ir().model.faces.len(), 1, "{version:?}");
        assert_eq!(decoded.ir().model.loops.len(), loop_count, "{version:?}");
        assert_eq!(decoded.ir().model.coedges.len(), edge_count, "{version:?}");
        assert_eq!(decoded.ir().model.edges.len(), edge_count, "{version:?}");
        assert_eq!(decoded.ir().model.vertices.len(), edge_count, "{version:?}");
        for (actual, expected) in decoded.ir().model.edges.iter().zip(&ir.model.edges) {
            assert_eq!(actual.param_range, expected.param_range, "{version:?}");
        }
        assert!(
            cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).is_ok(),
            "{version:?}"
        );
    }
}

pub(crate) fn polygon_sheet(points: &[Point3]) -> CadIr {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
    use cadmpeg_ir::ids::*;
    use cadmpeg_ir::math::Vector3;
    use cadmpeg_ir::topology::*;

    let mut ir = CadIr::empty();
    let body: BodyId = "cadir:model:body#polygon".into();
    let region: RegionId = "cadir:model:region#polygon".into();
    let shell: ShellId = "cadir:model:shell#polygon".into();
    let face: FaceId = "cadir:model:face#polygon".into();
    let loop_id: LoopId = "cadir:model:loop#polygon".into();
    let surface: SurfaceId = "cadir:model:surface#polygon".into();
    let point_ids = (0..points.len())
        .map(|index| {
            PointId::mint(format!("cadir:model:point#polygon.{index}")).expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let vertex_ids = (0..points.len())
        .map(|index| {
            VertexId::mint(format!("cadir:model:vertex#polygon.{index}")).expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let edge_ids = (0..points.len())
        .map(|index| {
            EdgeId::mint(format!("cadir:model:edge#polygon.{index}")).expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let curve_ids = (0..points.len())
        .map(|index| {
            CurveId::mint(format!("cadir:model:curve#polygon.{index}")).expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let coedge_ids = (0..points.len())
        .map(|index| {
            CoedgeId::mint(format!("cadir:model:coedge#polygon.{index}")).expect("identity grammar")
        })
        .collect::<Vec<_>>();
    ir.model.bodies.push(Body {
        id: body.clone(),
        kind: BodyKind::Sheet,
        regions: vec![region.clone()],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.regions.push(Region {
        id: region.clone(),
        body,
        shells: vec![shell.clone()],
    });
    ir.model.shells.push(Shell {
        id: shell.clone(),
        region,
        faces: vec![face.clone()],
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
    ir.model.faces.push(Face {
        id: face.clone(),
        shell,
        surface: surface.clone(),
        sense: Sense::Forward,
        loops: vec![loop_id.clone()],
        name: None,
        color: None,
        tolerance: None,
    });
    ir.model.loops.push(Loop {
        id: loop_id.clone(),
        face,
        boundary_role: LoopBoundaryRole::default(),
        boundary: cadmpeg_ir::topology::LoopBoundary::Ring {
            coedges: coedge_ids.clone(),
            vertex_uses: Vec::new(),
        },
    });
    ir.model.surfaces.push(Surface {
        id: surface,
        geometry: SurfaceGeometry::Plane {
            origin: points[0],
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    for index in 0..points.len() {
        let next = points[(index + 1) % points.len()];
        let delta = Vector3::new(
            next.x - points[index].x,
            next.y - points[index].y,
            next.z - points[index].z,
        );
        let length = delta.norm();
        let direction = Vector3::new(delta.x / length, delta.y / length, delta.z / length);
        ir.model.points.push(Point {
            id: point_ids[index].clone(),
            position: points[index],
            source_object: None,
        });
        ir.model.vertices.push(Vertex {
            id: vertex_ids[index].clone(),
            point: point_ids[index].clone(),
            tolerance: None,
        });
        ir.model.curves.push(Curve {
            id: curve_ids[index].clone(),
            geometry: CurveGeometry::Line {
                origin: points[index],
                direction,
            },
            source_object: None,
        });
        ir.model.edges.push(Edge {
            id: edge_ids[index].clone(),
            curve: Some(curve_ids[index].clone()),
            start: vertex_ids[index].clone(),
            end: vertex_ids[(index + 1) % points.len()].clone(),
            param_range: Some([0.0, length]),
            tolerance: None,
        });
        ir.model.coedges.push(Coedge {
            id: coedge_ids[index].clone(),
            owner_loop: loop_id.clone(),
            edge: edge_ids[index].clone(),
            radial_next: coedge_ids[index].clone(),
            sense: Sense::Forward,
            pcurves: Vec::new(),
            use_curve: None,
        });
    }
    ir.finalize();
    ir
}

pub(crate) fn add_polygon_hole(ir: &mut CadIr, points: &[Point3]) {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry};
    use cadmpeg_ir::ids::*;
    use cadmpeg_ir::math::Vector3;
    use cadmpeg_ir::topology::*;

    let base = ir.model.edges.len();
    let face = ir.model.faces[0].id.clone();
    let loop_id = LoopId::mint(format!("cadir:model:loop#polygon.{}", ir.model.loops.len()))
        .expect("identity grammar");
    let point_ids = (0..points.len())
        .map(|index| {
            PointId::mint(format!("cadir:model:point#polygon.{}", base + index))
                .expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let vertex_ids = (0..points.len())
        .map(|index| {
            VertexId::mint(format!("cadir:model:vertex#polygon.{}", base + index))
                .expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let edge_ids = (0..points.len())
        .map(|index| {
            EdgeId::mint(format!("cadir:model:edge#polygon.{}", base + index))
                .expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let curve_ids = (0..points.len())
        .map(|index| {
            CurveId::mint(format!("cadir:model:curve#polygon.{}", base + index))
                .expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let coedge_ids = (0..points.len())
        .map(|index| {
            CoedgeId::mint(format!("cadir:model:coedge#polygon.{}", base + index))
                .expect("identity grammar")
        })
        .collect::<Vec<_>>();
    ir.model.faces[0].loops.push(loop_id.clone());
    ir.model.loops.push(Loop {
        id: loop_id.clone(),
        face,
        boundary_role: LoopBoundaryRole::default(),
        boundary: cadmpeg_ir::topology::LoopBoundary::Ring {
            coedges: coedge_ids.clone(),
            vertex_uses: Vec::new(),
        },
    });
    for index in 0..points.len() {
        let next_index = (index + 1) % points.len();
        let next = points[next_index];
        let delta = Vector3::new(
            next.x - points[index].x,
            next.y - points[index].y,
            next.z - points[index].z,
        );
        let length = delta.norm();
        ir.model.points.push(Point {
            id: point_ids[index].clone(),
            position: points[index],
            source_object: None,
        });
        ir.model.vertices.push(Vertex {
            id: vertex_ids[index].clone(),
            point: point_ids[index].clone(),
            tolerance: None,
        });
        ir.model.curves.push(Curve {
            id: curve_ids[index].clone(),
            geometry: CurveGeometry::Line {
                origin: points[index],
                direction: Vector3::new(delta.x / length, delta.y / length, delta.z / length),
            },
            source_object: None,
        });
        ir.model.edges.push(Edge {
            id: edge_ids[index].clone(),
            curve: Some(curve_ids[index].clone()),
            start: vertex_ids[index].clone(),
            end: vertex_ids[next_index].clone(),
            param_range: Some([0.0, length]),
            tolerance: None,
        });
        ir.model.coedges.push(Coedge {
            id: coedge_ids[index].clone(),
            owner_loop: loop_id.clone(),
            edge: edge_ids[index].clone(),
            radial_next: coedge_ids[index].clone(),
            sense: Sense::Forward,
            pcurves: Vec::new(),
            use_curve: None,
        });
    }
    ir.finalize();
}

pub(crate) fn adjacent_quad_sheet() -> CadIr {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
    use cadmpeg_ir::ids::*;
    use cadmpeg_ir::math::Vector3;
    use cadmpeg_ir::topology::*;

    let mut ir = CadIr::empty();
    let body: BodyId = "cadir:model:body#adjacent".into();
    let region: RegionId = "cadir:model:region#adjacent".into();
    let shell: ShellId = "cadir:model:shell#adjacent".into();
    let face_ids = [
        FaceId::mint("cadir:model:face#adjacent.0").expect("identity grammar"),
        FaceId::mint("cadir:model:face#adjacent.1").expect("identity grammar"),
    ];
    let loop_ids = [
        LoopId::mint("cadir:model:loop#adjacent.0").expect("identity grammar"),
        LoopId::mint("cadir:model:loop#adjacent.1").expect("identity grammar"),
    ];
    let surface_ids = [
        SurfaceId::mint("cadir:model:surface#adjacent.0").expect("identity grammar"),
        SurfaceId::mint("cadir:model:surface#adjacent.1").expect("identity grammar"),
    ];
    let positions = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(1.0, 1.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(2.0, 1.0, 0.0),
    ];
    let point_ids = (0..positions.len())
        .map(|index| {
            PointId::mint(format!("cadir:model:point#adjacent.{index}")).expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let vertex_ids = (0..positions.len())
        .map(|index| {
            VertexId::mint(format!("cadir:model:vertex#adjacent.{index}"))
                .expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let edge_ids = (0..7)
        .map(|index| {
            EdgeId::mint(format!("cadir:model:edge#adjacent.{index}")).expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let curve_ids = (0..7)
        .map(|index| {
            CurveId::mint(format!("cadir:model:curve#adjacent.{index}")).expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let coedge_ids = (0..8)
        .map(|index| {
            CoedgeId::mint(format!("cadir:model:coedge#adjacent.{index}"))
                .expect("identity grammar")
        })
        .collect::<Vec<_>>();
    ir.model.bodies.push(Body {
        id: body.clone(),
        kind: BodyKind::Sheet,
        regions: vec![region.clone()],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.regions.push(Region {
        id: region.clone(),
        body,
        shells: vec![shell.clone()],
    });
    ir.model.shells.push(Shell {
        id: shell.clone(),
        region,
        faces: face_ids.to_vec(),
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
    for index in 0..2 {
        ir.model.faces.push(Face {
            id: face_ids[index].clone(),
            shell: shell.clone(),
            surface: surface_ids[index].clone(),
            sense: Sense::Forward,
            loops: vec![loop_ids[index].clone()],
            name: None,
            color: None,
            tolerance: None,
        });
        ir.model.surfaces.push(Surface {
            id: surface_ids[index].clone(),
            geometry: SurfaceGeometry::Plane {
                origin: positions[0],
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        });
    }
    ir.model.loops.push(Loop {
        id: loop_ids[0].clone(),
        face: face_ids[0].clone(),
        boundary_role: LoopBoundaryRole::default(),
        boundary: cadmpeg_ir::topology::LoopBoundary::Ring {
            coedges: coedge_ids[0..4].to_vec(),
            vertex_uses: Vec::new(),
        },
    });
    ir.model.loops.push(Loop {
        id: loop_ids[1].clone(),
        face: face_ids[1].clone(),
        boundary_role: LoopBoundaryRole::default(),
        boundary: cadmpeg_ir::topology::LoopBoundary::Ring {
            coedges: coedge_ids[4..8].to_vec(),
            vertex_uses: Vec::new(),
        },
    });
    for index in 0..positions.len() {
        ir.model.points.push(Point {
            id: point_ids[index].clone(),
            position: positions[index],
            source_object: None,
        });
        ir.model.vertices.push(Vertex {
            id: vertex_ids[index].clone(),
            point: point_ids[index].clone(),
            tolerance: None,
        });
    }
    let endpoints = [(0, 1), (1, 2), (2, 3), (3, 0), (1, 4), (4, 5), (5, 2)];
    for (index, (start, end)) in endpoints.into_iter().enumerate() {
        let delta = Vector3::new(
            positions[end].x - positions[start].x,
            positions[end].y - positions[start].y,
            positions[end].z - positions[start].z,
        );
        ir.model.curves.push(Curve {
            id: curve_ids[index].clone(),
            geometry: CurveGeometry::Line {
                origin: Point3::new(
                    positions[start].x - 2.0 * delta.x,
                    positions[start].y - 2.0 * delta.y,
                    positions[start].z - 2.0 * delta.z,
                ),
                direction: delta,
            },
            source_object: None,
        });
        ir.model.edges.push(Edge {
            id: edge_ids[index].clone(),
            curve: Some(curve_ids[index].clone()),
            start: vertex_ids[start].clone(),
            end: vertex_ids[end].clone(),
            param_range: Some([2.0, 3.0]),
            tolerance: None,
        });
    }
    let uses = [
        (0, Sense::Forward),
        (1, Sense::Forward),
        (2, Sense::Forward),
        (3, Sense::Forward),
        (4, Sense::Forward),
        (5, Sense::Forward),
        (6, Sense::Forward),
        (1, Sense::Reversed),
    ];
    for (index, (edge, sense)) in uses.into_iter().enumerate() {
        let radial_next = if index == 1 {
            7
        } else if index == 7 {
            1
        } else {
            index
        };
        ir.model.coedges.push(Coedge {
            id: coedge_ids[index].clone(),
            owner_loop: loop_ids[usize::from(index >= 4)].clone(),
            edge: edge_ids[edge].clone(),
            radial_next: coedge_ids[radial_next].clone(),
            sense,
            pcurves: Vec::new(),
            use_curve: None,
        });
    }
    ir.finalize();
    ir
}

pub(crate) fn planar_tetrahedron() -> CadIr {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
    use cadmpeg_ir::ids::*;
    use cadmpeg_ir::math::Vector3;
    use cadmpeg_ir::topology::*;

    let mut ir = CadIr::empty();
    let body: BodyId = "cadir:model:body#tetrahedron".into();
    let region: RegionId = "cadir:model:region#tetrahedron".into();
    let shell: ShellId = "cadir:model:shell#tetrahedron".into();
    let positions = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
        Point3::new(0.0, 0.0, 1.0),
    ];
    let point_ids = (0..4)
        .map(|index| {
            PointId::mint(format!("cadir:model:point#tetrahedron.{index}"))
                .expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let vertex_ids = (0..4)
        .map(|index| {
            VertexId::mint(format!("cadir:model:vertex#tetrahedron.{index}"))
                .expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let edge_ids = (0..6)
        .map(|index| {
            EdgeId::mint(format!("cadir:model:edge#tetrahedron.{index}")).expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let curve_ids = (0..6)
        .map(|index| {
            CurveId::mint(format!("cadir:model:curve#tetrahedron.{index}"))
                .expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let face_ids = (0..4)
        .map(|index| {
            FaceId::mint(format!("cadir:model:face#tetrahedron.{index}")).expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let loop_ids = (0..4)
        .map(|index| {
            LoopId::mint(format!("cadir:model:loop#tetrahedron.{index}")).expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let surface_ids = (0..4)
        .map(|index| {
            SurfaceId::mint(format!("cadir:model:surface#tetrahedron.{index}"))
                .expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let coedge_ids = (0..12)
        .map(|index| {
            CoedgeId::mint(format!("cadir:model:coedge#tetrahedron.{index}"))
                .expect("identity grammar")
        })
        .collect::<Vec<_>>();
    ir.model.bodies.push(Body {
        id: body.clone(),
        kind: BodyKind::Solid,
        regions: vec![region.clone()],
        transform: None,
        name: Some("tetrahedron".into()),
        color: None,
        visible: Some(true),
    });
    ir.model.regions.push(Region {
        id: region.clone(),
        body,
        shells: vec![shell.clone()],
    });
    ir.model.shells.push(Shell {
        id: shell.clone(),
        region,
        faces: face_ids.clone(),
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
    for index in 0..4 {
        ir.model.points.push(Point {
            id: point_ids[index].clone(),
            position: positions[index],
            source_object: None,
        });
        ir.model.vertices.push(Vertex {
            id: vertex_ids[index].clone(),
            point: point_ids[index].clone(),
            tolerance: None,
        });
    }
    let endpoints = [(0, 1), (1, 2), (2, 0), (0, 3), (1, 3), (2, 3)];
    for (index, (start, end)) in endpoints.into_iter().enumerate() {
        let delta = Vector3::new(
            positions[end].x - positions[start].x,
            positions[end].y - positions[start].y,
            positions[end].z - positions[start].z,
        );
        let length = delta.norm();
        let direction = Vector3::new(delta.x / length, delta.y / length, delta.z / length);
        ir.model.curves.push(Curve {
            id: curve_ids[index].clone(),
            geometry: CurveGeometry::Line {
                origin: Point3::new(
                    positions[start].x - 2.0 * direction.x,
                    positions[start].y - 2.0 * direction.y,
                    positions[start].z - 2.0 * direction.z,
                ),
                direction,
            },
            source_object: None,
        });
        ir.model.edges.push(Edge {
            id: edge_ids[index].clone(),
            curve: Some(curve_ids[index].clone()),
            start: vertex_ids[start].clone(),
            end: vertex_ids[end].clone(),
            param_range: Some([2.0, 2.0 + length]),
            tolerance: None,
        });
    }
    let inverse_sqrt_2 = 1.0 / 2.0_f64.sqrt();
    let inverse_sqrt_3 = 1.0 / 3.0_f64.sqrt();
    let planes = [
        (Vector3::new(0.0, 0.0, -1.0), Vector3::new(1.0, 0.0, 0.0)),
        (Vector3::new(0.0, -1.0, 0.0), Vector3::new(1.0, 0.0, 0.0)),
        (
            Vector3::new(inverse_sqrt_3, inverse_sqrt_3, inverse_sqrt_3),
            Vector3::new(-inverse_sqrt_2, inverse_sqrt_2, 0.0),
        ),
        (Vector3::new(-1.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
    ];
    let face_uses = [
        [
            (2, Sense::Reversed),
            (1, Sense::Reversed),
            (0, Sense::Reversed),
        ],
        [
            (0, Sense::Forward),
            (4, Sense::Forward),
            (3, Sense::Reversed),
        ],
        [
            (1, Sense::Forward),
            (5, Sense::Forward),
            (4, Sense::Reversed),
        ],
        [
            (3, Sense::Forward),
            (5, Sense::Reversed),
            (2, Sense::Forward),
        ],
    ];
    for face in 0..4 {
        let start = face * 3;
        ir.model.faces.push(Face {
            id: face_ids[face].clone(),
            shell: shell.clone(),
            surface: surface_ids[face].clone(),
            sense: Sense::Forward,
            loops: vec![loop_ids[face].clone()],
            name: None,
            color: None,
            tolerance: None,
        });
        ir.model.loops.push(Loop {
            id: loop_ids[face].clone(),
            face: face_ids[face].clone(),
            boundary_role: LoopBoundaryRole::default(),
            boundary: cadmpeg_ir::topology::LoopBoundary::Ring {
                coedges: coedge_ids[start..start + 3].to_vec(),
                vertex_uses: Vec::new(),
            },
        });
        ir.model.surfaces.push(Surface {
            id: surface_ids[face].clone(),
            geometry: SurfaceGeometry::Plane {
                origin: positions[face_uses[face][0].0],
                normal: planes[face].0,
                u_axis: planes[face].1,
            },
            source_object: None,
        });
        for offset in 0..3 {
            let index = start + offset;
            ir.model.coedges.push(Coedge {
                id: coedge_ids[index].clone(),
                owner_loop: loop_ids[face].clone(),
                edge: edge_ids[face_uses[face][offset].0].clone(),
                radial_next: coedge_ids[index].clone(),
                sense: face_uses[face][offset].1,
                pcurves: Vec::new(),
                use_curve: None,
            });
        }
    }
    for edge_id in &edge_ids {
        let uses = ir
            .model
            .coedges
            .iter()
            .enumerate()
            .filter(|(_, coedge)| coedge.edge == *edge_id)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        ir.model.coedges[uses[0]].radial_next = coedge_ids[uses[1]].clone();
        ir.model.coedges[uses[1]].radial_next = coedge_ids[uses[0]].clone();
    }
    ir.finalize();
    ir
}

pub(crate) fn rectangular_nurbs_patch() -> CadIr {
    use cadmpeg_ir::geometry::{
        CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry, SurfaceGeometry,
    };

    let points = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(3.0, 0.0, 0.0),
        Point3::new(3.0, 2.0, 1.0),
        Point3::new(0.0, 2.0, 0.0),
    ];
    let mut ir = polygon_sheet(&points);
    ir.model.surfaces[0].geometry = SurfaceGeometry::Nurbs(
        NurbsSurface::new(
            1,
            1,
            vec![2.0, 2.0, 5.0, 5.0],
            vec![7.0, 7.0, 11.0, 11.0],
            2,
            2,
            vec![points[0], points[3], points[1], points[2]],
            Some(vec![1.0, 0.8, 1.2, 1.0]),
            false,
            false,
            false,
        )
        .expect("valid patch surface"),
    );
    let edge_data = [
        (
            [20.0, 23.0],
            vec![points[0], points[1]],
            vec![1.0, 1.2],
            cadmpeg_ir::math::Point2::new(-18.0, 7.0),
            cadmpeg_ir::math::Point2::new(1.0, 0.0),
        ),
        (
            [30.0, 32.0],
            vec![points[1], points[2]],
            vec![1.2, 1.0],
            cadmpeg_ir::math::Point2::new(5.0, -53.0),
            cadmpeg_ir::math::Point2::new(0.0, 2.0),
        ),
        (
            [40.0, 43.0],
            vec![points[2], points[3]],
            vec![1.0, 0.8],
            cadmpeg_ir::math::Point2::new(45.0, 11.0),
            cadmpeg_ir::math::Point2::new(-1.0, 0.0),
        ),
        (
            [50.0, 52.0],
            vec![points[3], points[0]],
            vec![0.8, 1.0],
            cadmpeg_ir::math::Point2::new(2.0, 111.0),
            cadmpeg_ir::math::Point2::new(0.0, -2.0),
        ),
    ];
    for (index, (domain, control_points, weights, origin, direction)) in
        edge_data.into_iter().enumerate()
    {
        ir.model.edges[index].param_range = Some(domain);
        ir.model.curves[index].geometry = CurveGeometry::Nurbs(
            NurbsCurve::new(
                1,
                vec![domain[0], domain[0], domain[1], domain[1]],
                control_points,
                Some(weights),
                false,
            )
            .expect("valid patch edge"),
        );
        let id: cadmpeg_ir::ids::PcurveId = format!("cadir:model:pcurve#patch.{index}").into();
        ir.model.pcurves.push(Pcurve {
            id: id.clone(),
            geometry: PcurveGeometry::Line { origin, direction },
            metadata: cadmpeg_ir::geometry::PcurveMetadata::general(
                None,
                Some(domain),
                Some(0.001),
            ),
        });
        ir.model.coedges[index].pcurves = vec![cadmpeg_ir::topology::PcurveUse {
            pcurve: id,
            isoparametric: None,
            parameter_range: None,
        }];
    }
    ir.finalize();
    ir
}

pub(crate) fn mixed_plane_nurbs_sheet() -> CadIr {
    use cadmpeg_ir::geometry::{
        CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry, SurfaceGeometry,
    };

    let mut ir = adjacent_quad_sheet();
    let points = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(1.0, 1.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
    ];
    ir.model.surfaces[0].geometry = SurfaceGeometry::Nurbs(
        NurbsSurface::new(
            1,
            1,
            vec![2.0, 2.0, 5.0, 5.0],
            vec![7.0, 7.0, 11.0, 11.0],
            2,
            2,
            vec![points[0], points[3], points[1], points[2]],
            Some(vec![1.0, 0.8, 1.2, 1.0]),
            false,
            false,
            false,
        )
        .expect("valid mixed surface"),
    );
    let edge_data = [
        (
            [20.0, 23.0],
            vec![points[0], points[1]],
            vec![1.0, 1.2],
            cadmpeg_ir::math::Point2::new(-18.0, 7.0),
            cadmpeg_ir::math::Point2::new(1.0, 0.0),
        ),
        (
            [30.0, 32.0],
            vec![points[1], points[2]],
            vec![1.2, 1.0],
            cadmpeg_ir::math::Point2::new(5.0, -53.0),
            cadmpeg_ir::math::Point2::new(0.0, 2.0),
        ),
        (
            [40.0, 43.0],
            vec![points[2], points[3]],
            vec![1.0, 0.8],
            cadmpeg_ir::math::Point2::new(45.0, 11.0),
            cadmpeg_ir::math::Point2::new(-1.0, 0.0),
        ),
        (
            [50.0, 52.0],
            vec![points[3], points[0]],
            vec![0.8, 1.0],
            cadmpeg_ir::math::Point2::new(2.0, 111.0),
            cadmpeg_ir::math::Point2::new(0.0, -2.0),
        ),
    ];
    for (index, (domain, control_points, weights, origin, direction)) in
        edge_data.into_iter().enumerate()
    {
        ir.model.edges[index].param_range = Some(domain);
        ir.model.curves[index].geometry = CurveGeometry::Nurbs(
            NurbsCurve::new(
                1,
                vec![domain[0], domain[0], domain[1], domain[1]],
                control_points,
                Some(weights),
                false,
            )
            .expect("valid mixed edge"),
        );
        let id: cadmpeg_ir::ids::PcurveId = format!("cadir:model:pcurve#mixed.{index}").into();
        ir.model.pcurves.push(Pcurve {
            id: id.clone(),
            geometry: PcurveGeometry::Line { origin, direction },
            metadata: cadmpeg_ir::geometry::PcurveMetadata::general(
                None,
                Some(domain),
                Some(0.001),
            ),
        });
        ir.model.coedges[index].pcurves = vec![cadmpeg_ir::topology::PcurveUse {
            pcurve: id,
            isoparametric: None,
            parameter_range: None,
        }];
    }
    ir.finalize();
    ir
}

pub(crate) fn make_planar_nurbs_trimmed_face(ir: &mut CadIr) {
    use cadmpeg_ir::geometry::{NurbsSurface, Pcurve, PcurveGeometry, SurfaceGeometry};

    ir.model.surfaces[0].geometry = SurfaceGeometry::Nurbs(
        NurbsSurface::new(
            1,
            1,
            vec![0.0, 0.0, 4.0, 4.0],
            vec![0.0, 0.0, 4.0, 4.0],
            2,
            2,
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(0.0, 4.0, 0.0),
                Point3::new(4.0, 0.0, 0.0),
                Point3::new(4.0, 4.0, 0.0),
            ],
            None,
            false,
            false,
            false,
        )
        .expect("valid planar patch"),
    );
    for index in 0..ir.model.coedges.len() {
        let coedge = &ir.model.coedges[index];
        let edge = ir
            .model
            .edges
            .iter()
            .find(|edge| edge.id == coedge.edge)
            .expect("fixture edge");
        let domain = edge.param_range.expect("fixture edge domain");
        let (start, end) = if coedge.sense == cadmpeg_ir::topology::Sense::Forward {
            (&edge.start, &edge.end)
        } else {
            (&edge.end, &edge.start)
        };
        let start = vertex_point(&ir.model, start).expect("fixture start");
        let end = vertex_point(&ir.model, end).expect("fixture end");
        let scale = domain[1] - domain[0];
        let direction =
            cadmpeg_ir::math::Point2::new((end.x - start.x) / scale, (end.y - start.y) / scale);
        let origin = cadmpeg_ir::math::Point2::new(
            start.x - direction.u * domain[0],
            start.y - direction.v * domain[0],
        );
        let id: cadmpeg_ir::ids::PcurveId = format!("cadir:model:pcurve#general.{index}").into();
        ir.model.pcurves.push(Pcurve {
            id: id.clone(),
            geometry: PcurveGeometry::Line { origin, direction },
            metadata: cadmpeg_ir::geometry::PcurveMetadata::general(
                None,
                Some(domain),
                Some(0.0001),
            ),
        });
        ir.model.coedges[index].pcurves = vec![cadmpeg_ir::topology::PcurveUse {
            pcurve: id,
            isoparametric: None,
            parameter_range: None,
        }];
    }
    ir.finalize();
}
