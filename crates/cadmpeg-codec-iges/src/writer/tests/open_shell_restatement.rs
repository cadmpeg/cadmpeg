// SPDX-License-Identifier: Apache-2.0
//! An open shell written for an IGES version that defines none, and an
//! analytic face written outside a B-rep.

use std::f64::consts::{FRAC_1_SQRT_2, TAU};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::geometry::pcurve::{LinePcurve, Pcurve, PcurveGeometry, PcurveMetadata};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::LoopId;
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::{BodyKind, FaceLoops, Loop, LoopBoundary, LoopRing, PcurveUse};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::FiniteVector;
use cadmpeg_ir::CadIr;
use cadmpeg_test_support::solids::{CYLINDER_HEIGHT, CYLINDER_RADIUS};

use crate::test_support::decoded_with;
use crate::test_support::test_closed_cylinder::solid_cylinder;
use crate::{IgesCodec, IgesVersion};

fn written(ir: &CadIr, version: IgesVersion) -> (Vec<LossNote>, Vec<u8>) {
    let plan = IgesCodec
        .plan(
            EncodeInput::new(ir, None),
            TargetRequest::Explicit(version.descriptor().id.as_str()),
        )
        .expect("the document is written");
    let losses = plan.report().losses.clone();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("the file is written");
    (losses, bytes)
}

fn charged<'a>(losses: &'a [LossNote], code: &str) -> Vec<&'a str> {
    losses
        .iter()
        .filter(|loss| loss.code.to_string() == code)
        .map(|loss| loss.message.as_str())
        .collect()
}

fn keyed(id: &str, key: &str) -> bool {
    id.ends_with(&format!("#{key}"))
}

/// The cylinder's side and top faces as one open sheet whose coedges carry
/// no parameter curve.
fn open_sheet() -> CadIr {
    let mut ir = solid_cylinder();
    let geometry = &mut ir.geometry;
    geometry
        .faces
        .retain(|face| !keyed(face.id.as_str(), "bottom"));
    geometry
        .loops
        .retain(|loop_| !keyed(loop_.id.as_str(), "bottom"));
    geometry
        .coedges
        .retain(|coedge| !keyed(coedge.id.as_str(), "bottom"));
    geometry
        .surfaces
        .retain(|surface| !keyed(surface.id.as_str(), "bottom"));
    geometry.bodies[0].kind = BodyKind::Sheet;
    let side_bottom = geometry
        .coedges
        .iter_mut()
        .find(|coedge| keyed(coedge.id.as_str(), "side-bottom"))
        .expect("the side coedge");
    side_bottom.radial_next = side_bottom.id.clone();
    ir.finalize();
    ir
}

/// [`open_sheet`] whose side face coedges carry line parameter curves on the
/// cylinder chart, each run in its coedge's traversal:
/// `(origin, direction, length)`.
fn open_cylinder(side_curves: [(&str, [f64; 2], [f64; 2], f64); 4]) -> CadIr {
    let mut ir = open_sheet();
    let geometry = &mut ir.geometry;
    for (key, origin, direction, length) in side_curves {
        let id = format!("test:model:pcurve#{key}");
        geometry.pcurves.push(Pcurve {
            id: id.as_str().try_into().expect("valid identity"),
            geometry: PcurveGeometry::Line(
                LinePcurve::try_new(
                    Point2::new(origin[0], origin[1]),
                    Point2::new(direction[0], direction[1]),
                )
                .expect("a unit direction"),
            ),
            metadata: PcurveMetadata::general(None, FiniteVector::new([0.0, length]), None),
            native_ref: None,
        });
        geometry
            .coedges
            .iter_mut()
            .find(|coedge| keyed(coedge.id.as_str(), key))
            .expect("the side coedge")
            .pcurves = vec![PcurveUse {
            pcurve: id.as_str().try_into().expect("valid identity"),
            isoparametric: Some(true),
            parameter_range: None,
        }];
    }
    ir.finalize();
    ir
}

