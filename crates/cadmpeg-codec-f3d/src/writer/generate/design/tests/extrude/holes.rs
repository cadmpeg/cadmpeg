// SPDX-License-Identifier: Apache-2.0
//! The `Extrude` of a profile that is one region with circular holes
//! (f3d.md §3.1, "Profile with circular holes", "Tracked results of a
//! profile with circular holes" and "Sketch links of the start cap").

use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::design::ops::sketch::PlanarCurve;
use cadmpeg_ir::design::parameter::ParameterValue;
use cadmpeg_ir::design::selection::PrincipalEntity;
use cadmpeg_ir::design::units::UnitSymbol;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::extents::{ExtrudeExtent, LinearTermination};
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::features::profiles::{PlanarProfileRef, ProfileRef};
use cadmpeg_ir::geometry::analytic::{CircleCurve, CylinderSurface, LineCurve};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry as ModelCurve, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    CoedgeId, CurveId, EdgeId, FaceId, IdentityError, LoopId, PointId, SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::PositiveLength;
use cadmpeg_ir::topology::{
    Coedge, Edge, EdgeCarrier, EdgeEnds, Face, FaceLoops, Loop, LoopBoundary, LoopRing, Point,
    Sense, Vertex,
};
use cadmpeg_test_support::solids::{closed_edge, solid_cylinder, CYLINDER_HEIGHT, CYLINDER_RADIUS};

use super::super::super::extrude::profile::{profile, LoopEntry, ProfileLoop};
use super::super::super::sketch::{CurveGeometry, PlannedCurve, PlannedSketch};
use super::super::roster::segment;
use super::super::sketch::{feature_id, frame, picked, point2, principal, sketch_id, Source};
use super::super::{loss, neutral, number, parameter_id, write, write_bytes};
use super::{
    blind, decoded_extrude, entry, kinds, planned, reference_at, sketch_and_extrude_of, square,
    u32_at, u64_at, SIDE, TOLERANCE,
};

/// `lines` (see [`planned`]) then one circle per `(center, radius)`, in cm.
pub(super) fn with_circles<'a>(
    id: &'a cadmpeg_ir::features::FeatureId,
    sketch: &'a cadmpeg_ir::sketches::SketchId,
    lines: &[([f64; 2], [f64; 2])],
    circles: &[([f64; 2], f64)],
) -> PlannedSketch<'a> {
    let mut planned = planned(id, sketch, lines);
    for (center, radius) in circles {
        planned.points.push(*center);
        planned.curves.push(PlannedCurve {
            geometry: CurveGeometry::Circle {
                center: planned.points.len() - 1,
                radius: *radius,
            },
            construction: false,
        });
    }
    planned
}

fn exterior(entries: Vec<LoopEntry>) -> ProfileLoop {
    ProfileLoop {
        exterior: true,
        entries,
    }
}

/// A hole of one circle, run against its direction: traversal flag `1`.
fn hole(curve: usize) -> ProfileLoop {
    ProfileLoop {
        exterior: false,
        entries: vec![entry(curve, true)],
    }
}

#[test]
fn a_tube_profile_is_one_region_with_one_hole() {
    let id = feature_id("tube");
    let sketch = sketch_id("tube");
    let outer_first = with_circles(&id, &sketch, &[], &[([0.0, 0.0], 2.0), ([0.0, 0.0], 1.0)]);
    assert_eq!(
        profile(&outer_first, TOLERANCE),
        Ok(vec![exterior(vec![entry(0, false)]), hole(1)])
    );
    let inner_first = with_circles(&id, &sketch, &[], &[([0.0, 0.0], 1.0), ([0.0, 0.0], 2.0)]);
    assert_eq!(
        profile(&inner_first, TOLERANCE),
        Ok(vec![exterior(vec![entry(1, false)]), hole(0)])
    );
    assert_eq!(hole(0).entries[0].traversal_flag(), 1);
}

