// SPDX-License-Identifier: Apache-2.0
//! Documents that IGES states through an exact equivalent are written and
//! read back.

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::eval::decode::{curve_point_solved, pcurve_uv, surface_point_solved};
use cadmpeg_ir::examples::{add_document_root, unit_cube};
use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_ir::geometry::pcurve::{LinePcurve, Pcurve, PcurveGeometry, PcurveMetadata};
use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::native::NativeRecord;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::{BodyKind, Face, Sense};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::FiniteVector;
use cadmpeg_ir::CadIr;

use super::super::native_dispositions;
use super::super::pcurve_entity;
use super::super::records::TopologyIndex;
use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
use crate::{IgesCodec, IgesVersion};

const OPEN_SHELL: &[u8] = include_bytes!("../../../tests/golden/fixtures/explicit_open_shell.igs");
const LINE: &[u8] = include_bytes!("../../../tests/golden/fixtures/line.igs");
const ELLIPSE: &[u8] = include_bytes!("../../../tests/golden/fixtures/conic_ellipse.igs");
const PARABOLA: &[u8] = include_bytes!("../../../tests/golden/fixtures/conic_parabola.igs");
const COMPOSITE: &[u8] = include_bytes!("../../../tests/golden/fixtures/composite_curve.igs");
/// Bound on the distance of a placed source point from the written curve.
const ON_CURVE: f64 = 1.0e-6;
const LOOPLESS_FACE: &[u8] =
    include_bytes!("../../../tests/golden/fixtures/parameter_domain_trimmed_surface.igs");

fn decoded(bytes: &[u8]) -> CadIr {
    IgesCodec
        .decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default())
        .expect("the file decodes")
        .ir()
        .clone()
}

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

/// The (type, form) of each Directory Entry, in file order.
fn directory(bytes: &[u8]) -> Vec<(i64, i64)> {
    let text = std::str::from_utf8(bytes).expect("IGES Fixed ASCII is UTF-8");
    let cards = text
        .lines()
        .filter(|card| card.as_bytes().get(72) == Some(&b'D'))
        .collect::<Vec<_>>();
    cards
        .chunks(2)
        .map(|pair| {
            let field = |card: &str, start: usize| {
                card[start..start + 8]
                    .trim()
                    .parse::<i64>()
                    .expect("a numeric Directory field")
            };
            (field(pair[0], 0), field(pair[1], 32))
        })
        .collect()
}

fn cube_with(rows: [[f64; 4]; 3]) -> CadIr {
    let mut ir = unit_cube().expect("the example cube builds");
    add_document_root(&mut ir);
    ir.geometry.bodies[0].transform = Transform::affine(rows);
    ir
}

fn assert_rows_eq(actual: Option<Transform>, expected: [[f64; 4]; 3]) {
    let actual = actual
        .expect("the body reads back with a transform")
        .affine_rows();
    for (actual, expected) in actual.iter().flatten().zip(expected.iter().flatten()) {
        assert!(
            (actual - expected).abs() <= 1.0e-12,
            "{actual} != {expected}"
        );
    }
}

fn positions(ir: &CadIr) -> Vec<[f64; 3]> {
    ir.geometry
        .points
        .iter()
        .map(|point| {
            let position = point.position().get();
            [position.x, position.y, position.z]
        })
        .collect()
}

fn transformed(rows: [[f64; 4]; 3], point: [f64; 3]) -> [f64; 3] {
    rows.map(|row| {
        row[0].mul_add(
            point[0],
            row[1].mul_add(point[1], row[2].mul_add(point[2], row[3])),
        )
    })
}

fn near(actual: [f64; 3], expected: [f64; 3]) -> bool {
    actual
        .iter()
        .zip(expected)
        .all(|(actual, expected)| (actual - expected).abs() <= 1.0e-9)
}

fn assert_points_contain(actual: &[[f64; 3]], expected: &[[f64; 3]]) {
    for point in expected {
        assert!(
            actual.iter().any(|actual| near(*actual, *point)),
            "{point:?} is missing from {actual:?}"
        );
    }
}

fn support<'ir>(ir: &'ir CadIr, face: &Face) -> &'ir SolvedSurfaceGeometry {
    let surface = ir
        .geometry
        .surfaces
        .iter()
        .find(|surface| surface.id == face.surface)
        .expect("the face support reads back");
    match &surface.geometry {
        SurfaceGeometry::Solved(solved) => solved,
        SurfaceGeometry::Procedural { cache, .. } => {
            cache.as_ref().expect("the support reads back solved")
        }
    }
}

