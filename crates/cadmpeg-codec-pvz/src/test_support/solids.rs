// SPDX-License-Identifier: Apache-2.0
//! Code-built closed solids with curved, B-spline, and nested faces
//! (specification §8).
//!
//! Every face stores its loops counter-clockwise in its own `(u, v)` domain
//! when the domain normal points out of the solid, and inner loops
//! clockwise. The two coedges of each edge run in opposite directions.

use std::f64::consts::TAU;

use super::exact::{Patch, Stream, NONE};

/// Radius of [`cylinder_stream`] and [`cone_stream`], in metres.
pub(crate) const RADIUS: f64 = 0.005;
/// Height of [`cylinder_stream`] and [`cone_stream`], in metres.
pub(crate) const HEIGHT: f64 = 0.01;
/// Side of the square block of [`holed_block`], in metres.
pub(crate) const BLOCK_SIDE: f64 = 0.015;
/// Side of the square hole of [`holed_block`], in metres.
pub(crate) const HOLE_SIDE: f64 = 0.005;
/// Thickness of [`holed_block`], in metres.
pub(crate) const BLOCK_THICKNESS: f64 = 0.005;
/// Weight of every pole of a [`PlanarCarrier::RationalBspline`] face.
const RATIONAL_WEIGHT: f64 = 2.0;

fn index(value: usize) -> u32 {
    u32::try_from(value).expect("record index")
}

/// The bottom cap of [`cylinder_stream`] and [`cone_stream`]: the plane
/// `z = 0` with `U × V = −z`, bounded by one coedge on edge `edge`, the
/// circle of [`RADIUS`] about the z axis, against the circle direction.
fn bottom_cap(stream: &mut Stream, edge: u32, pcurve: u32) {
    stream.plane([0.0; 3], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]);
    stream
        .loop_record(-2)
        .coedge(edge, 0, pcurve, NONE)
        .terminator();
}

/// The domain image `(y, x)` of the circle of [`bottom_cap`], run against
/// the circle direction.
fn bottom_cap_pcurve(stream: &mut Stream) {
    stream.ellipse_pcurve([0.0, 0.0], [0.0, RADIUS], [-RADIUS, 0.0]);
}

/// The four seam-bounded native pcurves of a periodic face over
/// `u ∈ [0, 2π]`, `v ∈ [0, 1]`: the `v = 0` boundary, the seam at
/// `u = 2π`, the `v = 1` boundary, and the seam at `u = 0`.
fn seam_rectangle_pcurves(stream: &mut Stream) {
    stream
        .line_pcurve([0.0, 0.0], [TAU, 0.0])
        .line_pcurve([TAU, 0.0], [0.0, 1.0])
        .line_pcurve([TAU, 1.0], [-TAU, 0.0])
        .line_pcurve([0.0, 1.0], [0.0, -1.0]);
}

/// A closed cylinder of [`RADIUS`] and [`HEIGHT`] on the z axis from
/// `z = 0`: one cylinder face whose loop runs along the bottom circle, up
/// the seam, back along the top circle, and down the seam, and two plane
/// caps with one circle coedge each. Edge 0 is the bottom circle, edge 1
/// the top circle, and edge 2 the seam line; the seam has two coedges on
/// the cylinder face.
pub(crate) fn cylinder_stream() -> Stream {
    let mut stream = Stream::new(true);
    stream.cylinder(
        [0.0; 3],
        [RADIUS, 0.0, 0.0],
        [0.0, RADIUS, 0.0],
        [0.0, 0.0, HEIGHT],
    );
    stream
        .loop_record(-2)
        .coedge(0, 1, 0, NONE)
        .coedge(2, 1, 1, NONE)
        .coedge(1, 0, 2, NONE)
        .coedge(2, 0, 3, NONE)
        .terminator();
    bottom_cap(&mut stream, 0, 4);
    stream.plane(
        [0.0, 0.0, HEIGHT],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    );
    stream.loop_record(-2).coedge(1, 1, 5, NONE).terminator();
    stream
        .circle_edge([0.0; 3], [RADIUS, 0.0, 0.0], [0.0, RADIUS, 0.0])
        .circle_edge([0.0, 0.0, HEIGHT], [RADIUS, 0.0, 0.0], [0.0, RADIUS, 0.0])
        .line_edge([RADIUS, 0.0, 0.0], [0.0, 0.0, HEIGHT]);
    seam_rectangle_pcurves(&mut stream);
    bottom_cap_pcurve(&mut stream);
    stream.ellipse_pcurve([0.0, 0.0], [RADIUS, 0.0], [0.0, RADIUS]);
    stream
}