fn isoparametric_side() -> [(&'static str, [f64; 2], [f64; 2], f64); 4] {
    let h = CYLINDER_HEIGHT;
    [
        ("side-bottom", [0.0, 0.0], [1.0, 0.0], TAU),
        ("side-seam-up", [TAU, 0.0], [0.0, 1.0], h),
        ("side-top", [TAU, h], [-1.0, 0.0], TAU),
        ("side-seam-down", [0.0, h], [0.0, -1.0], h),
    ]
}

#[test]
fn an_open_shell_before_5_3_writes_each_face_as_its_own_trimmed_surface() {
    let ir = open_cylinder(isoparametric_side());
    let body = ir.geometry.bodies[0].id.to_string();
    for version in [IgesVersion::V5_0, IgesVersion::V5_1] {
        let (losses, bytes) = written(&ir, version);
        assert!(
            charged(&losses, "iges/writer.face-omitted").is_empty(),
            "{losses:#?}"
        );
        let unjoined = charged(&losses, "iges/writer.open-shell-unjoined");
        assert_eq!(unjoined.len(), 1, "{losses:#?}");
        assert!(unjoined[0].contains(&body), "{unjoined:?}");
        let read = decoded_with(&bytes, &[]);
        assert_eq!(read.geometry.faces.len(), 2);
        assert_eq!(read.geometry.bodies.len(), 2);
    }
}

/// Before IGES 5.3 each face of an open sheet is written as a trimmed
/// surface of its own, whose one Directory colour field states the face
/// colour. A body colour that differs from the face colours is charged once,
/// naming the body and its faces. Where a binding of the whole body names the
/// body colour, the charge of that binding states the loss.
#[test]
fn a_body_colour_that_unjoined_faces_replace_is_charged_once() {
    let mut ir = open_cylinder(isoparametric_side());
    let red = cadmpeg_ir::topology::Color::new(1.0, 0.0, 0.0, 1.0);
    let blue = cadmpeg_ir::topology::Color::new(0.0, 0.0, 1.0, 1.0);
    ir.geometry.bodies[0].color = red;
    for face in &mut ir.geometry.faces {
        face.color = blue;
    }
    let body = ir.geometry.bodies[0].id.clone();
    let (losses, _) = written(&ir, IgesVersion::V5_0);
    let charges = charged(&losses, "iges/writer.body-color-not-represented");
    assert_eq!(charges.len(), 1, "{losses:#?}");
    assert!(
        charges[0].contains(&format!("body {body} "))
            && ir
                .geometry
                .faces
                .iter()
                .all(|face| charges[0].contains(face.id.as_str())),
        "{charges:#?}"
    );
    let binding = bind_the_body(&mut ir, red, None);
    let (losses, _) = written(&ir, IgesVersion::V5_0);
    assert!(
        charged(&losses, "iges/writer.body-color-not-represented").is_empty(),
        "{losses:#?}"
    );
    assert!(
        charged(&losses, "iges/writer.appearance-not-represented")
            .iter()
            .any(|loss| loss.contains(binding.as_str())),
        "{losses:#?}"
    );
}

/// A binding of the whole body to an appearance of the body colour that is
/// charged for its visibility states no colour loss, so the body colour that
/// the unjoined faces replace is charged.
#[test]
fn a_body_colour_whose_binding_is_charged_for_its_visibility_is_charged() {
    let mut ir = open_cylinder(isoparametric_side());
    let red = cadmpeg_ir::topology::Color::new(1.0, 0.0, 0.0, 1.0);
    let blue = cadmpeg_ir::topology::Color::new(0.0, 0.0, 1.0, 1.0);
    ir.geometry.bodies[0].color = red;
    for face in &mut ir.geometry.faces {
        face.color = blue;
    }
    let body = ir.geometry.bodies[0].id.clone();
    let binding = bind_the_body(&mut ir, red, Some(true));
    let (losses, _) = written(&ir, IgesVersion::V5_0);
    let charges = charged(&losses, "iges/writer.body-color-not-represented");
    assert_eq!(charges.len(), 1, "{losses:#?}");
    assert!(
        charges[0].contains(&format!("body {body} ")),
        "{charges:#?}"
    );
    assert!(
        charged(&losses, "iges/writer.appearance-not-represented")
            .iter()
            .any(|loss| loss.contains("visibility") && loss.contains(binding.as_str())),
        "{losses:#?}"
    );
}

/// A body whose id ends as the id of a piece of a split body does, and which
/// the writer does not split, is the target of its own binding.
#[test]
fn a_body_named_like_a_split_piece_carries_its_own_binding() {
    let mut ir = open_cylinder(isoparametric_side());
    let red = cadmpeg_ir::topology::Color::new(1.0, 0.0, 0.0, 1.0);
    let source = ir.geometry.bodies[0].id.clone();
    let body =
        cadmpeg_ir::ids::BodyId::try_from(format!("{source}:unjoined.1")).expect("a body id");
    for region in &mut ir.geometry.regions {
        if region.body == source {
            region.body = body.clone();
        }
    }
    ir.geometry.bodies[0].id = body;
    ir.geometry.bodies[0].color = red;
    ir.finalize();
    let binding = bind_the_body(&mut ir, red, None);
    let (losses, _) = written(&ir, IgesVersion::V5_3);
    assert!(
        charged(&losses, "iges/writer.appearance-not-represented")
            .iter()
            .all(|loss| !loss.contains(binding.as_str())),
        "{losses:#?}"
    );
}

/// Binds the whole of the first body of `ir` to a new appearance of `color`,
/// with the blank status `visible`.
fn bind_the_body(
    ir: &mut CadIr,
    color: Option<cadmpeg_ir::topology::Color>,
    visible: Option<bool>,
) -> cadmpeg_ir::ids::AppearanceBindingId {
    let appearance =
        cadmpeg_ir::ids::AppearanceId::try_from("test:model:appearance#red".to_owned())
            .expect("an appearance id");
    let binding = cadmpeg_ir::ids::AppearanceBindingId::try_from(
        "test:model:appearance-binding#body".to_owned(),
    )
    .expect("a binding id");
    ir.model
        .appearances
        .push(cadmpeg_ir::appearance::Appearance {
            id: appearance.clone(),
            name: None,
            asset_guid: None,
            library_id: None,
            visual_guid: None,
            physical_token: None,
            schema: None,
            category: None,
            base_color: color,
            properties: std::collections::BTreeMap::new(),
            textures: Vec::new(),
            native_ref: None,
        });
    ir.model
        .appearance_bindings
        .push(cadmpeg_ir::appearance::AppearanceBinding {
            id: binding.clone(),
            target: cadmpeg_ir::appearance::AppearanceTarget::Body(
                ir.geometry.bodies[0].id.clone(),
            )
            .into(),
            appearance,
            precedence: None,
            source_entity_id: None,
            object_type: None,
            visible,
            channels: std::collections::BTreeMap::new(),
            native_ref: None,
        });
    binding
}

#[test]
fn an_open_shell_at_5_3_keeps_its_joined_edges() {
    let (losses, bytes) = written(&open_cylinder(isoparametric_side()), IgesVersion::V5_3);
    assert!(charged(&losses, "iges/writer.open-shell-unjoined").is_empty());
    assert!(
        charged(&losses, "iges/writer.face-omitted").is_empty(),
        "{losses:#?}"
    );
    let read = decoded_with(&bytes, &[]);
    assert_eq!(read.geometry.faces.len(), 2);
    assert_eq!(read.geometry.bodies.len(), 1);
}

/// Points of the parameter curves on the boundary of the face that reads back
/// on a curved support, each mapped through that support.
fn curved_boundary_points(read: &CadIr) -> Vec<Point3> {
    let geometry = &read.geometry;
    let (face, surface) = geometry
        .faces
        .iter()
        .find_map(|face| {
            let surface = geometry
                .surfaces
                .iter()
                .find(|surface| surface.id == face.surface)?;
            match &surface.geometry {
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_)) => None,
                SurfaceGeometry::Solved(solved) => Some((face, solved)),
                SurfaceGeometry::Procedural { cache, .. } => {
                    cache.as_ref().map(|cache| (face, cache))
                }
            }
        })
        .expect("a face reads back on a solved curved support");
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
        .expect("an evaluation context");
    let mut points = Vec::new();
    for loop_id in face.loops.stated() {
        let loop_ = geometry
            .loops
            .iter()
            .find(|loop_| loop_.id == *loop_id)
            .expect("the face loop reads back");
        for coedge_id in loop_.coedges() {
            let coedge = geometry
                .coedges
                .iter()
                .find(|coedge| coedge.id == *coedge_id)
                .expect("the loop coedge reads back");
            for pcurve_use in &coedge.pcurves {
                let pcurve = geometry
                    .pcurves
                    .iter()
                    .find(|pcurve| pcurve.id == pcurve_use.pcurve)
                    .expect("the parameter curve reads back");
                let [start, end] = pcurve
                    .parameter_range()
                    .expect("a bounded parameter curve")
                    .get();
                for step in 0..=8 {
                    let parameter = (f64::from(step) / 8.0).mul_add(end - start, start);
                    let uv = cadmpeg_ir::eval::decode::pcurve_uv(&ctx, &pcurve.geometry, parameter)
                        .expect("a parameter point")
                        .get();
                    points.push(
                        cadmpeg_ir::eval::decode::surface_point_solved(&ctx, surface, uv.u, uv.v)
                            .expect("a point on the support")
                            .get(),
                    );
                }
            }
        }
    }
    assert!(
        !points.is_empty(),
        "the curved face boundary carries parameter curves"
    );
    points
}