/// The parameter curves on the loops of `face`.
fn boundary_pcurves<'ir>(ir: &'ir CadIr, face: &'ir Face) -> Vec<&'ir Pcurve> {
    let geometry = &ir.geometry;
    face.loops
        .stated()
        .filter_map(|loop_id| geometry.loops.iter().find(|loop_| loop_.id == *loop_id))
        .flat_map(cadmpeg_ir::topology::Loop::coedges)
        .filter_map(|coedge_id| {
            geometry
                .coedges
                .iter()
                .find(|coedge| coedge.id == *coedge_id)
        })
        .flat_map(|coedge| coedge.pcurves.iter())
        .filter_map(|used| {
            geometry
                .pcurves
                .iter()
                .find(|pcurve| pcurve.id == used.pcurve)
        })
        .collect()
}

/// The support points at sixteen equal steps of each parameter curve on a
/// face boundary.
fn boundary_samples(ir: &CadIr) -> Vec<[f64; 3]> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
        .expect("an evaluation context");
    let mut points = Vec::new();
    for face in &ir.geometry.faces {
        let solved = support(ir, face);
        for pcurve in boundary_pcurves(ir, face) {
            let [start, end] = pcurve
                .parameter_range()
                .expect("a bounded parameter curve")
                .get();
            for step in 0..=16 {
                let parameter = (f64::from(step) / 16.0).mul_add(end - start, start);
                let uv = pcurve_uv(&ctx, &pcurve.geometry, parameter)
                    .expect("a parameter point")
                    .get();
                let point = surface_point_solved(&ctx, solved, uv.u, uv.v)
                    .expect("a point on the support")
                    .get();
                points.push([point.x, point.y, point.z]);
            }
        }
    }
    points
}

/// A point of the support of `face` and the unit normal there, oriented by
/// the support's reversed-normal flag and the face sense. The point is on the face boundary where the face carries
/// a parameter curve, and at the parameter origin otherwise.
fn oriented_normal(ir: &CadIr, face: &Face) -> ([f64; 3], [f64; 3]) {
    let solved = support(ir, face);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
        .expect("an evaluation context");
    let uv = boundary_pcurves(ir, face)
        .first()
        .map_or(Point2::new(0.0, 0.0), |pcurve| {
            let [start, end] = pcurve
                .parameter_range()
                .expect("a bounded parameter curve")
                .get();
            pcurve_uv(&ctx, &pcurve.geometry, 0.5 * (start + end))
                .expect("a parameter point")
                .get()
        });
    let at = |u: f64, v: f64| {
        let point = surface_point_solved(&ctx, solved, u, v)
            .expect("a point on the support")
            .get();
        [point.x, point.y, point.z]
    };
    let step = 1.0e-4;
    let difference = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let du = difference(at(uv.u + step, uv.v), at(uv.u - step, uv.v));
    let dv = difference(at(uv.u, uv.v + step), at(uv.u, uv.v - step));
    let normal = [
        du[1].mul_add(dv[2], -du[2] * dv[1]),
        du[2].mul_add(dv[0], -du[0] * dv[2]),
        du[0].mul_add(dv[1], -du[1] * dv[0]),
    ];
    let length = normal.iter().map(|value| value * value).sum::<f64>().sqrt();
    let normal_reversed =
        matches!(solved, SolvedSurfaceGeometry::Nurbs(nurbs) if nurbs.normal_reversed());
    let sign = if (face.sense == Sense::Reversed) == normal_reversed {
        1.0
    } else {
        -1.0
    };
    (at(uv.u, uv.v), normal.map(|value| sign * value / length))
}

#[test]
fn a_translated_solid_writes_a_type_124_form_0_and_reads_back_placed() {
    let rows = [
        [0.0, -1.0, 0.0, 5.0],
        [1.0, 0.0, 0.0, -2.0],
        [0.0, 0.0, 1.0, 3.0],
    ];
    let (losses, bytes) = written(&cube_with(rows), IgesVersion::V5_3);
    assert!(
        charged(&losses, "iges/writer.body-omitted").is_empty(),
        "{losses:#?}"
    );
    let entries = directory(&bytes);
    assert!(entries.contains(&(124, 0)), "{entries:?}");
    assert!(entries.contains(&(186, 0)), "{entries:?}");
    let read = decoded(&bytes);
    assert_eq!(read.geometry.bodies.len(), 1);
    assert_rows_eq(read.geometry.bodies[0].transform, rows);
}