/// A closed right circular cone of base [`RADIUS`] at `z = 0` and apex at
/// `z = `[`HEIGHT`]: one cone face with `a = 1`, `b = −1`, so its apex is
/// at `v = 1`, and one plane base cap. The cone loop runs along the base
/// circle (edge 0), up the seam line (edge 1), along the apex point curve
/// (edge 2) from `u = 2π` to `u = 0`, and down the seam. A singular use of
/// edge 2 on the cone face states the apex collapse.
pub(crate) fn cone_stream() -> Stream {
    let mut stream = Stream::new(true);
    stream.cone(
        [0.0; 3],
        [RADIUS, 0.0, 0.0],
        [0.0, RADIUS, 0.0],
        [0.0, 0.0, HEIGHT],
        [1.0, -1.0],
    );
    stream
        .loop_record(-2)
        .coedge(0, 1, 0, NONE)
        .coedge(1, 1, 1, NONE)
        .coedge(2, 0, 2, NONE)
        .coedge(1, 0, 3, NONE)
        .terminator();
    stream.singular_use(2, [0.0, 0.0]);
    bottom_cap(&mut stream, 0, 4);
    stream
        .circle_edge([0.0; 3], [RADIUS, 0.0, 0.0], [0.0, RADIUS, 0.0])
        .line_edge([RADIUS, 0.0, 0.0], [-RADIUS, 0.0, HEIGHT])
        .point_edge([0.0, 0.0, HEIGHT]);
    seam_rectangle_pcurves(&mut stream);
    bottom_cap_pcurve(&mut stream);
    stream
}

/// The supporting surface record of one planar face.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanarCarrier {
    /// A plane record with native line pcurves.
    Plane,
    /// A rational bilinear B-spline record equal to the plane over the face
    /// bounds, with native line pcurves in its domain.
    RationalBspline,
    /// A plane record with an embedded bilinear B-spline over `[0, 1]²`,
    /// and only secondary line pcurves in the embedded domain.
    SecondaryPcurves,
}

/// One planar face: its outward normal, the vertex ring of its outer loop,
/// the vertex rings of its inner loops, and its carrier. Rings can run in
/// either direction; the builder orients them.
#[derive(Debug, Clone)]
pub(crate) struct PlanarFace {
    pub(crate) outward: [f64; 3],
    pub(crate) outer: Vec<usize>,
    pub(crate) inner: Vec<Vec<usize>>,
    pub(crate) carrier: PlanarCarrier,
}

/// A closed polyhedron with line edges, one record per shared edge.
#[derive(Debug, Clone)]
pub(crate) struct Polyhedron {
    pub(crate) vertices: Vec<[f64; 3]>,
    pub(crate) faces: Vec<PlanarFace>,
}

/// The cube of [`super::exact::CUBE_FACES`], with `carrier(f)` the carrier
/// of face `f`.
pub(crate) fn cube(carrier: impl Fn(usize) -> PlanarCarrier) -> Polyhedron {
    let vertices: Vec<[f64; 3]> = (0..8).map(super::exact::cube_corner).collect();
    let half = super::exact::CUBE_SIZE / 2.0;
    let faces = super::exact::CUBE_FACES
        .iter()
        .enumerate()
        .map(|(face, ring)| {
            let centre = ring
                .iter()
                .fold([0.0; 3], |sum, corner| add(sum, vertices[*corner]));
            PlanarFace {
                outward: sub(scale(centre, 0.25), [half; 3]),
                outer: ring.to_vec(),
                inner: Vec::new(),
                carrier: carrier(face),
            }
        })
        .collect();
    Polyhedron { vertices, faces }
}

/// A square block of side [`BLOCK_SIDE`] and thickness [`BLOCK_THICKNESS`]
/// on `z = 0` with a centred square through hole of side [`HOLE_SIDE`].
/// The bottom and top faces each have one outer loop and one inner loop.
pub(crate) fn holed_block() -> Polyhedron {
    let (side, hole, thickness) = (BLOCK_SIDE, HOLE_SIDE, BLOCK_THICKNESS);
    let low = (side - hole) / 2.0;
    let high = low + hole;
    let square = |a: f64, b: f64, z: f64| [[a, a, z], [b, a, z], [b, b, z], [a, b, z]];
    let mut vertices = Vec::new();
    vertices.extend(square(0.0, side, 0.0));
    vertices.extend(square(0.0, side, thickness));
    vertices.extend(square(low, high, 0.0));
    vertices.extend(square(low, high, thickness));
    let face = |outward: [f64; 3], outer: [usize; 4], inner: Vec<Vec<usize>>| PlanarFace {
        outward,
        outer: outer.to_vec(),
        inner,
        carrier: PlanarCarrier::Plane,
    };
    let faces = vec![
        face([0.0, 0.0, -1.0], [0, 1, 2, 3], vec![vec![8, 9, 10, 11]]),
        face([0.0, 0.0, 1.0], [4, 5, 6, 7], vec![vec![12, 13, 14, 15]]),
        face([0.0, -1.0, 0.0], [0, 1, 5, 4], Vec::new()),
        face([1.0, 0.0, 0.0], [1, 2, 6, 5], Vec::new()),
        face([0.0, 1.0, 0.0], [2, 3, 7, 6], Vec::new()),
        face([-1.0, 0.0, 0.0], [3, 0, 4, 7], Vec::new()),
        face([0.0, 1.0, 0.0], [8, 9, 13, 12], Vec::new()),
        face([-1.0, 0.0, 0.0], [9, 10, 14, 13], Vec::new()),
        face([0.0, -1.0, 0.0], [10, 11, 15, 14], Vec::new()),
        face([1.0, 0.0, 0.0], [11, 8, 12, 15], Vec::new()),
    ];
    Polyhedron { vertices, faces }
}

