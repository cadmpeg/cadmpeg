// SPDX-License-Identifier: Apache-2.0
//! B-rep records of the `UsdGeom` intrinsic primitives (spec §11.5), in
//! scene units, so that a primitive becomes a B-rep body through the same
//! import as a `BrepArray` prim.
//!
//! A solid of revolution comes from a profile in `(ρ, z)` that runs from the
//! axis at the bottom to the axis at the top: each segment is a face, each
//! profile point off the axis a circle edge, and each face that is not a disc
//! has a seam at `θ = 0`.

use std::collections::BTreeMap;

use super::{
    AnalyticCurve, AnalyticSurface, Brep, Catalog, CurveKind, EdgeRecord, FaceRecord, Frame,
    LoopRecord, OrientedEdgeRecord, ShellRecord, SolidRecord, SurfaceKind, DEFAULT_TOLERANCE,
};

const X: [f64; 3] = [1.0, 0.0, 0.0];
const Z: [f64; 3] = [0.0, 0.0, 1.0];

/// The axis of a primitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    /// The rotation that takes the local `z` axis to this axis.
    fn rotate(self, [x, y, z]: [f64; 3]) -> [f64; 3] {
        match self {
            Self::X => [z, x, y],
            Self::Y => [y, z, x],
            Self::Z => [x, y, z],
        }
    }
}

/// One segment of a revolved profile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Segment {
    Line,
    /// An arc of the circle about the axis point at height `center`.
    Arc {
        center: f64,
    },
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

fn unit(a: [f64; 3]) -> [f64; 3] {
    let length = length(a);
    a.map(|value| value / length)
}

fn frame(position: [f64; 3], axis: [f64; 3], ref_direction: [f64; 3]) -> Frame {
    Frame {
        position,
        axis,
        ref_direction,
    }
}

/// Records under construction.
struct Records {
    brep: Brep,
    next_face: i64,
}

impl Records {
    fn new() -> Self {
        Self {
            brep: Brep {
                tolerance: DEFAULT_TOLERANCE,
                angle_scale: 1.0,
                valid: true,
                ..Brep::default()
            },
            next_face: 0,
        }
    }

    fn vertex(&mut self, position: [f64; 3]) -> usize {
        self.brep.vertices.push(position);
        self.brep.vertices.len() - 1
    }

    fn edge(&mut self, vertices: [usize; 2], curve: AnalyticCurve, range: [f64; 2]) -> usize {
        self.brep.analytic_curves.push(curve);
        self.brep.edges.push(EdgeRecord {
            vertices,
            curve: Some((Catalog::Analytic, self.brep.analytic_curves.len() - 1)),
            range,
            same_sense: true,
            tolerance: None,
        });
        self.brep.edges.len() - 1
    }

    fn line(&mut self, from: usize, to: usize) -> usize {
        let (start, end) = (self.brep.vertices[from], self.brep.vertices[to]);
        let direction = sub(end, start);
        self.edge(
            [from, to],
            AnalyticCurve {
                kind: CurveKind::Line,
                frame: frame(start, unit(direction), unit(direction)),
                params: Vec::new(),
            },
            [0.0, length(direction)],
        )
    }

    fn face(&mut self, loops: &[Vec<(usize, bool)>], surface: AnalyticSurface) -> usize {
        self.brep.analytic_surfaces.push(surface);
        let mut indices = Vec::new();
        for uses in loops {
            let mut oriented = Vec::new();
            for (edge, orientation) in uses {
                self.brep.oriented_edges.push(OrientedEdgeRecord {
                    edge: *edge,
                    orientation: *orientation,
                    pcurve: None,
                });
                oriented.push(self.brep.oriented_edges.len() - 1);
            }
            self.brep.loops.push(LoopRecord {
                oriented_edges: oriented,
                vertex: None,
            });
            indices.push(self.brep.loops.len() - 1);
        }
        let id = self.brep.faces.len();
        self.brep.faces.push(FaceRecord {
            loops: indices,
            surface: (Catalog::Analytic, self.brep.analytic_surfaces.len() - 1),
            same_sense: true,
            id: self.next_face,
        });
        self.next_face += 1;
        id
    }

