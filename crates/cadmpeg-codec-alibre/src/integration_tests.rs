// SPDX-License-Identifier: Apache-2.0
//! Codec-wide inspect and decode contracts over the committed Alibre specimens.
//!
//! The expected volumes and boxes follow from the design values below, which
//! are lengths in millimetres.

use std::f64::consts::PI;
use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, InspectOptions};
use cadmpeg_core::dialect::Admission;
use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions, DecodeResult};
use cadmpeg_ir::report::Severity;
use cadmpeg_measure::report::BodyMeasures;
use cadmpeg_measure::value::{BoxValue, Quantity};
use cadmpeg_measure::{MeasureInput, MeasureOptions, Measurer, Selection, SurfaceKind};

use crate::loss::AlibreLossCode;
use crate::AlibreCodec;

pub(crate) mod assembly;

/// Allowance of a measured value beyond its stated numeric error, relative to
/// the expected magnitude.
const MEASURE_RELATIVE_FLOOR: f64 = 1.0e-9;

const BOX_HALF_X: f64 = 10.0;
const BOX_HALF_Y: f64 = 15.0;
const BOX_HEIGHT: f64 = 10.0;
const CYLINDER_RADIUS: f64 = 10.0;
const CYLINDER_HEIGHT: f64 = 15.0;
const FILLET_RADIUS: f64 = 2.0;
const HOLE_RADIUS: f64 = 3.0;
const CONE_INNER_RADIUS: f64 = 5.0;
const CONE_OUTER_RADIUS: f64 = 15.0;
const CONE_HEIGHT: f64 = 10.0;
const LOFT_HALF_WIDTH: f64 = 10.0;
const LOFT_HEIGHT: f64 = 20.0;

const BOX: &[u8] = include_bytes!("../../../corpus/alibre/box.AD_PRT");
const BOX_HOLE: &[u8] = include_bytes!("../../../corpus/alibre/box_hole.AD_PRT");
const CONE: &[u8] = include_bytes!("../../../corpus/alibre/cone.AD_PRT");
const CYLINDER: &[u8] = include_bytes!("../../../corpus/alibre/cylinder.AD_PRT");
const CYLINDER_FILLET: &[u8] = include_bytes!("../../../corpus/alibre/cylinder_fillet.AD_PRT");
const LOFT: &[u8] = include_bytes!("../../../corpus/alibre/loft.AD_PRT");
const LOFT_TWIN: &[u8] = include_bytes!("../../../corpus/alibre/loft.stp");

pub(crate) fn specimens() -> [(&'static str, &'static [u8]); 6] {
    [
        ("box", BOX),
        ("box_hole", BOX_HOLE),
        ("cone", CONE),
        ("cylinder", CYLINDER),
        ("cylinder_fillet", CYLINDER_FILLET),
        ("loft", LOFT),
    ]
}

fn decode(bytes: &[u8]) -> DecodeResult {
    AlibreCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
}

fn loss_kinds(result: &DecodeResult) -> Vec<cadmpeg_ir::report::loss::LossKind> {
    result
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.clone())
        .collect()
}

fn primary_dialect(result: &DecodeResult) -> &str {
    result
        .ir()
        .source
        .as_ref()
        .and_then(|source| source.dialect())
        .map(|matched| matched.dialect().as_str())
        .unwrap()
}

fn measured_body(ir: &cadmpeg_ir::CadIr) -> BodyMeasures {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0).unwrap();
    let input = MeasureInput {
        ir,
        fidelity: None,
        geometry_transferred: true,
    };
    let report = Measurer::new(input, MeasureOptions::default())
        .measure(&ctx, &Selection::default())
        .unwrap();
    ctx.finish_session().unwrap();
    let [body] = report.bodies().as_slice() else {
        panic!("one body row");
    };
    body.clone()
}

fn assert_volume(body: &BodyMeasures, expected_mm3: f64) {
    let measured = body
        .volume()
        .as_ref()
        .and_then(Quantity::measured)
        .expect("volume is measured");
    let (value, error) = (measured.value().value.get(), measured.numeric_error().get());
    let allowed = error + MEASURE_RELATIVE_FLOOR * expected_mm3.abs();
    assert!(
        (value - expected_mm3).abs() <= allowed,
        "volume: measured {value}, expected {expected_mm3}, allowed {allowed}"
    );
}

fn assert_box(body: &BodyMeasures, min: [f64; 3], max: [f64; 3]) {
    let measured: &BoxValue = body
        .bbox()
        .measured()
        .expect("bounding box is measured")
        .value();
    let allowed = MEASURE_RELATIVE_FLOOR * 10.0;
    for (corner, expected) in [(&measured.min, min), (&measured.max, max)] {
        for (actual, expected) in [corner.x, corner.y, corner.z].into_iter().zip(expected) {
            assert!(
                (actual - expected).abs() <= allowed,
                "box coordinate {actual}, expected {expected}"
            );
        }
    }
}