#[test]
fn a_square_with_a_round_hole_runs_the_walk_then_the_hole() {
    let id = feature_id("plate");
    let sketch = sketch_id("plate");
    let (a, b, c, d) = ([0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]);
    let plate = with_circles(
        &id,
        &sketch,
        &[(a, b), (b, c), (c, d), (d, a)],
        &[([2.0, 2.0], 1.0)],
    );
    assert_eq!(
        profile(&plate, TOLERANCE),
        Ok(vec![
            exterior(vec![
                entry(3, false),
                entry(0, false),
                entry(1, false),
                entry(2, false)
            ]),
            hole(4)
        ])
    );
}

#[test]
fn a_circle_with_two_holes_stores_the_holes_in_curve_order() {
    let id = feature_id("disc");
    let sketch = sketch_id("disc");
    let disc = with_circles(
        &id,
        &sketch,
        &[],
        &[([1.5, 0.0], 0.5), ([0.0, 0.0], 3.0), ([-1.5, 0.0], 0.5)],
    );
    assert_eq!(
        profile(&disc, TOLERANCE),
        Ok(vec![exterior(vec![entry(1, false)]), hole(0), hole(2)])
    );
}

#[test]
fn profiles_outside_one_region_are_refused() {
    let id = feature_id("refused");
    let sketch = sketch_id("refused");
    let (a, b, c, d) = ([-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]);
    let square = [(a, b), (b, c), (c, d), (d, a)];
    let cases = [
        (
            with_circles(&id, &sketch, &[], &[([0.0, 0.0], 1.0), ([5.0, 0.0], 1.0)]),
            "its profile has several regions",
        ),
        (
            with_circles(
                &id,
                &sketch,
                &[],
                &[([0.0, 0.0], 3.0), ([0.0, 0.0], 2.0), ([0.0, 0.0], 1.0)],
            ),
            "its profile has a loop inside a hole",
        ),
        (
            with_circles(&id, &sketch, &square[..3], &[]),
            "its profile is not closed",
        ),
        (
            with_circles(&id, &sketch, &[], &[]),
            "its profile has no curves",
        ),
    ];
    for (sketch, reason) in &cases {
        assert_eq!(profile(sketch, TOLERANCE), Err(*reason));
    }
}

/// Two loops whose boundaries cross or touch bound a region whose boundary
/// is no loop of whole curves, so "Profile with circular holes" does not
/// describe it.
#[test]
fn loops_that_cross_or_touch_are_refused() {
    let id = feature_id("meet");
    let sketch = sketch_id("meet");
    let (a, b, c, d) = ([-5.0, -5.0], [5.0, -5.0], [5.0, 5.0], [-5.0, 5.0]);
    let square = [(a, b), (b, c), (c, d), (d, a)];
    let outer = ([0.0, 0.0], 10.0);
    let holes_meet = [
        // Two holes that cross.
        with_circles(
            &id,
            &sketch,
            &[],
            &[outer, ([-1.0, 0.0], 2.0), ([1.0, 0.0], 2.0)],
        ),
        // Two holes that touch.
        with_circles(
            &id,
            &sketch,
            &[],
            &[outer, ([-2.0, 0.0], 2.0), ([2.0, 0.0], 2.0)],
        ),
        // Two holes that touch within the tolerance.
        with_circles(
            &id,
            &sketch,
            &[],
            &[
                outer,
                ([-2.0, 0.0], 2.0),
                ([2.0 + TOLERANCE / 2.0, 0.0], 2.0),
            ],
        ),
        // The same hole twice.
        with_circles(
            &id,
            &sketch,
            &[],
            &[outer, ([1.0, 0.0], 2.0), ([1.0, 0.0], 2.0)],
        ),
        // Two holes of a square that cross.
        with_circles(
            &id,
            &sketch,
            &square,
            &[([-1.0, 0.0], 2.0), ([1.0, 0.0], 2.0)],
        ),
    ];
    let loops_meet = [
        // A hole that touches the outer circle from inside.
        with_circles(&id, &sketch, &[], &[([0.0, 0.0], 5.0), ([3.0, 0.0], 2.0)]),
        // A hole closer to the outer circle than the tolerance.
        with_circles(
            &id,
            &sketch,
            &[],
            &[([0.0, 0.0], 5.0), ([3.0 - TOLERANCE / 2.0, 0.0], 2.0)],
        ),
        // A hole that crosses the outer circle.
        with_circles(&id, &sketch, &[], &[([0.0, 0.0], 2.0), ([1.5, 0.0], 1.0)]),
        // A hole that touches an edge of the square.
        with_circles(&id, &sketch, &square, &[([3.0, 0.0], 2.0)]),
        // A hole that crosses an edge of the square.
        with_circles(&id, &sketch, &square, &[([4.0, 0.0], 2.0)]),
    ];
    let refusals = |cases: &[PlannedSketch<'_>]| {
        cases
            .iter()
            .map(|case| profile(case, TOLERANCE))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        (refusals(&holes_meet), refusals(&loops_meet)),
        (
            vec![Err("holes of its profile meet"); holes_meet.len()],
            vec![Err("loops of its profile meet"); loops_meet.len()]
        )
    );
}