    fn finish(mut self, closed: bool, axis: Axis) -> Brep {
        let faces = (0..self.brep.faces.len()).collect();
        self.brep.shells.push(ShellRecord { faces, closed });
        if closed {
            self.brep.solids.push(SolidRecord {
                outer: 0,
                voids: Vec::new(),
            });
        }
        let brep = &mut self.brep;
        for point in &mut brep.vertices {
            *point = axis.rotate(*point);
        }
        for frame in brep
            .analytic_curves
            .iter_mut()
            .map(|curve| &mut curve.frame)
            .chain(
                brep.analytic_surfaces
                    .iter_mut()
                    .map(|surface| &mut surface.frame),
            )
        {
            frame.position = axis.rotate(frame.position);
            frame.axis = axis.rotate(frame.axis);
            frame.ref_direction = axis.rotate(frame.ref_direction);
        }
        self.brep
    }
}

/// The solid that a profile in `(ρ, z)` sweeps about the `axis`. The profile
/// starts and ends on the axis, and `segments` has one entry per pair of
/// adjacent points.
pub(crate) fn revolved(points: &[[f64; 2]], segments: &[Segment], axis: Axis) -> Brep {
    let mut records = Records::new();
    let mut vertices: BTreeMap<usize, usize> = BTreeMap::new();
    let mut circles: BTreeMap<usize, usize> = BTreeMap::new();
    let point = |index: usize| {
        let [rho, z] = points[index];
        [rho, 0.0, z]
    };
    for (index, [rho, z]) in points.iter().enumerate() {
        if *rho > 0.0 {
            let vertex = records.vertex([*rho, 0.0, *z]);
            vertices.insert(index, vertex);
            let circle = records.edge(
                [vertex, vertex],
                AnalyticCurve {
                    kind: CurveKind::Circle,
                    frame: frame([0.0, 0.0, *z], Z, X),
                    params: vec![*rho],
                },
                [0.0, std::f64::consts::TAU],
            );
            circles.insert(index, circle);
        }
    }
    for (index, segment) in segments.iter().enumerate() {
        let (a, b) = (index, index + 1);
        let ([rho_a, z_a], [rho_b, z_b]) = (points[a], points[b]);
        if z_a == z_b {
            // A disc: a plane whose normal is the profile's outward normal.
            let normal = if rho_b > rho_a { [0.0, 0.0, -1.0] } else { Z };
            let mut uses = Vec::new();
            if let Some(circle) = circles.get(&a) {
                uses.push((*circle, true));
            }
            if let Some(circle) = circles.get(&b) {
                uses.push((*circle, false));
            }
            records.face(
                &[uses],
                AnalyticSurface {
                    kind: SurfaceKind::Plane,
                    frame: frame([0.0, 0.0, z_a], normal, X),
                    params: Vec::new(),
                    profile: None,
                },
            );
            continue;
        }
        let mut vertex = |index: usize, records: &mut Records| {
            *vertices
                .entry(index)
                .or_insert_with(|| records.vertex(point(index)))
        };
        let (start, end) = (vertex(a, &mut records), vertex(b, &mut records));
        let (seam, surface) = match segment {
            Segment::Line => {
                let seam = records.line(start, end);
                let surface = if rho_a == rho_b {
                    AnalyticSurface {
                        kind: SurfaceKind::Cylinder,
                        frame: frame([0.0; 3], Z, X),
                        params: vec![rho_a],
                        profile: None,
                    }
                } else {
                    // The cone opens towards its larger radius.
                    let (wide, narrow) = if rho_a > rho_b { (a, b) } else { (b, a) };
                    let [rho_wide, z_wide] = points[wide];
                    let direction = if z_wide > points[narrow][1] {
                        Z
                    } else {
                        [0.0, 0.0, -1.0]
                    };
                    let semi_angle = ((rho_a - rho_b).abs() / (z_a - z_b).abs()).atan();
                    AnalyticSurface {
                        kind: SurfaceKind::Cone,
                        frame: frame([0.0, 0.0, z_wide], direction, X),
                        params: vec![rho_wide, semi_angle],
                        profile: None,
                    }
                };
                (seam, surface)
            }
            Segment::Arc { center } => {
                let center = [0.0, 0.0, *center];
                let (from, to) = (sub(point(a), center), sub(point(b), center));
                let radius = length(from);
                let normal = unit(cross(from, to));
                let sweep = dot(unit(from), unit(to)).clamp(-1.0, 1.0).acos();
                let seam = records.edge(
                    [start, end],
                    AnalyticCurve {
                        kind: CurveKind::Circle,
                        frame: frame(center, normal, unit(from)),
                        params: vec![radius],
                    },
                    [0.0, sweep],
                );
                let surface = AnalyticSurface {
                    kind: SurfaceKind::Sphere,
                    frame: frame(center, Z, X),
                    params: vec![radius],
                    profile: None,
                };
                (seam, surface)
            }
        };
        let mut uses = Vec::new();
        if let Some(circle) = circles.get(&a) {
            uses.push((*circle, true));
        }
        uses.push((seam, true));
        if let Some(circle) = circles.get(&b) {
            uses.push((*circle, false));
        }
        uses.push((seam, false));
        // A loop through a pole starts at the use that leaves it.
        if rho_b == 0.0 {
            uses.rotate_right(1);
        }
        records.face(&[uses], surface);
    }
    records.finish(true, axis)
}

