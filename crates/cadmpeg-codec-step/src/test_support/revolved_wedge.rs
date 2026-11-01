// SPDX-License-Identifier: Apache-2.0
//! A solid revolved about `+z` over a finite angle, for tests of analytic
//! faces with closed parameter directions.

use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{
    CircleCurve, CylinderSurface, LineCurve, PlaneSurface, SphereSurface, TorusSurface,
};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, IdentityError, LoopId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, EdgeCarrier, Face, FaceLoops, Loop, LoopBoundary, LoopRing,
    Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::CadIr;

/// A point `(rho, z)` of the meridian plane.
pub(crate) type Meridian = [f64; 2];

/// One piece of a profile that runs counterclockwise in `(rho, z)` around the
/// material. Each piece starts where the previous one ends.
#[derive(Clone, Copy)]
pub(crate) enum Piece {
    /// A line of constant `rho` or constant `z`, from its start to `to`.
    Line { to: Meridian },
    /// A circular arc about `center`, from its start to `to`.
    /// `material_inside` states that the material lies inside the circle.
    /// A concave arc gives a torus of negative minor radius, or a sphere of
    /// negative radius when `center` lies on the axis.
    Arc {
        to: Meridian,
        center: Meridian,
        material_inside: bool,
    },
}

fn id<T>(mint: fn(String) -> Result<T, IdentityError>, kind: &str, key: &str) -> T {
    mint(format!("test:model:{kind}#{key}")).expect("identity grammar")
}

fn on_plane(point: Meridian, angle: f64) -> Point3 {
    Point3::new(point[0] * angle.cos(), point[0] * angle.sin(), point[1])
}

fn radial(angle: f64) -> Vector3 {
    Vector3::new(angle.cos(), angle.sin(), 0.0)
}

fn tangential(angle: f64) -> Vector3 {
    Vector3::new(-angle.sin(), angle.cos(), 0.0)
}

struct Builder {
    ir: CadIr,
    profile: Vec<(Meridian, Piece)>,
    angles: [f64; 2],
}

impl Builder {
    fn vertices(&mut self) {
        for (index, (start, _)) in self.profile.clone().into_iter().enumerate() {
            for (side, angle) in self.angles.into_iter().enumerate() {
                let key = format!("{index}-{side}");
                let position =
                    FinitePoint3::new(on_plane(start, angle)).expect("finite profile point");
                self.ir.model.points.push(Point::new(
                    id(PointId::mint, "point", &key),
                    position,
                    None,
                ));
                self.ir.model.vertices.push(Vertex {
                    id: id(VertexId::mint, "vertex", &key),
                    point: id(PointId::mint, "point", &key),
                    tolerance: None,
                });
            }
        }
    }

    fn edge(
        &mut self,
        key: &str,
        geometry: SolvedCurveGeometry,
        range: [f64; 2],
        ends: [String; 2],
    ) {
        self.ir.model.curves.push(Curve {
            parameter_range: None,
            id: id(CurveId::mint, "curve", key),
            geometry: CurveGeometry::Solved(geometry),
            source_object: None,
        });
        let [start, end] = ends;
        self.ir.model.edges.push(Edge {
            id: id(EdgeId::mint, "edge", key),
            carrier: EdgeCarrier::new(Some(id(CurveId::mint, "curve", key)), Some(range))
                .expect("bounded edge"),
            start: id(VertexId::mint, "vertex", &start),
            end: id(VertexId::mint, "vertex", &end),
            tolerance: None,
        });
    }

    /// The circle swept by each profile point, and the piece on each side plane.
    fn edges(&mut self) {
        let [first, last] = self.angles;
        let count = self.profile.len();
        for (index, (start, piece)) in self.profile.clone().into_iter().enumerate() {
            let circle = CircleCurve::try_new(
                Point3::new(0.0, 0.0, start[1]),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                start[0],
            )
            .expect("swept circle");
            self.edge(
                &format!("swept-{index}"),
                SolvedCurveGeometry::Circle(circle),
                [first, last],
                [format!("{index}-0"), format!("{index}-1")],
            );
            for (side, angle) in self.angles.into_iter().enumerate() {
                let (geometry, range) = meridian_curve(start, piece, angle);
                let next = (index + 1) % count;
                self.edge(
                    &format!("meridian-{index}-{side}"),
                    geometry,
                    range,
                    [format!("{index}-{side}"), format!("{next}-{side}")],
                );
            }
        }
    }