fn assert_on_side(points: &[Point3], x_scale: f64) {
    for point in points {
        assert!(
            ((point.x / x_scale).hypot(point.y) - CYLINDER_RADIUS).abs() <= 1.0e-9,
            "{point:?} lies off the side surface"
        );
        assert!(
            (-1.0e-9..=CYLINDER_HEIGHT + 1.0e-9).contains(&point.z),
            "{point:?} lies outside the side face"
        );
    }
}

#[test]
fn an_analytic_face_outside_a_brep_writes_an_exact_rational_support() {
    let (losses, bytes) = written(&open_cylinder(isoparametric_side()), IgesVersion::V5_1);
    assert!(
        charged(&losses, "iges/writer.face-omitted").is_empty(),
        "{losses:#?}"
    );
    let text = std::str::from_utf8(&bytes).expect("IGES Fixed ASCII is UTF-8");
    assert!(
        text.lines()
            .any(|card| card.as_bytes().get(72) == Some(&b'D') && card[..8].trim() == "128"),
        "the side support is written as a Type 128"
    );
    assert_on_side(&curved_boundary_points(&decoded_with(&bytes, &[])), 1.0);
}

#[test]
fn a_cylinder_face_without_parameter_curves_writes_computed_ones() {
    let (losses, bytes) = written(&open_sheet(), IgesVersion::V5_1);
    assert!(
        charged(&losses, "iges/writer.face-omitted").is_empty(),
        "{losses:#?}"
    );
    let read = decoded_with(&bytes, &[]);
    assert_eq!(read.geometry.faces.len(), 2);
    let points = curved_boundary_points(&read);
    assert_on_side(&points, 1.0);
    for height in [0.0, CYLINDER_HEIGHT] {
        assert!(
            points
                .iter()
                .any(|point| (point.z - height).abs() <= 1.0e-9),
            "no boundary point lies on the circle at height {height}"
        );
    }
}