/// A complete sphere: one face with no boundary.
pub(crate) fn sphere(radius: f64) -> Brep {
    let mut records = Records::new();
    records.face(
        &[Vec::new()],
        AnalyticSurface {
            kind: SurfaceKind::Sphere,
            frame: frame([0.0; 3], Z, X),
            params: vec![radius],
            profile: None,
        },
    );
    records.finish(true, Axis::Z)
}

/// A polyhedron of planar faces. Each face lists its vertices counterclockwise
/// about its outward `normal`.
fn polyhedron(points: &[[f64; 3]], faces: &[(Vec<usize>, [f64; 3])], closed: bool) -> Brep {
    let mut records = Records::new();
    let vertices: Vec<usize> = points.iter().map(|point| records.vertex(*point)).collect();
    let mut edges: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for (corners, normal) in faces {
        let mut uses = Vec::new();
        for (index, corner) in corners.iter().enumerate() {
            let next = corners[(index + 1) % corners.len()];
            let key = ((*corner).min(next), (*corner).max(next));
            let edge = *edges
                .entry(key)
                .or_insert_with(|| records.line(vertices[key.0], vertices[key.1]));
            uses.push((edge, *corner == key.0));
        }
        let first = points[corners[0]];
        let center = corners
            .iter()
            .map(|corner| points[*corner])
            .fold([0.0; 3], |sum, point| {
                [sum[0] + point[0], sum[1] + point[1], sum[2] + point[2]]
            })
            .map(|value| {
                value / cadmpeg_core::convert::f64_from_index(corners.len()).unwrap_or(1.0)
            });
        let reference = unit(sub(points[corners[1]], first));
        records.face(
            &[uses],
            AnalyticSurface {
                kind: SurfaceKind::Plane,
                frame: frame(center, *normal, reference),
                params: Vec::new(),
                profile: None,
            },
        );
    }
    records.finish(closed, Axis::Z)
}

/// An axis-aligned cube of edge `size` centered on the origin.
pub(crate) fn cube(size: f64) -> Brep {
    let half = size / 2.0;
    let corner =
        |index: usize| [0, 1, 2].map(|axis| if index >> axis & 1 == 1 { half } else { -half });
    let points: Vec<[f64; 3]> = (0..8).map(corner).collect();
    let faces = [
        (vec![0, 4, 6, 2], [-1.0, 0.0, 0.0]),
        (vec![1, 3, 7, 5], [1.0, 0.0, 0.0]),
        (vec![0, 1, 5, 4], [0.0, -1.0, 0.0]),
        (vec![2, 6, 7, 3], [0.0, 1.0, 0.0]),
        (vec![0, 2, 3, 1], [0.0, 0.0, -1.0]),
        (vec![4, 5, 7, 6], [0.0, 0.0, 1.0]),
    ];
    polyhedron(&points, &faces, true)
}

/// A rectangle of `width` and `length` centered on the origin, normal to
/// `axis`: the width runs along `x`, or `z` when the axis is `x`, and the
/// length along `y`, or `z` when the axis is `y`.
pub(crate) fn plane(width: f64, length: f64, axis: Axis) -> Brep {
    let (normal, along_width) = match axis {
        Axis::X => (X, Z),
        Axis::Y => ([0.0, 1.0, 0.0], X),
        Axis::Z => (Z, X),
    };
    let along_length = cross(normal, along_width);
    let at = |w: f64, l: f64| {
        [0, 1, 2].map(|k| along_width[k] * w * width / 2.0 + along_length[k] * l * length / 2.0)
    };
    let points = [at(-1.0, -1.0), at(1.0, -1.0), at(1.0, 1.0), at(-1.0, 1.0)];
    polyhedron(&points, &[(vec![0, 1, 2, 3], normal)], false)
}

#[cfg(test)]
mod tests;