    /// One face whose ring is `uses`, in loop order.
    fn face(
        &mut self,
        key: &str,
        carrier: SolvedSurfaceGeometry,
        sense: Sense,
        uses: &[(String, Sense)],
    ) {
        self.ir.model.surfaces.push(Surface {
            id: id(SurfaceId::mint, "surface", key),
            geometry: SurfaceGeometry::Solved(carrier),
            source_object: None,
        });
        let mut ring = Vec::with_capacity(uses.len());
        for (edge, use_sense) in uses {
            let coedge = format!("{key}-{edge}");
            ring.push(id(CoedgeId::mint, "coedge", &coedge));
            self.ir.model.coedges.push(Coedge {
                id: id(CoedgeId::mint, "coedge", &coedge),
                owner_loop: id(LoopId::mint, "loop", key),
                edge: id(EdgeId::mint, "edge", edge),
                radial_next: id(CoedgeId::mint, "coedge", &coedge),
                sense: *use_sense,
                pcurves: Vec::new(),
                use_curve: None,
            });
        }
        self.ir.model.loops.push(Loop {
            id: id(LoopId::mint, "loop", key),
            face: id(FaceId::mint, "face", key),
            boundary: LoopBoundary::Ring(LoopRing::new(ring, Vec::new()).expect("non-empty ring")),
        });
        self.ir.model.faces.push(Face {
            id: id(FaceId::mint, "face", key),
            shell: id(ShellId::mint, "shell", "wedge"),
            surface: id(SurfaceId::mint, "surface", key),
            sense,
            loops: FaceLoops::classified(id(LoopId::mint, "loop", key), Vec::new()),
            name: Some(key.to_owned()),
            color: None,
            tolerance: None,
        });
    }

    fn faces(&mut self) {
        let count = self.profile.len();
        for (index, (start, piece)) in self.profile.clone().into_iter().enumerate() {
            let (carrier, sense) = swept_surface(start, piece);
            self.face(
                &format!("swept-{index}"),
                carrier,
                sense,
                &[
                    (format!("swept-{index}"), Sense::Forward),
                    (format!("meridian-{index}-1"), Sense::Forward),
                    (format!("swept-{}", (index + 1) % count), Sense::Reversed),
                    (format!("meridian-{index}-0"), Sense::Reversed),
                ],
            );
        }
        for (side, angle) in self.angles.into_iter().enumerate() {
            let plane =
                PlaneSurface::try_new(Point3::new(0.0, 0.0, 0.0), tangential(angle), radial(angle))
                    .expect("side plane");
            let (sense, use_sense, order): (_, _, Vec<_>) = if side == 0 {
                (Sense::Reversed, Sense::Forward, (0..count).collect())
            } else {
                (Sense::Forward, Sense::Reversed, (0..count).rev().collect())
            };
            let uses = order
                .into_iter()
                .map(|index| (format!("meridian-{index}-{side}"), use_sense))
                .collect::<Vec<_>>();
            self.face(
                &format!("side-{side}"),
                SolvedSurfaceGeometry::Plane(plane),
                sense,
                &uses,
            );
        }
    }

    fn body(&mut self) {
        let faces = self
            .ir
            .model
            .faces
            .iter()
            .map(|face| face.id.clone())
            .collect();
        self.ir.model.shells.push(
            Shell::new(
                id(ShellId::mint, "shell", "wedge"),
                id(RegionId::mint, "region", "wedge"),
                faces,
                Vec::new(),
                Vec::new(),
            )
            .expect("shell with faces"),
        );
        self.ir.model.regions.push(Region {
            id: id(RegionId::mint, "region", "wedge"),
            body: id(BodyId::mint, "body", "wedge"),
            shells: vec![id(ShellId::mint, "shell", "wedge")],
        });
        self.ir.model.bodies.push(Body {
            id: id(BodyId::mint, "body", "wedge"),
            kind: BodyKind::Solid,
            regions: vec![id(RegionId::mint, "region", "wedge")],
            transform: None,
            name: Some("wedge".into()),
            color: None,
            visible: None,
        });
    }
}