#[test]
fn a_mirrored_solid_writes_a_type_124_form_1_and_reads_back_placed() {
    let rows = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, -1.0, 4.0],
    ];
    let (losses, bytes) = written(&cube_with(rows), IgesVersion::V5_3);
    assert!(
        charged(&losses, "iges/writer.body-omitted").is_empty(),
        "{losses:#?}"
    );
    assert!(directory(&bytes).contains(&(124, 1)));
    let read = decoded(&bytes);
    assert_eq!(read.geometry.bodies.len(), 1);
    assert_rows_eq(read.geometry.bodies[0].transform, rows);
}

#[test]
fn a_sheet_body_transform_is_placed_into_its_geometry() {
    let rows = [
        [0.0, -1.0, 0.0, 5.0],
        [1.0, 0.0, 0.0, -2.0],
        [0.0, 0.0, 1.0, 3.0],
    ];
    let original = decoded(OPEN_SHELL);
    let expected = positions(&original)
        .into_iter()
        .map(|point| transformed(rows, point))
        .collect::<Vec<_>>();
    let mut ir = original;
    ir.geometry
        .bodies
        .iter_mut()
        .find(|body| body.kind == BodyKind::Sheet)
        .expect("the fixture holds a sheet body")
        .transform = Transform::affine(rows);
    for version in [IgesVersion::V5_3, IgesVersion::V5_0] {
        let (losses, bytes) = written(&ir, version);
        assert!(
            charged(&losses, "iges/writer.body-omitted").is_empty(),
            "{losses:#?}"
        );
        assert!(
            charged(&losses, "iges/writer.face-omitted").is_empty(),
            "{losses:#?}"
        );
        let read = decoded(&bytes);
        assert_eq!(read.geometry.faces.len(), 1);
        assert!(read
            .geometry
            .bodies
            .iter()
            .all(|body| body.transform.is_none()));
        assert_points_contain(&boundary_samples(&read), &expected);
    }
}

#[test]
fn a_wire_body_transform_is_placed_into_its_geometry() {
    let rows = [
        [0.0, 0.0, 1.0, -1.0],
        [0.0, 1.0, 0.0, 4.0],
        [-1.0, 0.0, 0.0, 2.0],
    ];
    let mut ir = decoded(LINE);
    for body in &mut ir.geometry.bodies {
        body.transform = Transform::affine(rows);
    }
    let (losses, bytes) = written(&ir, IgesVersion::V5_3);
    assert!(
        charged(&losses, "iges/writer.body-omitted").is_empty(),
        "{losses:#?}"
    );
    let read = decoded(&bytes);
    assert_points_contain(
        &positions(&read),
        &[
            transformed(rows, [1.0, 2.0, 3.0]),
            transformed(rows, [4.0, 6.0, 3.0]),
        ],
    );
}

#[test]
fn a_solid_under_a_non_uniform_scale_writes_its_scaled_geometry() {
    let rows = [
        [2.0, 0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 3.0, -1.0],
    ];
    let ir = cube_with(rows);
    let expected = positions(&unit_cube().expect("the example cube builds"))
        .into_iter()
        .map(|point| transformed(rows, point))
        .collect::<Vec<_>>();
    let (losses, bytes) = written(&ir, IgesVersion::V5_3);
    assert!(
        charged(&losses, "iges/writer.body-omitted").is_empty(),
        "{losses:#?}"
    );
    assert!(
        charged(&losses, "iges/writer.face-omitted").is_empty(),
        "{losses:#?}"
    );
    assert!(directory(&bytes).contains(&(186, 0)));
    let read = decoded(&bytes);
    assert_eq!(read.geometry.bodies.len(), 1);
    assert!(read.geometry.bodies[0].transform.is_none());
    let actual = positions(&read);
    assert_eq!(actual.len(), expected.len(), "{actual:?}");
    assert_points_contain(&actual, &expected);
}

