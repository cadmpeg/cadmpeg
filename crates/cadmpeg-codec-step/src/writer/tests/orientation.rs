// SPDX-License-Identifier: Apache-2.0
//! Loop orientation of written STEP solids.

#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::CadIr;

use crate::test_support::exchange::export;
use crate::StepCodec;

/// Signed volume of a closed planar solid by the divergence theorem, and
/// whether every loop runs counterclockwise about its face's oriented normal.
fn planar_solid_volume_and_ccw_loops(ir: &CadIr) -> (f64, bool) {
    use cadmpeg_ir::topology::{LoopBoundary, Sense};
    let model = &ir.model;
    let position = |vertex: &cadmpeg_ir::ids::VertexId| {
        let vertex = model.vertices.iter().find(|v| &v.id == vertex).unwrap();
        let point = model.points.iter().find(|p| p.id == vertex.point).unwrap();
        point.position().get()
    };
    let origin = Point3::new(0.0, 0.0, 0.0);
    let mut volume6 = 0.0;
    let mut every_loop_ccw = true;
    for face in &model.faces {
        let surface = model
            .surfaces
            .iter()
            .find(|s| s.id == face.surface)
            .unwrap();
        let Some(SolvedSurfaceGeometry::Plane(plane)) = surface.geometry.solved() else {
            panic!("planar face");
        };
        let mut normal = *plane.frame().axis().as_raw();
        if face.sense == Sense::Reversed {
            normal = Vector3::new(-normal.x, -normal.y, -normal.z);
        }
        for loop_id in &face.loops {
            let ring = model.loops.iter().find(|l| &l.id == loop_id).unwrap();
            let LoopBoundary::Ring(ring) = &ring.boundary else {
                panic!("edge loop");
            };
            let corners: Vec<Point3> = ring
                .coedges()
                .iter()
                .map(|id| {
                    let coedge = model.coedges.iter().find(|c| &c.id == id).unwrap();
                    let edge = model.edges.iter().find(|e| e.id == coedge.edge).unwrap();
                    position(match coedge.sense {
                        Sense::Forward => &edge.start,
                        Sense::Reversed => &edge.end,
                    })
                })
                .collect();
            let mut area_normal = Vector3::new(0.0, 0.0, 0.0);
            for (index, a) in corners.iter().enumerate() {
                let b = corners[(index + 1) % corners.len()];
                let term = a.vector_from(origin).cross(b.vector_from(origin));
                area_normal = Vector3::new(
                    area_normal.x + term.x,
                    area_normal.y + term.y,
                    area_normal.z + term.z,
                );
            }
            every_loop_ccw &= area_normal.dot(normal) > 0.0;
            for pair in corners[1..].windows(2) {
                let [o, a, b] = [corners[0], pair[0], pair[1]].map(|p| p.vector_from(origin));
                volume6 += o.dot(a.cross(b));
            }
        }
    }
    (volume6 / 6.0, every_loop_ccw)
}

#[test]
fn cube_round_trip_bounds_run_counterclockwise_about_outward_face_normals() {
    const EPS_CUBE_VOLUME: f64 = 1.0e-9;
    let source = unit_cube().expect("unit cube fixture is admitted");
    let (volume, ccw) = planar_solid_volume_and_ccw_loops(&source);
    assert!(ccw, "source cube loops run counterclockwise");
    assert!((volume - 1000.0).abs() < EPS_CUBE_VOLUME, "{volume}");

    let output = export(&source);
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode written cube");
    let (volume, ccw) = planar_solid_volume_and_ccw_loops(decoded.ir());
    assert!(ccw, "written cube bounds run counterclockwise");
    assert!((volume - 1000.0).abs() < EPS_CUBE_VOLUME, "{volume}");
}

#[test]
fn a_bspline_face_with_a_reversed_normal_keeps_its_outward_side() {
    use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::topology::Sense;
    // `Pu × Pv` is +z; the reversed oriented normal is -z, the bottom face's
    // outward side.
    let mut source = unit_cube().expect("unit cube fixture is admitted");
    let bottom = source.model.faces[0].surface.clone();
    let patch = NurbsSurface::new(
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsPoleGrid::Polynomial {
            rows: vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 10.0, 0.0)],
                vec![Point3::new(10.0, 0.0, 0.0), Point3::new(10.0, 10.0, 0.0)],
            ],
        },
        true,
    )
    .expect("bilinear patch");
    let surface = source
        .model
        .surfaces
        .iter_mut()
        .find(|surface| surface.id == bottom)
        .expect("bottom surface");
    surface.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(patch));

    let output = export(&source);
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode written cube");
    let model = &decoded.ir().model;
    let mut patches = 0;
    for face in &model.faces {
        let surface = model
            .surfaces
            .iter()
            .find(|surface| surface.id == face.surface)
            .expect("face surface");
        let Some(SolvedSurfaceGeometry::Nurbs(nurbs)) = surface.geometry.solved() else {
            continue;
        };
        patches += 1;
        let rows = nurbs.control_grid();
        let corner = rows[0][0].get();
        let along_u = rows[1][0].get().vector_from(corner);
        let along_v = rows[0][1].get().vector_from(corner);
        let mut normal = along_u.cross(along_v);
        if nurbs.normal_reversed() != (face.sense == Sense::Reversed) {
            normal = Vector3::new(-normal.x, -normal.y, -normal.z);
        }
        assert!(normal.z < 0.0, "{normal:?}");
    }
    assert_eq!(patches, 1);
}