pub(super) fn mint<T>(mint: fn(String) -> Result<T, IdentityError>, kind: &str, key: &str) -> T {
    mint(format!("test:model:{kind}#{key}")).expect("identity grammar")
}

/// The planar face of `ir` normal to z at height `z` mm.
fn cap(ir: &CadIr, z: f64) -> FaceId {
    ir.geometry
        .faces
        .iter()
        .find(|face| {
            ir.geometry.surfaces.iter().any(|surface| {
                surface.id == face.surface
                    && matches!(
                        surface.geometry.solved(),
                        Some(SolvedSurfaceGeometry::Plane(plane))
                            if (plane.frame().axis().as_raw().z.abs() - 1.0).abs() <= TOLERANCE
                                && (plane.origin().get().z - z).abs() <= TOLERANCE
                    )
            })
        })
        .map(|face| face.id.clone())
        .expect("a cap face")
}

/// Drills a round hole of `radius` mm about `(center, z)` through the prism
/// of `ir` from `z = 0` to `height`: a cylindrical side face whose seam is
/// at `+x`, and an inner loop on each cap.
fn drill(ir: &mut CadIr, key: &str, center: [f64; 2], radius: f64, height: f64) {
    let (bottom, top) = (cap(ir, 0.0), cap(ir, height));
    let shell = ir
        .geometry
        .faces
        .iter()
        .find(|face| face.id == bottom)
        .map(|face| face.shell.clone())
        .expect("the bottom cap");
    let name = |part: &str| format!("{key}-{part}");
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);
    let seam = [center[0] + radius, center[1]];
    let geometry = &mut ir.geometry;
    for (part, z) in [("bottom", 0.0), ("top", height)] {
        let part = name(part);
        geometry.points.push(Point::new(
            mint(PointId::mint, "point", &part),
            FinitePoint3::new(Point3::new(seam[0], seam[1], z)).expect("a finite point"),
            None,
        ));
        geometry.vertices.push(Vertex {
            id: mint(VertexId::mint, "vertex", &part),
            point: mint(PointId::mint, "point", &part),
            tolerance: None,
            native_ref: None,
        });
        geometry.curves.push(Curve {
            parameter_range: None,
            id: mint(CurveId::mint, "curve", &part),
            geometry: ModelCurve::Solved(SolvedCurveGeometry::Circle(
                CircleCurve::try_new(
                    Point3::new(center[0], center[1], z),
                    axis,
                    reference,
                    radius,
                )
                .expect("a circle"),
            )),
            source_object: None,
            native_ref: None,
        });
        geometry.edges.push(closed_edge(&part, &part, &part));
    }
    let seam_key = name("seam");
    geometry.curves.push(Curve {
        parameter_range: None,
        id: mint(CurveId::mint, "curve", &seam_key),
        geometry: ModelCurve::Solved(SolvedCurveGeometry::Line(
            LineCurve::try_new(Point3::new(seam[0], seam[1], 0.0), axis).expect("a line"),
        )),
        source_object: None,
        native_ref: None,
    });
    geometry.edges.push(Edge {
        id: mint(EdgeId::mint, "edge", &seam_key),
        carrier: EdgeCarrier::new(
            Some(mint(CurveId::mint, "curve", &seam_key)),
            Some([0.0, height]),
        )
        .expect("a bounded seam"),
        ends: Some(EdgeEnds::new(
            mint(VertexId::mint, "vertex", &name("bottom")),
            mint(VertexId::mint, "vertex", &name("top")),
        )),
        tolerance: None,
        native_ref: None,
    });
    let side = name("side");
    geometry.surfaces.push(Surface {
        id: mint(SurfaceId::mint, "surface", &side),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            CylinderSurface::try_new(
                Point3::new(center[0], center[1], 0.0),
                axis,
                reference,
                radius,
            )
            .expect("a cylinder"),
        )),
        source_object: None,
        native_ref: None,
    });
    // (coedge, loop, edge, sense, radial partner). The side face faces the
    // axis; each cap's inner loop runs its circle opposite to the side.
    let coedges = [
        (
            "side-bottom",
            "side",
            "bottom",
            Sense::Reversed,
            "cap-bottom",
        ),
        (
            "side-seam-up",
            "side",
            "seam",
            Sense::Forward,
            "side-seam-down",
        ),
        ("side-top", "side", "top", Sense::Forward, "cap-top"),
        (
            "side-seam-down",
            "side",
            "seam",
            Sense::Reversed,
            "side-seam-up",
        ),
        (
            "cap-bottom",
            "cap-bottom",
            "bottom",
            Sense::Forward,
            "side-bottom",
        ),
        ("cap-top", "cap-top", "top", Sense::Reversed, "side-top"),
    ];
    for (coedge, owner, edge, sense, partner) in coedges {
        geometry.coedges.push(Coedge {
            id: mint(CoedgeId::mint, "coedge", &name(coedge)),
            owner_loop: mint(LoopId::mint, "loop", &name(owner)),
            edge: mint(EdgeId::mint, "edge", &name(edge)),
            radial_next: mint(CoedgeId::mint, "coedge", &name(partner)),
            sense,
            pcurves: Vec::new(),
            use_curve: None,
            native_ref: None,
        });
    }
    let loops = [
        (
            "side",
            mint(FaceId::mint, "face", &side),
            &["side-bottom", "side-seam-up", "side-top", "side-seam-down"][..],
        ),
        ("cap-bottom", bottom.clone(), &["cap-bottom"][..]),
        ("cap-top", top.clone(), &["cap-top"][..]),
    ];
    for (owner, face, ring) in loops {
        geometry.loops.push(Loop {
            id: mint(LoopId::mint, "loop", &name(owner)),
            face,
            boundary: LoopBoundary::Ring(
                LoopRing::new(
                    ring.iter()
                        .map(|coedge| mint(CoedgeId::mint, "coedge", &name(coedge)))
                        .collect(),
                    Vec::new(),
                )
                .expect("a ring"),
            ),
            native_ref: None,
        });
    }
    geometry.faces.push(Face {
        id: mint(FaceId::mint, "face", &side),
        shell,
        surface: mint(SurfaceId::mint, "surface", &side),
        sense: Sense::Reversed,
        loops: FaceLoops::classified(mint(LoopId::mint, "loop", &side), Vec::new()),
        name: Some(side.clone()),
        color: None,
        tolerance: None,
        native_ref: None,
    });
    for (face, inner) in [(&bottom, "cap-bottom"), (&top, "cap-top")] {
        let face = geometry
            .faces
            .iter_mut()
            .find(|candidate| &candidate.id == face)
            .expect("a cap");
        let mut loops = face.loops.stated().cloned().collect::<Vec<_>>();
        let outer = loops.remove(0);
        loops.push(mint(LoopId::mint, "loop", &name(inner)));
        face.loops = FaceLoops::classified(outer, loops);
    }
    ir.finalize();
}