#[test]
fn a_reversed_face_writes_its_support_with_exchanged_parameters() {
    let ir = decoded(OPEN_SHELL);
    let (_, forward) = {
        let (_, bytes) = written(&ir, IgesVersion::V5_0);
        let read = decoded(&bytes);
        oriented_normal(&read, &read.geometry.faces[0])
    };
    let mut reversed = ir;
    reversed.geometry.faces[0].sense = Sense::Reversed;
    let (losses, bytes) = written(&reversed, IgesVersion::V5_0);
    assert!(
        charged(&losses, "iges/writer.face-omitted").is_empty(),
        "{losses:#?}"
    );
    let read = decoded(&bytes);
    assert_eq!(read.geometry.faces.len(), 1);
    let (_, normal) = oriented_normal(&read, &read.geometry.faces[0]);
    assert!(
        near(normal, forward.map(|value| -value)),
        "{normal:?} is not the reverse of {forward:?}"
    );
}

/// Assert that every face of `ir` has its oriented normal pointing away from
/// `centre`, a point inside the convex solid that the faces bound.
fn assert_faces_outward(ir: &CadIr, centre: [f64; 3]) {
    for face in &ir.geometry.faces {
        let (point, normal) = oriented_normal(ir, face);
        let outward = (0..3)
            .map(|axis| normal[axis] * (point[axis] - centre[axis]))
            .sum::<f64>();
        assert!(outward > 0.0, "{} faces inward: {normal:?}", face.id);
    }
}

/// The unit cube with its top face on a bilinear B-spline patch whose
/// parameter derivatives point into the cube, so that only the patch's
/// reversed-normal flag turns the face outward.
fn cube_with_reversed_normal_top() -> CadIr {
    let mut ir = unit_cube().expect("the example cube builds");
    add_document_root(&mut ir);
    let top = ir.geometry.faces[1].surface.clone();
    let patch = NurbsSurface::from_checked_lanes(
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            (0..2)
                .map(|y| {
                    (0..2)
                        .map(|x| Point3::new(f64::from(x), f64::from(y), 1.0))
                        .collect()
                })
                .collect(),
            None,
        ),
        true,
    )
    .expect("the patch builds");
    let surface = ir
        .geometry
        .surfaces
        .iter_mut()
        .find(|surface| surface.id == top)
        .expect("the top support");
    surface.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(patch));
    assert_faces_outward(&ir, [0.5; 3]);
    ir
}

#[test]
fn a_solid_face_on_a_reversed_normal_b_spline_reads_back_outward() {
    let ir = cube_with_reversed_normal_top();
    for version in [IgesVersion::V5_3, IgesVersion::V5_0] {
        let (losses, bytes) = written(&ir, version);
        assert!(
            charged(&losses, "iges/writer.face-omitted").is_empty(),
            "{version:?}: {losses:#?}"
        );
        assert!(
            charged(&losses, "iges/writer.body-omitted").is_empty(),
            "{version:?}: {losses:#?}"
        );
        let read = decoded(&bytes);
        assert_eq!(read.geometry.faces.len(), 6, "{version:?}");
        if version == IgesVersion::V5_3 {
            assert_eq!(read.geometry.bodies.len(), 1);
            assert_eq!(read.geometry.bodies[0].kind, BodyKind::Solid);
        }
        assert_faces_outward(&read, [0.5; 3]);
    }
}

#[test]
fn a_solid_before_5_1_writes_the_trimmed_surfaces_of_its_faces() {
    let mut ir = unit_cube().expect("the example cube builds");
    add_document_root(&mut ir);
    let body = ir.geometry.bodies[0].id.to_string();
    let (losses, bytes) = written(&ir, IgesVersion::V5_0);
    let unjoined = charged(&losses, "iges/writer.solid-unjoined");
    assert_eq!(unjoined.len(), 1, "{losses:#?}");
    assert!(unjoined[0].contains(&body), "{unjoined:?}");
    assert!(
        charged(&losses, "iges/writer.body-omitted").is_empty(),
        "{losses:#?}"
    );
    assert!(
        charged(&losses, "iges/writer.face-omitted").is_empty(),
        "{losses:#?}"
    );
    let entries = directory(&bytes);
    assert_eq!(
        entries
            .iter()
            .filter(|entry| matches!(entry.0, 143 | 144))
            .count(),
        6,
        "{entries:?}"
    );
    assert!(!entries.contains(&(186, 0)), "{entries:?}");
    let read = decoded(&bytes);
    assert_eq!(read.geometry.faces.len(), 6);
    assert_faces_outward(&read, [0.5; 3]);
}