#[test]
fn a_cylinder_under_a_non_uniform_scale_writes_an_elliptic_rational_support() {
    let mut ir = open_cylinder(isoparametric_side());
    ir.geometry.bodies[0].transform = Transform::affine([
        [2.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ]);
    let (losses, bytes) = written(&ir, IgesVersion::V5_1);
    assert!(
        charged(&losses, "iges/writer.face-omitted").is_empty(),
        "{losses:#?}"
    );
    assert!(
        charged(&losses, "iges/writer.body-omitted").is_empty(),
        "{losses:#?}"
    );
    let read = decoded_with(&bytes, &[]);
    assert!(read
        .geometry
        .bodies
        .iter()
        .all(|body| body.transform.is_none()));
    assert_on_side(&curved_boundary_points(&read), 2.0);
}

#[test]
fn a_solid_cylinder_under_a_non_uniform_scale_writes_an_elliptic_rational_support() {
    let mut ir = solid_cylinder();
    ir.geometry.bodies[0].transform = Transform::affine([
        [2.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ]);
    let (losses, bytes) = written(&ir, IgesVersion::V5_3);
    assert!(
        charged(&losses, "iges/writer.body-omitted").is_empty(),
        "{losses:#?}"
    );
    assert!(
        charged(&losses, "iges/writer.face-omitted").is_empty(),
        "{losses:#?}"
    );
    let read = &decoded_with(&bytes, &[]);
    assert_eq!(read.geometry.faces.len(), 3, "{losses:#?}");
    assert!(read
        .geometry
        .bodies
        .iter()
        .all(|body| body.transform.is_none()));
    assert_on_side(&curved_boundary_points(read), 2.0);
}

/// [`solid_cylinder`] with no seam: the side face is a band bounded by its
/// bottom and top circles, each a loop of its own, and the face classifies
/// neither loop as outer.
fn seamless_cylinder() -> CadIr {
    let mut ir = solid_cylinder();
    let geometry = &mut ir.geometry;
    geometry
        .coedges
        .retain(|coedge| !coedge.id.as_str().contains("#side-seam"));
    geometry
        .edges
        .retain(|edge| !keyed(edge.id.as_str(), "seam"));
    geometry
        .curves
        .retain(|curve| !keyed(curve.id.as_str(), "seam"));
    geometry
        .loops
        .retain(|loop_| !keyed(loop_.id.as_str(), "side"));
    let mut side_loops = Vec::new();
    for key in ["side-bottom", "side-top"] {
        let loop_id: LoopId = format!("test:model:loop#{key}")
            .try_into()
            .expect("valid identity");
        let coedge = geometry
            .coedges
            .iter_mut()
            .find(|coedge| keyed(coedge.id.as_str(), key))
            .expect("the side coedge");
        coedge.owner_loop = loop_id.clone();
        geometry.loops.push(Loop {
            id: loop_id.clone(),
            face: "test:model:face#side".try_into().expect("valid identity"),
            boundary: LoopBoundary::Ring(
                LoopRing::new(vec![coedge.id.clone()], Vec::new()).expect("a ring"),
            ),
            native_ref: None,
        });
        side_loops.push(loop_id);
    }
    geometry
        .faces
        .iter_mut()
        .find(|face| keyed(face.id.as_str(), "side"))
        .expect("the side face")
        .loops = FaceLoops::Unspecified { loops: side_loops };
    ir.finalize();
    ir
}

#[test]
fn a_seamless_cylinder_band_under_a_non_uniform_scale_writes_its_full_turn_face() {
    let mut ir = seamless_cylinder();
    ir.geometry.bodies[0].transform = Transform::affine([
        [2.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ]);
    let (losses, bytes) = written(&ir, IgesVersion::V5_3);
    for code in [
        "iges/writer.body-omitted",
        "iges/writer.face-omitted",
        "iges/writer.pcurve-omitted",
    ] {
        assert!(charged(&losses, code).is_empty(), "{losses:#?}");
    }
    let read = decoded_with(&bytes, &[]);
    assert_eq!(read.geometry.faces.len(), 3);
    let points = curved_boundary_points(&read);
    assert_on_side(&points, 2.0);
    for height in [0.0, CYLINDER_HEIGHT] {
        assert!(
            points
                .iter()
                .any(|point| (point.z - height).abs() <= 1.0e-9),
            "no boundary point lies on the ellipse at height {height}"
        );
    }
}

#[test]
fn a_cylinder_face_with_a_slanted_parameter_curve_keeps_its_model_space_boundary() {
    let mut curves = isoparametric_side();
    curves[0].2 = [FRAC_1_SQRT_2, FRAC_1_SQRT_2];
    let (losses, bytes) = written(&open_cylinder(curves), IgesVersion::V5_1);
    assert!(
        charged(&losses, "iges/writer.face-omitted").is_empty(),
        "{losses:#?}"
    );
    let omitted = charged(&losses, "iges/writer.pcurve-omitted");
    assert_eq!(omitted.len(), 4, "{losses:#?}");
    // A boundary with no parameter curves on a curved support keeps its
    // written traversal on read.
    let read = decoded_with(&bytes, &["iges/topology.boundary-winding-unresolved"]);
    assert_eq!(read.geometry.faces.len(), 2);
    let side = read
        .geometry
        .faces
        .iter()
        .find(|face| {
            read.geometry.surfaces.iter().any(|surface| {
                surface.id == face.surface
                    && !matches!(
                        surface.geometry,
                        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
                    )
            })
        })
        .expect("the side face reads back on its curved support");
    let side_loops = side.loops.stated().collect::<Vec<_>>();
    assert!(read
        .geometry
        .coedges
        .iter()
        .filter(|coedge| side_loops.contains(&&coedge.owner_loop))
        .all(|coedge| coedge.pcurves.is_empty()));
}
