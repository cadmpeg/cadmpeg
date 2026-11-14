// SPDX-License-Identifier: Apache-2.0
//! Loop direction of a closed planar shell follows its oriented face normals.

use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::Sense;

use super::{assert_valid, decode};
use crate::test_support::test_container::{
    standard_catpart_from_streams, tetrahedron_topology_catpart,
};

const CORNERS: [[f32; 3]; 4] = [
    [1.0, 1.0, 1.0],
    [1.0, -1.0, -1.0],
    [-1.0, 1.0, -1.0],
    [-1.0, -1.0, 1.0],
];
const FACE_CORNERS: [[usize; 3]; 4] = [[0, 1, 2], [0, 3, 1], [1, 3, 2], [2, 3, 0]];

/// The tetrahedron with plane carriers. Each trim packet stores the outward
/// face normal, or the inward one when `inside_out` holds; every face-sense
/// byte is forward.
fn planar_tetrahedron(inside_out: bool) -> Vec<u8> {
    let (mut main, _) = crate::test_support::with_service_context(|ctx| {
        let scan = crate::container::scan_bytes(ctx, tetrahedron_topology_catpart())
            .expect("authored closed tetrahedron");
        let main = scan.main_data_stream().expect("main stream");
        let surfaces = &scan.brep.as_ref().expect("standard streams")[main.len()..];
        (main.to_vec(), surfaces.to_vec())
    });
    let packets: Vec<_> = main
        .windows(4)
        .enumerate()
        .filter_map(|(at, bytes)| (bytes == [1, 0x44, 1, 0xff]).then_some(at))
        .collect();
    assert_eq!(packets.len(), 4);
    let mut surfaces = Vec::new();
    for (face, corners) in FACE_CORNERS.iter().enumerate() {
        let points = corners.map(|corner| CORNERS[corner]);
        let centroid: [f32; 3] =
            std::array::from_fn(|axis| points.iter().map(|point| point[axis]).sum::<f32>() / 3.0);
        let length = centroid
            .iter()
            .map(|value| value * value)
            .sum::<f32>()
            .sqrt();
        let sign = if inside_out { -1.0 } else { 1.0 };
        let normal = centroid.map(|value| sign * value / length);
        let lower: [f32; 3] = std::array::from_fn(|axis| {
            points
                .iter()
                .map(|point| point[axis])
                .fold(f32::MAX, f32::min)
        });
        let upper: [f32; 3] = std::array::from_fn(|axis| {
            points
                .iter()
                .map(|point| point[axis])
                .fold(f32::MIN, f32::max)
        });
        let radius = points
            .iter()
            .map(|point| {
                (0..3)
                    .map(|axis| (point[axis] - centroid[axis]).powi(2))
                    .sum::<f32>()
                    .sqrt()
            })
            .fold(0.0, f32::max);
        surfaces.extend_from_slice(&[
            u8::try_from(face + 1).expect("face tag"),
            0,
            0,
            0,
            2,
            0,
            0x33,
            0x32,
        ]);
        for value in (0..3)
            .map(|axis| f32::midpoint(lower[axis], upper[axis]))
            .chain((0..3).map(|axis| (upper[axis] - lower[axis]) / 2.0))
            .chain(centroid)
            .chain([radius])
        {
            surfaces.extend_from_slice(&value.to_le_bytes());
        }
        surfaces.push(1);
        let packet = packets[face] + 12 * face;
        main[packet + 1] = 0x4c;
        main.splice(
            packet + 8..packet + 8,
            normal.into_iter().flat_map(f32::to_le_bytes),
        );
    }
    standard_catpart_from_streams(&main, &surfaces)
}

#[test]
fn every_planar_loop_turns_counterclockwise_about_its_oriented_normal() {
    for inside_out in [false, true] {
        let result = decode(planar_tetrahedron(inside_out));
        assert_valid(&result);
        let model = &result.ir().model;
        assert_eq!(model.faces.len(), 4);
        for face in &model.faces {
            assert_eq!(face.sense, Sense::Forward);
            let surface = model
                .surfaces
                .iter()
                .find(|surface| surface.id == face.surface)
                .expect("face carrier");
            let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) = &surface.geometry
            else {
                panic!("plane carrier");
            };
            let normal = *plane.frame().axis().as_raw();
            let boundary = model
                .loops
                .iter()
                .find(|boundary| boundary.face == face.id)
                .expect("face loop");
            let cadmpeg_ir::topology::LoopBoundary::Ring(ring) = &boundary.boundary else {
                panic!("ring loop");
            };
            let corners: Vec<Point3> = ring
                .vertex_uses()
                .iter()
                .map(|vertex_use| {
                    let vertex = model
                        .vertices
                        .iter()
                        .find(|vertex| vertex.id == vertex_use.vertex)
                        .expect("loop vertex");
                    model
                        .points
                        .iter()
                        .find(|point| point.id == vertex.point)
                        .expect("vertex point")
                        .position()
                        .get()
                })
                .collect();
            assert_eq!(corners.len(), 3);
            let area: Vector3 = corners[1]
                .vector_from(corners[0])
                .cross(corners[2].vector_from(corners[0]));
            assert!(
                area.dot(normal) > 0.0,
                "{} turns clockwise about its oriented normal (inside out: {inside_out})",
                face.id
            );
        }
    }
}