/// The cube's square with a round hole of radius 2 mm at its centre.
fn plate() -> CadIr {
    let mut profile = square();
    profile.entity(
        "hole",
        PlanarCurve::Circle {
            center: point2(SIDE / 2.0, SIDE / 2.0),
            radius: PositiveLength::new(2.0).expect("a radius"),
        },
        &[],
    );
    let mut ir = sketch_and_extrude_of(profile, blind(), false);
    drill(&mut ir, "hole", [SIDE / 2.0, SIDE / 2.0], 2.0, SIDE);
    ir
}

/// The radius of the holes of [`tube`] and [`disc`], in mm.
const HOLE_RADIUS: f64 = 4.0;

/// The solid cylinder of `cadmpeg_test_support` as `Sketch` (its bottom
/// circle and one circle for each of `holes`) and `Extrude`, with the
/// holes drilled through it.
fn cylinder_with_holes(holes: &[[f64; 2]], radius: f64) -> CadIr {
    let mut profile = Source::new(
        "profile",
        picked(principal(PrincipalEntity::XyPlane)),
        frame([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    );
    profile.entity(
        "outer",
        PlanarCurve::Circle {
            center: point2(0.0, 0.0),
            radius: PositiveLength::new(CYLINDER_RADIUS).expect("a radius"),
        },
        &[],
    );
    for (index, center) in holes.iter().enumerate() {
        profile.entity(
            &format!("hole{index}"),
            PlanarCurve::Circle {
                center: point2(center[0], center[1]),
                radius: PositiveLength::new(radius).expect("a radius"),
            },
            &[],
        );
    }
    let mut ir = sketch_and_extrude_of(profile, blind(), false);
    let cylinder = solid_cylinder();
    let body = cylinder.geometry.bodies[0].id.clone();
    ir.geometry = cylinder.geometry;
    ir.design.scopes[0].current[0].bodies = vec![body];
    let depth = ir
        .design
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == parameter_id("d1"))
        .expect("the extrude's depth");
    depth.definition = neutral(number(CYLINDER_HEIGHT, Some(UnitSymbol::Mm)));
    depth.value = Some(ParameterValue::Number(crate::test_support::real(
        CYLINDER_HEIGHT,
    )));
    for (index, center) in holes.iter().enumerate() {
        drill(
            &mut ir,
            &format!("hole{index}"),
            *center,
            radius,
            CYLINDER_HEIGHT,
        );
    }
    ir
}

fn tube() -> CadIr {
    cylinder_with_holes(&[[0.0, 0.0]], HOLE_RADIUS)
}

/// The cylinder with two holes, the first written at `+x`.
fn disc() -> CadIr {
    cylinder_with_holes(&[[5.0, 0.0], [-5.0, 0.0]], 2.0)
}

/// The decoded Extrude's one-sided blind depth, in mm.
pub(super) fn depth(decoded: &CadIr) -> f64 {
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Extrude {
            profile, extent, ..
        },
    ) = decoded_extrude(decoded).evaluation.definition()
    else {
        panic!("an Extrude");
    };
    assert!(
        matches!(
            profile,
            ProfileRef::Planar(
                PlanarProfileRef::Sketch(_)
                    | PlanarProfileRef::SketchProfiles { .. }
                    | PlanarProfileRef::SketchRegions { .. }
            )
        ),
        "{profile:?}"
    );
    let ExtrudeExtent::OneSided { side } = extent else {
        panic!("one side: {extent:?}");
    };
    let LinearTermination::Blind { length } = &side.termination else {
        panic!("blind: {side:?}");
    };
    length.get()
}

