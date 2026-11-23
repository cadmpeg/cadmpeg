// SPDX-License-Identifier: Apache-2.0
//! Bodies of revolution for the `Revolve` tests: the solid that a closed
//! polygon of the `(r, z)` half-plane sweeps in a full turn about an axis
//! through the origin.

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{CircleCurve, CylinderSurface, LineCurve, PlaneSurface};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, IdentityError, LoopId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, EdgeCarrier, EdgeEnds, Face, FaceLoops, Loop, LoopBoundary,
    LoopRing, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_test_support::solids::closed_edge;

/// The identity `closed_edge` mints for `key`.
fn id<T>(mint: fn(String) -> Result<T, IdentityError>, kind: &str, key: &str) -> T {
    mint(format!("test:model:{kind}#{key}")).expect("identity grammar")
}

/// A coedge of a face's loop: its key, its edge and its sense.
type Use = (String, String, Sense);

/// [`revolved_about`] `+z`, with `r` along `+x`.
pub(super) fn revolved(corners: &[(f64, f64)]) -> CadIr {
    revolved_about(
        corners,
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    )
}

/// The solid that the closed polygon `corners` of `(r, z)` points, in mm and
/// counterclockwise in the half-plane `r ≥ 0`, sweeps in a full turn about
/// the unit `axis` through the origin, `z` measured along `axis` and `r`
/// along the unit `reference` across it. Each side with both ends on the
/// axis makes no face; a side of constant `z` makes a plane, a side of
/// constant `r` a cylinder with a seam at angle 0. A corner off the axis
/// makes a circle edge.
pub(super) fn revolved_about(corners: &[(f64, f64)], axis: Vector3, reference: Vector3) -> CadIr {
    let place = |r: f64, z: f64| {
        Point3::new(
            r * reference.x + z * axis.x,
            r * reference.y + z * axis.y,
            r * reference.z + z * axis.z,
        )
    };
    let mut ir = CadIr::empty();
    let count = corners.len();
    let circle = |at: usize| format!("circle{at}");
    for (at, &(r, z)) in corners.iter().enumerate() {
        if r <= 0.0 {
            continue;
        }
        let key = circle(at);
        let position = FinitePoint3::new(place(r, z)).expect("finite position");
        ir.geometry
            .points
            .push(Point::new(id(PointId::mint, "point", &key), position, None));
        ir.geometry.vertices.push(Vertex {
            id: id(VertexId::mint, "vertex", &key),
            point: id(PointId::mint, "point", &key),
            tolerance: None,
            native_ref: None,
        });
        ir.geometry.curves.push(Curve {
            parameter_range: None,
            id: id(CurveId::mint, "curve", &key),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                CircleCurve::try_new(place(0.0, z), axis, reference, r).expect("circle"),
            )),
            source_object: None,
            native_ref: None,
        });
        ir.geometry.edges.push(closed_edge(&key, &key, &key));
    }
    // Each face with its loops, each loop a ring of coedge uses.
    let mut faces: Vec<(String, Sense, Vec<Vec<Use>>)> = Vec::new();
    for at in 0..count {
        let next = (at + 1) % count;
        let ((ra, za), (rb, zb)) = (corners[at], corners[next]);
        let face = format!("side{at}");
        if ra <= 0.0 && rb <= 0.0 {
            continue;
        }
        if (za - zb).abs() <= f64::EPSILON {
            // Outward normal (dz, -dr) of the counterclockwise polygon:
            // `-z` where r grows, `+z` where it shrinks.
            let up = rb < ra;
            ir.geometry.surfaces.push(Surface {
                id: id(SurfaceId::mint, "surface", &face),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    PlaneSurface::try_new(place(0.0, za), axis, reference).expect("plane"),
                )),
                source_object: None,
                native_ref: None,
            });
            let sense = if up { Sense::Forward } else { Sense::Reversed };
            let mut loops = Vec::new();
            for corner in [at, next] {
                if corners[corner].0 > 0.0 {
                    loops.push(vec![(
                        format!("{face}-{}", circle(corner)),
                        circle(corner),
                        sense,
                    )]);
                }
            }
            loops.sort_by(|a, b| {
                let radius = |ring: &Vec<Use>| {
                    let corner = ring[0].1.trim_start_matches("circle");
                    corners[corner.parse::<usize>().expect("corner")].0
                };
                radius(b).total_cmp(&radius(a))
            });
            faces.push((face, sense, loops));
        } else {
            assert!(
                (ra - rb).abs() <= f64::EPSILON && ra > 0.0,
                "a side off the axis of constant r"
            );
            let outward = zb > za;
            ir.geometry.surfaces.push(Surface {
                id: id(SurfaceId::mint, "surface", &face),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                    CylinderSurface::try_new(place(0.0, 0.0), axis, reference, ra)
                        .expect("cylinder"),
                )),
                source_object: None,
                native_ref: None,
            });
            let seam = format!("seam{at}");
            let (low, high) = if outward { (at, next) } else { (next, at) };
            ir.geometry.curves.push(Curve {
                parameter_range: None,
                id: id(CurveId::mint, "curve", &seam),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                    LineCurve::try_new(place(ra, corners[low].1), axis).expect("line"),
                )),
                source_object: None,
                native_ref: None,
            });
            ir.geometry.edges.push(Edge {
                id: id(EdgeId::mint, "edge", &seam),
                carrier: EdgeCarrier::new(
                    Some(id(CurveId::mint, "curve", &seam)),
                    Some([0.0, corners[high].1 - corners[low].1]),
                )
                .expect("bounded seam"),
                ends: Some(EdgeEnds::new(
                    id(VertexId::mint, "vertex", &circle(low)),
                    id(VertexId::mint, "vertex", &circle(high)),
                )),
                tolerance: None,
                native_ref: None,
            });
            let (sense, low_sense, high_sense) = if outward {
                (Sense::Forward, Sense::Forward, Sense::Reversed)
            } else {
                (Sense::Reversed, Sense::Reversed, Sense::Forward)
            };
            let ring = vec![
                (format!("{face}-{}", circle(low)), circle(low), low_sense),
                (format!("{face}-up"), seam.clone(), Sense::Forward),
                (format!("{face}-{}", circle(high)), circle(high), high_sense),
                (format!("{face}-down"), seam, Sense::Reversed),
            ];
            faces.push((face, sense, vec![ring]));
        }
    }
    // The plane coedge on a circle runs against the cylinder coedge on it.
    let mut uses = std::collections::HashMap::<String, Vec<(String, Sense)>>::new();
    for (face, _, loops) in &faces {
        for ring in loops {
            for (coedge, edge, sense) in ring {
                if !face_is_plane(&ir, face) {
                    uses.entry(edge.clone())
                        .or_default()
                        .push((coedge.clone(), *sense));
                }
            }
        }
    }
    for (face, _, loops) in &mut faces {
        let plane = face_is_plane(&ir, face);
        for ring in loops {
            for (coedge, edge, sense) in ring.iter_mut() {
                if plane {
                    if let Some((_, other)) = uses.get(edge.as_str()).and_then(|of| of.first()) {
                        *sense = match other {
                            Sense::Forward => Sense::Reversed,
                            Sense::Reversed => Sense::Forward,
                        };
                    }
                    uses.entry(edge.clone())
                        .or_default()
                        .push((coedge.clone(), *sense));
                }
            }
        }
    }
    for (face, sense, loops) in &faces {
        let mut keys = Vec::new();
        for (index, ring) in loops.iter().enumerate() {
            let key = format!("{face}-loop{index}");
            for (coedge, edge, coedge_sense) in ring {
                let partner = uses[edge]
                    .iter()
                    .find(|(other, _)| other != coedge)
                    .map_or_else(|| coedge.clone(), |(other, _)| other.clone());
                ir.geometry.coedges.push(Coedge {
                    id: id(CoedgeId::mint, "coedge", coedge),
                    owner_loop: id(LoopId::mint, "loop", &key),
                    edge: id(EdgeId::mint, "edge", edge),
                    radial_next: id(CoedgeId::mint, "coedge", &partner),
                    sense: *coedge_sense,
                    pcurves: Vec::new(),
                    use_curve: None,
                    native_ref: None,
                });
            }
            ir.geometry.loops.push(Loop {
                id: id(LoopId::mint, "loop", &key),
                face: id(FaceId::mint, "face", face),
                boundary: LoopBoundary::Ring(
                    LoopRing::new(
                        ring.iter()
                            .map(|(coedge, _, _)| id(CoedgeId::mint, "coedge", coedge))
                            .collect(),
                        Vec::new(),
                    )
                    .expect("non-empty ring"),
                ),
                native_ref: None,
            });
            keys.push(id(LoopId::mint, "loop", &key));
        }
        let mut keys = keys.into_iter();
        let outer = keys.next().expect("a face has a loop");
        ir.geometry.faces.push(Face {
            id: id(FaceId::mint, "face", face),
            shell: id(ShellId::mint, "shell", "solid"),
            surface: id(SurfaceId::mint, "surface", face),
            sense: *sense,
            loops: FaceLoops::classified(outer, keys.collect()),
            name: Some(face.clone()),
            color: None,
            tolerance: None,
            native_ref: None,
        });
    }
    ir.geometry.shells.push(Shell::new(
        id(ShellId::mint, "shell", "solid"),
        id(RegionId::mint, "region", "solid"),
    ));
    ir.geometry.regions.push(Region {
        id: id(RegionId::mint, "region", "solid"),
        body: id(BodyId::mint, "body", "solid"),
        shells: vec![id(ShellId::mint, "shell", "solid")],
        native_ref: None,
    });
    ir.geometry.bodies.push(Body {
        id: id(BodyId::mint, "body", "solid"),
        kind: BodyKind::Solid,
        transform: None,
        name: Some("solid".into()),
        color: None,
        visible: None,
        native_ref: None,
    });
    ir.finalize();
    ir
}