#[test]
fn a_plane_face_without_parameter_curves_writes_computed_ones() {
    let ir = decoded(OPEN_SHELL);
    assert!(
        ir.geometry.pcurves.is_empty(),
        "the fixture states no pcurve"
    );
    let (losses, bytes) = written(&ir, IgesVersion::V5_3);
    assert!(
        charged(&losses, "iges/writer.face-omitted").is_empty(),
        "{losses:#?}"
    );
    let entries = directory(&bytes);
    assert!(entries.iter().any(|entry| entry.0 == 144), "{entries:?}");
    assert!(entries.iter().any(|entry| entry.0 == 142), "{entries:?}");
    let read = decoded(&bytes);
    assert_eq!(read.geometry.faces.len(), 1);
    assert!(!read.geometry.coedges.is_empty());
    assert!(read
        .geometry
        .coedges
        .iter()
        .all(|coedge| !coedge.pcurves.is_empty()));
}

#[test]
fn a_face_without_a_boundary_loop_writes_an_untrimmed_type_144() {
    let ir = decoded(LOOPLESS_FACE);
    assert_eq!(ir.geometry.faces.len(), 1);
    assert_eq!(ir.geometry.faces[0].loops.stated().count(), 0);
    let (losses, bytes) = written(&ir, IgesVersion::V5_3);
    assert!(
        charged(&losses, "iges/writer.face-omitted").is_empty(),
        "{losses:#?}"
    );
    assert!(
        charged(&losses, "export/output.empty").is_empty(),
        "{losses:#?}"
    );
    assert!(directory(&bytes).contains(&(144, 0)));
    let read = decoded(&bytes);
    assert_eq!(read.geometry.faces.len(), 1);
    assert_eq!(read.geometry.faces[0].loops.stated().count(), 0);
}

#[test]
fn a_line_pcurve_writes_as_a_degree_1_type_126() {
    let pcurve = Pcurve {
        id: "test:model:pcurve#line".try_into().expect("valid identity"),
        geometry: PcurveGeometry::Line(
            LinePcurve::try_new(Point2::new(1.0, 0.0), Point2::new(0.0, 1.0))
                .expect("a unit direction"),
        ),
        metadata: PcurveMetadata::general(None, FiniteVector::new([0.0, 2.0]), None),
        native_ref: None,
    };
    let mut ir = CadIr::empty();
    ir.geometry.pcurves.push(pcurve.clone());
    let index = TopologyIndex::new(&ir);
    let entity = pcurve_entity(&index, &pcurve).expect("a line pcurve is written");
    assert_eq!(entity.type_code, 126);
    let parameters = String::from_utf8(entity.parameter_body).expect("ASCII parameters");
    assert!(parameters.starts_with("1,1,"), "{parameters}");
}

#[test]
fn a_native_curve_without_a_directory_sequence_is_declared_dropped() {
    let mut ir = CadIr::empty();
    let mut fields = serde_json::Map::new();
    fields.insert("entity_type".into(), 126.into());
    fields.insert("form".into(), 0.into());
    let record = NativeRecord::new(
        "iges:native:entity#unsequenced"
            .try_into()
            .expect("valid identity"),
        fields,
    )
    .expect("a flat record");
    ir.native
        .namespace_mut(cadmpeg_ir::codec_id!("iges"))
        .set_arena_records("entities", vec![record])
        .expect("a valid arena name");
    let (_, native) = native_dispositions(&ir);
    assert_eq!(
        native.disposition("iges", "entities", "iges:native:entity#unsequenced"),
        cadmpeg_ir::codec::write::NativeDisposition::Dropped
    );
}