/// The decoded face of each side token, with its surface.
pub(super) fn named_faces(decoded: &CadIr) -> Vec<(String, SolvedSurfaceGeometry)> {
    let native = crate::test_support::native_test::f3d_native(decoded);
    native
        .persistent_subentity_tags
        .iter()
        .map(|tag| {
            let AttributeTarget::Face(face) = &tag.target else {
                panic!("token {} names a face", tag.token.as_str());
            };
            let face = decoded
                .geometry
                .faces
                .iter()
                .find(|candidate| &candidate.id == face)
                .expect("a face");
            let surface = decoded
                .geometry
                .surfaces
                .iter()
                .find(|surface| surface.id == face.surface)
                .and_then(|surface| surface.geometry.solved())
                .expect("a solved surface");
            (tag.token.as_str().to_owned(), surface.clone())
        })
        .collect()
}

/// The radius of the cylinder named `token`.
fn cylinder_radius(faces: &[(String, SolvedSurfaceGeometry)], token: &str) -> f64 {
    match faces.iter().find(|(name, _)| name == token) {
        Some((_, SolvedSurfaceGeometry::Cylinder(cylinder))) => cylinder.radius().get(),
        other => panic!("token {token} names a cylinder: {other:?}"),
    }
}

/// The token of the face that owns each sketch link's coedge, with the
/// link's curve and sense.
pub(super) fn links(decoded: &CadIr) -> Vec<(String, i64, Option<i64>)> {
    let native = crate::test_support::native_test::f3d_native(decoded);
    let mut links = native
        .sketch_curve_links
        .iter()
        .map(|link| {
            let AttributeTarget::Coedge(coedge) = &link.target else {
                panic!("a link names a coedge");
            };
            let owner = &decoded
                .geometry
                .coedges
                .iter()
                .find(|candidate| &candidate.id == coedge)
                .expect("a coedge")
                .owner_loop;
            let face = decoded
                .geometry
                .loops
                .iter()
                .find(|face_loop| &face_loop.id == owner)
                .map(|face_loop| AttributeTarget::Face(face_loop.face.clone()))
                .expect("a loop");
            let token = native
                .persistent_subentity_tags
                .iter()
                .find(|tag| tag.target == face)
                .map(|tag| tag.token.as_str().to_owned())
                .expect("a named face");
            assert_eq!(link.ref_b, 0);
            (token, link.sketch_curve_id, link.sense.map(i64::from))
        })
        .collect::<Vec<_>>();
    links.sort_unstable();
    links
}