fn face_is_plane(ir: &CadIr, face: &str) -> bool {
    ir.geometry.surfaces.iter().any(|surface| {
        surface.id == id(SurfaceId::mint, "surface", face)
            && matches!(
                surface.geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
            )
    })
}

/// A body of one face on each of `surfaces`, with no edges: enough for the
/// face rows of the plan, which read the faces' surfaces alone.
pub(super) fn faces_only(surfaces: Vec<SolvedSurfaceGeometry>) -> CadIr {
    let mut ir = revolved(&[(2.0, 0.0), (4.0, 0.0), (4.0, 6.0), (2.0, 6.0)]);
    ir.geometry.coedges.clear();
    ir.geometry.loops.clear();
    ir.geometry.edges.clear();
    ir.geometry.curves.clear();
    ir.geometry.vertices.clear();
    ir.geometry.points.clear();
    ir.geometry.faces.clear();
    ir.geometry.surfaces.clear();
    for (at, geometry) in surfaces.into_iter().enumerate() {
        let key = format!("only{at}");
        ir.geometry.surfaces.push(Surface {
            id: id(SurfaceId::mint, "surface", &key),
            geometry: SurfaceGeometry::Solved(geometry),
            source_object: None,
            native_ref: None,
        });
        ir.geometry.faces.push(Face {
            id: id(FaceId::mint, "face", &key),
            shell: id(ShellId::mint, "shell", "solid"),
            surface: id(SurfaceId::mint, "surface", &key),
            sense: Sense::Forward,
            loops: FaceLoops::unspecified(Vec::new()),
            name: None,
            color: None,
            tolerance: None,
            native_ref: None,
        });
    }
    ir
}