fn face_total(body: &BodyMeasures) -> usize {
    body.surface_kinds().values().sum()
}

fn kind_count(body: &BodyMeasures, kind: SurfaceKind) -> usize {
    body.surface_kinds().get(&kind).copied().unwrap_or(0)
}

#[test]
fn inspect_lists_the_streams_and_both_dialect_layers() {
    let summary = AlibreCodec
        .inspect(&mut Cursor::new(BOX), &InspectOptions::default())
        .unwrap();
    assert_eq!(summary.format(), "alibre");
    assert_eq!(summary.container_kind.as_str(), "cfb");
    let dialects = summary.dialects().unwrap();
    assert_eq!(
        dialects.primary().dialect().as_str(),
        "alibre:cfb-acis-binary"
    );
    assert_eq!(dialects.primary().admission(), &Admission::Admitted);
    let sab = summary
        .entries
        .iter()
        .find(|entry| entry.name == "SAB_DATA")
        .unwrap();
    assert_eq!(sab.role.as_str(), "acis-binary");
    assert_eq!(sab.attributes["acis_save_format_version"], "3300");
    assert_eq!(sab.attributes["header_scale"], "10");
    let names: Vec<_> = summary
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    for stream in ["STEP_DATA", "PROP_DATA", "VERSION"] {
        assert!(names.contains(&stream), "{names:?}");
    }
}

#[test]
fn detection_claims_a_part_and_no_other_compound_file() {
    let detect = |bytes: &[u8]| {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default()).unwrap();
        let confidence = AlibreCodec.detect(&ctx, root).unwrap();
        ctx.finish_session().unwrap();
        confidence
    };
    assert_eq!(detect(BOX), Confidence::High);
    assert_eq!(
        detect(&cadmpeg_test_support::compound::compound_fixture()),
        Confidence::No
    );
    assert_eq!(detect(LOFT_TWIN), Confidence::No);
}

#[test]
fn every_specimen_decodes_to_a_checked_solid() {
    for (name, bytes) in specimens() {
        let result = decode(bytes);
        assert!(result.report().geometry_transferred(), "{name}");
        assert_eq!(primary_dialect(&result), "alibre:cfb-acis-binary", "{name}");
        let report = cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).unwrap();
        let errors: Vec<_> = report
            .findings
            .iter()
            .filter(|finding| finding.severity >= Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{name}: {errors:?}");
        assert_eq!(result.ir().geometry.bodies.len(), 1, "{name}");
        let kinds = loss_kinds(&result);
        assert!(kinds.contains(&AlibreLossCode::SourceDialectUnverified.kind()));
        assert!(kinds.contains(&AlibreLossCode::SourceStreamsUndecoded.kind()));
    }
}

#[test]
fn face_counts_follow_the_modelled_features() {
    for (name, bytes, faces) in [
        ("box", BOX, 6),
        ("box_hole", BOX_HOLE, 7),
        ("cone", CONE, 3),
        ("cylinder", CYLINDER, 3),
        ("cylinder_fillet", CYLINDER_FILLET, 4),
        ("loft", LOFT, 6),
    ] {
        let result = decode(bytes);
        assert_eq!(result.ir().geometry.faces.len(), faces, "{name}");
    }
}

#[test]
fn a_box_measures_its_extrusion() {
    let body = measured_body(decode(BOX).ir());
    assert_eq!(kind_count(&body, SurfaceKind::Plane), 6);
    assert_volume(&body, 2.0 * BOX_HALF_X * 2.0 * BOX_HALF_Y * BOX_HEIGHT);
    assert_box(
        &body,
        [-BOX_HALF_X, -BOX_HALF_Y, 0.0],
        [BOX_HALF_X, BOX_HALF_Y, BOX_HEIGHT],
    );
}

#[test]
fn a_through_hole_removes_its_cylinder() {
    let body = measured_body(decode(BOX_HOLE).ir());
    assert_eq!(kind_count(&body, SurfaceKind::Plane), 6);
    assert_eq!(kind_count(&body, SurfaceKind::Cylinder), 1);
    let block = (2.0 * BOX_HALF_X) * (2.0 * BOX_HALF_Y) * BOX_HEIGHT;
    let hole = PI * HOLE_RADIUS * HOLE_RADIUS * BOX_HEIGHT;
    assert_volume(&body, block - hole);
}

#[test]
fn a_cylinder_measures_its_extrusion() {
    let body = measured_body(decode(CYLINDER).ir());
    assert_eq!(kind_count(&body, SurfaceKind::Plane), 2);
    assert_eq!(kind_count(&body, SurfaceKind::Cylinder), 1);
    assert_volume(
        &body,
        PI * CYLINDER_RADIUS * CYLINDER_RADIUS * CYLINDER_HEIGHT,
    );
    assert_box(
        &body,
        [-CYLINDER_RADIUS, -CYLINDER_RADIUS, 0.0],
        [CYLINDER_RADIUS, CYLINDER_RADIUS, CYLINDER_HEIGHT],
    );
}