#[test]
fn a_square_with_a_round_hole_is_written_as_an_extrude() {
    let (report, decoded) = write(&plate());
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude"],
        "{:#?}",
        report.losses
    );
    assert!((depth(&decoded) - SIDE).abs() <= TOLERANCE);
    let faces = named_faces(&decoded);
    assert_eq!(faces.len(), 7, "{faces:?}");
    assert!((cylinder_radius(&faces, "7") - 2.0).abs() <= TOLERANCE);
    // The square's four lines link with their traversal flag 0 on both
    // coedges of their start-cap edge; the hole circle with the all-ones
    // sense, on its side face `7` and on the start cap `1`.
    let links = links(&decoded);
    let holes = links
        .iter()
        .filter(|(_, _, sense)| sense.is_none())
        .collect::<Vec<_>>();
    let tokens = holes
        .iter()
        .map(|(token, _, _)| token.as_str())
        .collect::<Vec<_>>();
    assert_eq!(tokens, ["1", "7"], "{links:?}");
    assert_eq!(holes[0].1, holes[1].1);
    assert_eq!(
        links
            .iter()
            .filter(|(_, _, sense)| *sense == Some(0))
            .count(),
        8,
        "{links:?}"
    );
    assert!(
        loss(&report, "export/record.residual-unavailable")
            .iter()
            .all(|note| !note.contains("hole loops")),
        "{:#?}",
        report.losses
    );
}

/// Each side entry of the tracked-results table of the `Extrude` scope
/// `scope`: its key and its curve record.
pub(super) fn tracked_sides(
    design: &super::super::roster::Segment,
    scope: u64,
    sides: usize,
) -> Vec<(u64, &[u8])> {
    let tracked = reference_at(design.record(scope), 349 + 19);
    let results = design.record(tracked);
    assert_eq!(
        usize::try_from(u32_at(results, 21)).expect("count"),
        sides + 1
    );
    (0..sides)
        .map(|side| {
            let entry = design.record(reference_at(results, 25 + 19 * side));
            (
                u64_at(results, 25 + 19 * side + 11),
                design.record(reference_at(entry, 25)),
            )
        })
        .collect()
}