#[test]
fn a_solid_under_a_reflecting_non_uniform_map_reads_back_outward() {
    let rows = [
        [-2.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    let expected = positions(&unit_cube().expect("the example cube builds"))
        .into_iter()
        .map(|point| transformed(rows, point))
        .collect::<Vec<_>>();
    let (losses, bytes) = written(&cube_with(rows), IgesVersion::V5_3);
    for code in ["iges/writer.body-omitted", "iges/writer.face-omitted"] {
        assert!(charged(&losses, code).is_empty(), "{losses:#?}");
    }
    let read = decoded(&bytes);
    assert_eq!(read.geometry.bodies.len(), 1);
    assert_eq!(read.geometry.bodies[0].kind, BodyKind::Solid);
    assert!(read.geometry.bodies[0].transform.is_none());
    assert_eq!(read.geometry.faces.len(), 6);
    assert_points_contain(&positions(&read), &expected);
    assert_faces_outward(&read, [-1.0, 0.5, 0.5]);
}

/// A file of one independent Type 104 hyperbola.
fn hyperbola_file() -> Vec<u8> {
    owned_test_file(&[OwnedTestEntity {
        entity_type: 104,
        form: 2,
        label: "HYPERBOL".into(),
        status: "00000000",
        parameters:
            "104,0.25,0,-0.1111111111111111,0,0,-1,0,2,0,3.086161269630487,3.525603580931404;"
                .into(),
    }])
}

/// The solved carrier and the parameter interval of each edge of `ir`.
fn edge_curves(ir: &CadIr) -> Vec<(&SolvedCurveGeometry, [f64; 2])> {
    ir.geometry
        .edges
        .iter()
        .filter_map(|edge| {
            let curve = ir
                .geometry
                .curves
                .iter()
                .find(|curve| Some(&curve.id) == edge.carrier.curve())?;
            let [a, b] = edge.param_range().map(FiniteVector::get).or_else(|| {
                curve
                    .parameter_range
                    .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
            })?;
            Some((
                curve.geometry.solved().expect("an evaluable edge carrier"),
                [a.min(b), a.max(b)],
            ))
        })
        .collect()
}

/// The distance from `point` to the nearest of `curves`.
fn distance_to_curves(
    ctx: &DecodeContext<'_>,
    curves: &[(&SolvedCurveGeometry, [f64; 2])],
    point: [f64; 3],
) -> f64 {
    let distance = |geometry: &SolvedCurveGeometry, parameter: f64| {
        let at = curve_point_solved(ctx, geometry, parameter)
            .expect("a point on the curve")
            .get();
        (at.x - point[0])
            .hypot(at.y - point[1])
            .hypot(at.z - point[2])
    };
    curves
        .iter()
        .map(|(geometry, [start, end])| {
            let step = (end - start) / 256.0;
            let nearest = (0..=256)
                .min_by(|a, b| {
                    let at =
                        |index: i32| distance(geometry, f64::from(index).mul_add(step, *start));
                    at(*a).total_cmp(&at(*b))
                })
                .expect("a sample");
            let (mut low, mut high) = (
                f64::from(nearest - 1).mul_add(step, *start).max(*start),
                f64::from(nearest + 1).mul_add(step, *start).min(*end),
            );
            for _ in 0..100 {
                let left = (high - low).mul_add(1.0 / 3.0, low);
                let right = (high - low).mul_add(2.0 / 3.0, low);
                if distance(geometry, left) <= distance(geometry, right) {
                    high = right;
                } else {
                    low = left;
                }
            }
            distance(geometry, 0.5 * (low + high))
        })
        .fold(f64::INFINITY, f64::min)
}

#[test]
fn a_conic_or_composite_under_a_shearing_map_writes_its_exact_image() {
    let rows = [
        [2.0, 0.5, 0.0, 1.0],
        [0.0, 1.0, 0.3, -2.0],
        [0.2, 0.0, 1.5, 0.5],
    ];
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
        .expect("an evaluation context");
    for (label, fixture) in [
        ("ellipse", ELLIPSE.to_vec()),
        ("hyperbola", hyperbola_file()),
        ("parabola", PARABOLA.to_vec()),
        ("composite", COMPOSITE.to_vec()),
    ] {
        let mut ir = decoded(&fixture);
        assert!(!ir.geometry.bodies.is_empty(), "{label}");
        for body in &mut ir.geometry.bodies {
            body.transform = Transform::affine(rows);
        }
        let source = edge_curves(&ir);
        assert!(!source.is_empty(), "{label}");
        let (losses, bytes) = written(&ir, IgesVersion::V5_3);
        assert!(
            charged(&losses, "iges/writer.body-omitted").is_empty(),
            "{label}: {losses:#?}"
        );
        let read = decoded(&bytes);
        assert!(read
            .geometry
            .bodies
            .iter()
            .all(|body| body.transform.is_none()));
        let written_curves = edge_curves(&read);
        for (geometry, [start, end]) in source {
            for step in 0..=16 {
                let parameter = (f64::from(step) / 16.0).mul_add(end - start, start);
                let at = curve_point_solved(&ctx, geometry, parameter)
                    .expect("a point on the source curve")
                    .get();
                let placed = transformed(rows, [at.x, at.y, at.z]);
                let distance = distance_to_curves(&ctx, &written_curves, placed);
                assert!(
                    distance <= ON_CURVE,
                    "{label}: {placed:?} lies {distance} from the written curves"
                );
            }
        }
    }
}