#[test]
fn a_top_edge_fillet_is_a_torus_that_removes_its_corner_ring() {
    let body = measured_body(decode(CYLINDER_FILLET).ir());
    assert_eq!(kind_count(&body, SurfaceKind::Torus), 1);
    assert_eq!(face_total(&body), 4);
    // Pappus: the removed section is the square of the fillet radius less the
    // quarter disc, revolved about the axis at its centroid.
    let rf = FILLET_RADIUS;
    let section = rf * rf * (1.0 - PI / 4.0);
    let inward =
        (rf * rf * rf / 2.0 - (PI * rf * rf / 4.0) * (rf - 4.0 * rf / (3.0 * PI))) / section;
    let removed = 2.0 * PI * (CYLINDER_RADIUS - inward) * section;
    let cylinder = PI * CYLINDER_RADIUS * CYLINDER_RADIUS * CYLINDER_HEIGHT;
    assert_volume(&body, cylinder - removed);
}

#[test]
fn a_revolved_triangle_is_a_cone_frustum_ring() {
    let body = measured_body(decode(CONE).ir());
    assert_eq!(kind_count(&body, SurfaceKind::Cone), 1);
    assert_eq!(kind_count(&body, SurfaceKind::Cylinder), 1);
    assert_eq!(kind_count(&body, SurfaceKind::Plane), 1);
    // Pappus on the right triangle with legs CONE_OUTER_RADIUS -
    // CONE_INNER_RADIUS and CONE_HEIGHT, whose corners lie at the inner radius
    // and the outer radius.
    let area = 0.5 * (CONE_OUTER_RADIUS - CONE_INNER_RADIUS) * CONE_HEIGHT;
    let centroid = (2.0 * CONE_INNER_RADIUS + CONE_OUTER_RADIUS) / 3.0;
    assert_volume(&body, 2.0 * PI * centroid * area);
    assert_box(
        &body,
        [-CONE_OUTER_RADIUS, -CONE_OUTER_RADIUS, 0.0],
        [CONE_OUTER_RADIUS, CONE_OUTER_RADIUS, CONE_HEIGHT],
    );
}

#[test]
fn a_loft_keeps_its_spline_faces_as_cached_carriers() {
    let result = decode(LOFT);
    assert_eq!(result.ir().geometry.faces.len(), 6);
    assert_eq!(result.ir().geometry.procedural_surfaces.len(), 4);
    assert!(loss_kinds(&result).contains(&AlibreLossCode::GeometryProceduralSurfaceUntyped.kind()));
    let nurbs = result
        .ir()
        .geometry
        .surfaces
        .iter()
        .filter(|surface| {
            matches!(
                surface.geometry.solved(),
                Some(cadmpeg_ir::geometry::SolvedSurfaceGeometry::Nurbs(_))
            )
        })
        .count();
    assert_eq!(nurbs, 4);
}

#[test]
fn the_step_twin_of_the_loft_spans_the_design_box() {
    let result = cadmpeg_codec_step::StepCodec::default()
        .decode(&mut Cursor::new(LOFT_TWIN), &DecodeOptions::default())
        .unwrap();
    let body = measured_body(result.ir());
    assert_box(
        &body,
        [-LOFT_HALF_WIDTH, -LOFT_HALF_WIDTH, 0.0],
        [LOFT_HALF_WIDTH, LOFT_HALF_WIDTH, LOFT_HEIGHT],
    );
}

/// Overwrites the ACIS magic of the `SAB_DATA` stream in place.
fn without_acis_magic(bytes: &[u8]) -> Vec<u8> {
    let magic = b"ACIS BinaryFile";
    let at = bytes
        .windows(magic.len())
        .position(|window| window == magic)
        .unwrap();
    let mut patched = bytes.to_vec();
    patched[at] = b'X';
    patched
}

#[test]
fn a_stream_without_the_acis_magic_is_inspected_as_unknown_and_refused() {
    let patched = without_acis_magic(BOX);
    let summary = AlibreCodec
        .inspect(&mut Cursor::new(&patched), &InspectOptions::default())
        .unwrap();
    let primary = summary.dialects().unwrap().primary();
    assert_eq!(primary.dialect().as_str(), "alibre:unknown");
    assert_eq!(primary.admission(), &Admission::Refused);
    let failure = AlibreCodec
        .decode(&mut Cursor::new(&patched), &DecodeOptions::default())
        .unwrap_err();
    assert!(
        matches!(
            failure,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::UnsupportedDialect { .. })
        ),
        "{failure:?}"
    );
}
