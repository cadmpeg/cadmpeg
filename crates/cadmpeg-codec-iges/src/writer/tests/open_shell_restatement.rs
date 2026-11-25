// SPDX-License-Identifier: Apache-2.0
//! An open shell written for an IGES version that defines none, and an
//! analytic face written outside a B-rep.

use std::f64::consts::{FRAC_1_SQRT_2, TAU};
use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::pcurve::{LinePcurve, Pcurve, PcurveGeometry, PcurveMetadata};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::{BodyKind, PcurveUse};
use cadmpeg_ir::units::FiniteVector;
use cadmpeg_ir::CadIr;
use cadmpeg_test_support::solids::{CYLINDER_HEIGHT, CYLINDER_RADIUS};

use crate::test_support::test_closed_cylinder::solid_cylinder;
use crate::{IgesCodec, IgesVersion};

fn written(ir: &CadIr, version: IgesVersion) -> (Vec<LossNote>, Vec<u8>) {
    let plan = IgesCodec
        .plan(
            EncodeInput::new(ir, None)
                .with_target(TargetRequest::Explicit(version.descriptor().id.as_str())),
        )
        .expect("the document is written");
    let losses = plan.report().losses.clone();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("the file is written");
    (losses, bytes)
}

fn decoded(bytes: Vec<u8>) -> CadIr {
    IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("the written file decodes")
        .ir()
        .clone()
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

/// The cylinder's side and top faces as one open sheet. The side face's
/// coedges carry line parameter curves on the cylinder chart, each run in its
/// coedge's traversal: `(origin, direction, length)`.
fn open_cylinder(side_curves: [(&str, [f64; 2], [f64; 2], f64); 4]) -> CadIr {
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
        let coedge = geometry
            .coedges
            .iter_mut()
            .find(|coedge| keyed(coedge.id.as_str(), key))
            .expect("the side coedge");
        coedge.pcurves = vec![PcurveUse {
            pcurve: id.as_str().try_into().expect("valid identity"),
            isoparametric: Some(true),
            parameter_range: None,
        }];
        if key == "side-bottom" {
            coedge.radial_next = coedge.id.clone();
        }
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
        let read = decoded(bytes);
        assert_eq!(read.geometry.faces.len(), 2);
        assert_eq!(read.geometry.bodies.len(), 2);
    }
}

#[test]
fn an_open_shell_at_5_3_keeps_its_joined_edges() {
    let (losses, bytes) = written(&open_cylinder(isoparametric_side()), IgesVersion::V5_3);
    assert!(charged(&losses, "iges/writer.open-shell-unjoined").is_empty());
    assert!(
        charged(&losses, "iges/writer.face-omitted").is_empty(),
        "{losses:#?}"
    );
    let read = decoded(bytes);
    assert_eq!(read.geometry.faces.len(), 2);
    assert_eq!(read.geometry.bodies.len(), 1);
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
    let read = decoded(bytes);
    let geometry = &read.geometry;
    let side = geometry
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
        .expect("the side face reads back on a solved curved support");
    let (face, surface) = side;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
        .expect("an evaluation context");
    let mut sampled = 0;
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
                    let point =
                        cadmpeg_ir::eval::decode::surface_point_solved(&ctx, surface, uv.u, uv.v)
                            .expect("a point on the support")
                            .get();
                    assert!(
                        (point.x.hypot(point.y) - CYLINDER_RADIUS).abs() <= 1.0e-9,
                        "{point:?} lies off the cylinder"
                    );
                    assert!(
                        (-1.0e-9..=CYLINDER_HEIGHT + 1.0e-9).contains(&point.z),
                        "{point:?} lies outside the side face"
                    );
                    sampled += 1;
                }
            }
        }
    }
    assert!(
        sampled > 0,
        "the side face boundary carries parameter curves"
    );
}

#[test]
fn an_analytic_face_with_a_slanted_parameter_curve_is_dropped_naming_the_rule() {
    let mut curves = isoparametric_side();
    curves[0].2 = [FRAC_1_SQRT_2, FRAC_1_SQRT_2];
    let (losses, _) = written(&open_cylinder(curves), IgesVersion::V5_1);
    let omitted = charged(&losses, "iges/writer.face-omitted");
    assert_eq!(omitted.len(), 1, "{losses:#?}");
    assert!(omitted[0].contains("side"), "{omitted:?}");
    assert!(omitted[0].contains("isoparametric"), "{omitted:?}");
}