/// The curve of `piece` on the plane at `angle`, and its range from the
/// piece's start to its end.
fn meridian_curve(start: Meridian, piece: Piece, angle: f64) -> (SolvedCurveGeometry, [f64; 2]) {
    let from = on_plane(start, angle);
    match piece {
        Piece::Line { to } => {
            let vector = on_plane(to, angle).vector_from(from);
            let length = vector.norm();
            let line = LineCurve::try_new(from, vector.scale(1.0 / length)).expect("profile line");
            (SolvedCurveGeometry::Line(line), [0.0, length])
        }
        Piece::Arc {
            to,
            center,
            material_inside,
        } => {
            let hub = on_plane(center, angle);
            let reference = from.vector_from(hub);
            let radius = reference.norm();
            // `rho × z` is `-tangential`, so a counterclockwise profile arc
            // turns about `-tangential`.
            let normal = if material_inside {
                tangential(angle).scale(-1.0)
            } else {
                tangential(angle)
            };
            let circle = CircleCurve::try_new(hub, normal, reference.scale(1.0 / radius), radius)
                .expect("profile arc");
            let begin = (start[1] - center[1]).atan2(start[0] - center[0]);
            let end = (to[1] - center[1]).atan2(to[0] - center[0]);
            let turn = if material_inside {
                (end - begin).rem_euclid(std::f64::consts::TAU)
            } else {
                (begin - end).rem_euclid(std::f64::consts::TAU)
            };
            (SolvedCurveGeometry::Circle(circle), [0.0, turn])
        }
    }
}

/// The surface `piece` sweeps and the face sense on it. The material's
/// outward normal lies to the right of the profile.
fn swept_surface(start: Meridian, piece: Piece) -> (SolvedSurfaceGeometry, Sense) {
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);
    let by = |forward: bool| {
        if forward {
            Sense::Forward
        } else {
            Sense::Reversed
        }
    };
    match piece {
        Piece::Line { to } => {
            let [along_rho, along_z] = [to[0] - start[0], to[1] - start[1]];
            if along_z == 0.0 {
                let plane = PlaneSurface::try_new(Point3::new(0.0, 0.0, start[1]), axis, reference)
                    .expect("swept plane");
                (SolvedSurfaceGeometry::Plane(plane), by(along_rho < 0.0))
            } else {
                assert_eq!(along_rho, 0.0, "a profile line has constant rho or z");
                let cylinder =
                    CylinderSurface::try_new(Point3::new(0.0, 0.0, 0.0), axis, reference, start[0])
                        .expect("swept cylinder");
                (SolvedSurfaceGeometry::Cylinder(cylinder), by(along_z > 0.0))
            }
        }
        Piece::Arc {
            center,
            material_inside,
            ..
        } => {
            let magnitude = (start[0] - center[0]).hypot(start[1] - center[1]);
            let signed = if material_inside {
                magnitude
            } else {
                -magnitude
            };
            let hub = Point3::new(0.0, 0.0, center[1]);
            if center[0] == 0.0 {
                // `Su × Sv` of a sphere points away from its center for a
                // positive radius and toward it for a negative one.
                let sphere = SphereSurface::try_new(hub, axis, reference, signed).expect("sphere");
                (SolvedSurfaceGeometry::Sphere(sphere), Sense::Forward)
            } else {
                // `Su × Sv` of a torus points away from the tube center for
                // either sign of the minor radius.
                let torus =
                    TorusSurface::try_new(hub, axis, reference, center[0], signed).expect("torus");
                (SolvedSurfaceGeometry::Torus(torus), by(material_inside))
            }
        }
    }
}

/// The solid that the closed `profile` sweeps from `angles[0]` to `angles[1]`.
pub(crate) fn revolved_wedge(start: Meridian, pieces: &[Piece], angles: [f64; 2]) -> CadIr {
    let mut profile = Vec::with_capacity(pieces.len());
    let mut at = start;
    for piece in pieces {
        profile.push((at, *piece));
        at = match piece {
            Piece::Line { to } | Piece::Arc { to, .. } => *to,
        };
    }
    assert_eq!(at, start, "the profile closes");
    let mut builder = Builder {
        ir: CadIr::empty(),
        profile,
        angles,
    };
    builder.vertices();
    builder.edges();
    builder.faces();
    builder.body();
    let mut ir = builder.ir;
    pair_radial_uses(&mut ir);
    ir.finalize();
    ir
}

/// Each edge has two uses; each names the other as its radial partner.
fn pair_radial_uses(ir: &mut CadIr) {
    let mut first_use = std::collections::HashMap::new();
    for index in 0..ir.model.coedges.len() {
        let edge = ir.model.coedges[index].edge.clone();
        if let Some(other) = first_use.insert(edge, index) {
            let [left, right] = [other, index].map(|at: usize| ir.model.coedges[at].id.clone());
            ir.model.coedges[other].radial_next = right;
            ir.model.coedges[index].radial_next = left;
        }
    }
}