#[test]
fn a_tube_names_its_outer_and_inner_cylinders_by_loop() {
    let (report, decoded, bytes) = write_bytes(&tube());
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude"],
        "{:#?}",
        report.losses
    );
    assert!((depth(&decoded) - CYLINDER_HEIGHT).abs() <= TOLERANCE);
    let faces = named_faces(&decoded);
    assert_eq!(faces.len(), 4, "{faces:?}");
    assert!((cylinder_radius(&faces, "3") - CYLINDER_RADIUS).abs() <= TOLERANCE);
    assert!((cylinder_radius(&faces, "4") - HOLE_RADIUS).abs() <= TOLERANCE);

    let design = segment(&bytes, "FusionDesignSegmentType1");
    let timeline = design.record(207);
    let scope = reference_at(timeline, 47);
    // The second of the five scope references, which end before the
    // current state at offset 327, names the profile selection `N`.
    let selection = reference_at(design.record(scope), 327 - 4 * 11);
    let regions = design.record(selection + 3);
    // One region group of one region with two loops: the exterior circle
    // (entry flag 0, exterior byte 1), then the hole (flag 1, byte 0), and
    // no further region.
    assert_eq!((u32_at(regions, 32), u32_at(regions, 36)), (1, 2));
    let (outer, inner) = (u64_at(regions, 48), u64_at(regions, 93));
    assert_ne!(outer, inner);
    let entries = [(40, outer, 0, 1), (85, inner, 1, 0)];
    for (at, primary, flag, exterior) in entries {
        assert_eq!(u32_at(regions, at), 1, "entry count at {at}");
        assert_eq!(
            (
                u32_at(regions, at + 4),
                u64_at(regions, at + 8),
                u64_at(regions, at + 16),
                u32_at(regions, at + 24),
                u32_at(regions, at + 28),
                u32_at(regions, at + 32),
            ),
            (3, primary, 0, flag, 1, 1),
            "entry at {at}"
        );
        assert_eq!(regions[at + 44], exterior, "exterior byte at {at}");
    }
    assert_eq!(u32_at(regions, 130), 0);

    // Side keys 3 and 4; each curve record names its circle with the
    // entry's secondary identity, sense 1 or all-ones, and two empty lists.
    let sides = tracked_sides(&design, scope, 2);
    let expected = [(3, outer, 1), (4, inner, u32::MAX)];
    for ((key, record), (want, primary, sense)) in sides.iter().zip(expected) {
        assert_eq!(*key, want);
        assert_eq!(
            (
                u32_at(record, 33),
                u64_at(record, 37),
                u64_at(record, 45),
                u32_at(record, 53),
                u32_at(record, 57),
                u32_at(record, 61),
                u32_at(record, 73),
                u32_at(record, 77),
            ),
            (3, primary, 0, sense, 1, 1, 0, 0),
            "key {key}"
        );
    }
    let tracked = reference_at(design.record(scope), 349 + 19);
    let results = design.record(tracked);
    assert_eq!(u64_at(results, 25 + 19 * 2 + 11), 5);
    assert_eq!(u64_at(results, 25 + 19 * 3), 6);
    let body = design.record(reference_at(results, 25 + 19 * 2));
    assert_eq!(u32_at(body, 106), 2);
    assert_eq!(&body[110..112], b"34");
    assert_eq!(
        (
            u32_at(body, 112),
            u32_at(body, 116),
            u32_at(body, 120),
            u32_at(body, 124)
        ),
        (3, 0, 1, 2)
    );

    let identity = |primary: u64| i64::try_from(primary).expect("an identity");
    let mut expected = vec![
        ("1".to_owned(), identity(outer), Some(0)),
        ("1".to_owned(), identity(inner), None),
        ("3".to_owned(), identity(outer), Some(0)),
        ("4".to_owned(), identity(inner), None),
    ];
    expected.sort_unstable();
    assert_eq!(links(&decoded), expected);
}

#[test]
fn a_disc_with_two_holes_names_the_holes_in_curve_order() {
    let (report, decoded) = write(&disc());
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude"],
        "{:#?}",
        report.losses
    );
    assert!((depth(&decoded) - CYLINDER_HEIGHT).abs() <= TOLERANCE);
    let faces = named_faces(&decoded);
    assert_eq!(faces.len(), 5, "{faces:?}");
    assert!((cylinder_radius(&faces, "3") - CYLINDER_RADIUS).abs() <= TOLERANCE);
    let axis_x = |token: &str| match faces.iter().find(|(name, _)| name == token) {
        Some((_, SolvedSurfaceGeometry::Cylinder(cylinder))) => cylinder.origin().get().x,
        other => panic!("token {token} names a cylinder: {other:?}"),
    };
    assert!((axis_x("4") - 5.0).abs() <= TOLERANCE);
    assert!((axis_x("5") + 5.0).abs() <= TOLERANCE);
    let residual = loss(&report, "export/record.residual-unavailable").join("\n");
    assert!(
        residual.contains("the holes stored in the order of their curves' primary identities")
            && residual.contains("DR-883"),
        "{residual}"
    );
}