/// `ring` turned so that its Newell normal points along `normal` when
/// `along`, and against it otherwise.
fn oriented(vertices: &[[f64; 3]], ring: &[usize], normal: [f64; 3], along: bool) -> Vec<usize> {
    let mut newell = [0.0; 3];
    for (position, corner) in ring.iter().enumerate() {
        let next = ring[(position + 1) % ring.len()];
        newell = add(newell, cross(vertices[*corner], vertices[next]));
    }
    let mut ring = ring.to_vec();
    if (dot(newell, normal) > 0.0) != along {
        ring.reverse();
    }
    ring
}

/// The exact stream of `polyhedron`. Each face plane has its origin at the
/// first outer vertex, a unit `U` along the first outer edge, and
/// `V = N × U` for the unit outward normal `N`.
pub(crate) fn polyhedron_stream(polyhedron: &Polyhedron) -> Stream {
    let vertices = &polyhedron.vertices;
    let mut edges: Vec<(usize, usize)> = Vec::new();
    let mut pcurves: Vec<([f64; 2], [f64; 2])> = Vec::new();
    let mut stream = Stream::new(true);
    let mut loops = 0_usize;
    for face in &polyhedron.faces {
        let normal = unit(face.outward);
        let outer = oriented(vertices, &face.outer, normal, true);
        let origin = vertices[outer[0]];
        let u_axis = unit(sub(vertices[outer[1]], origin));
        let v_axis = cross(normal, u_axis);
        let uv = |corner: usize| {
            let offset = sub(vertices[corner], origin);
            [dot(offset, u_axis), dot(offset, v_axis)]
        };
        let bounds =
            outer
                .iter()
                .fold([[f64::INFINITY, f64::NEG_INFINITY]; 2], |[u, v], corner| {
                    let [a, b] = uv(*corner);
                    [[u[0].min(a), u[1].max(a)], [v[0].min(b), v[1].max(b)]]
                });
        let plane_point = |u: f64, v: f64| add(origin, add(scale(u_axis, u), scale(v_axis, v)));
        let to_unit = |[u, v]: [f64; 2]| {
            [
                (u - bounds[0][0]) / (bounds[0][1] - bounds[0][0]),
                (v - bounds[1][0]) / (bounds[1][1] - bounds[1][0]),
            ]
        };
        match face.carrier {
            PlanarCarrier::Plane => {
                stream.plane(origin, u_axis, v_axis, normal);
            }
            PlanarCarrier::RationalBspline => {
                let patch =
                    Patch::bilinear(bounds[0], bounds[1], Some(RATIONAL_WEIGHT), plane_point);
                stream.bspline_surface(&patch);
            }
            PlanarCarrier::SecondaryPcurves => {
                let embedded = Patch::bilinear([0.0, 1.0], [0.0, 1.0], None, |s, t| {
                    plane_point(
                        bounds[0][0] + s * (bounds[0][1] - bounds[0][0]),
                        bounds[1][0] + t * (bounds[1][1] - bounds[1][0]),
                    )
                });
                stream.plane_with_embedded(origin, u_axis, v_axis, normal, &embedded);
            }
        }
        let first = loops;
        let rings = std::iter::once(outer.clone()).chain(
            face.inner
                .iter()
                .map(|ring| oriented(vertices, ring, normal, false)),
        );
        for (position, ring) in rings.enumerate() {
            let marker = if position == 0 {
                -2
            } else {
                i32::try_from(first).expect("loop index")
            };
            stream.loop_record(marker);
            for (corner, start) in ring.iter().enumerate() {
                let end = ring[(corner + 1) % ring.len()];
                let key = ((*start).min(end), (*start).max(end));
                let edge = edges
                    .iter()
                    .position(|known| *known == key)
                    .unwrap_or_else(|| {
                        edges.push(key);
                        edges.len() - 1
                    });
                let (mut from, mut to) = (uv(*start), uv(end));
                if face.carrier == PlanarCarrier::SecondaryPcurves {
                    (from, to) = (to_unit(from), to_unit(to));
                }
                let pcurve = index(pcurves.len());
                pcurves.push((from, [to[0] - from[0], to[1] - from[1]]));
                let (native, secondary) = if face.carrier == PlanarCarrier::SecondaryPcurves {
                    (NONE, pcurve)
                } else {
                    (pcurve, NONE)
                };
                stream.coedge(index(edge), u8::from(*start < end), native, secondary);
            }
            stream.terminator();
            loops += 1;
        }
    }
    for (a, b) in edges {
        stream.line_edge(vertices[a], sub(vertices[b], vertices[a]));
    }
    for (origin, direction) in pcurves {
        stream.line_pcurve(origin, direction);
    }
    stream
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: [f64; 3], factor: f64) -> [f64; 3] {
    a.map(|value| value * factor)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit(a: [f64; 3]) -> [f64; 3] {
    scale(a, dot(a, a).sqrt().recip())
}
