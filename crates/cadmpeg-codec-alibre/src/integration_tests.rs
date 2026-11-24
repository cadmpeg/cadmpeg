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
        // Every specimen is at save format 33.0, which stores the extended entity base.
        assert!(
            kinds.contains(&AlibreLossCode::SourceRecordExtensionsUnprojected.kind()),
            "{name}"
        );
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
    // The loft's coedges name pcurve records, and the IR holds none of them.
    assert!(result.ir().geometry.pcurves.is_empty());
    assert!(loss_kinds(&result).contains(&AlibreLossCode::GeometryPcurveUndecoded.kind()));
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

/// The distinct vertex positions of an IR, in millimetres, rounded to a
/// nanometre and in order.
fn corner_points(ir: &cadmpeg_ir::CadIr) -> Vec<[f64; 3]> {
    let mut corners: Vec<[f64; 3]> = ir
        .geometry
        .vertices
        .iter()
        .map(|vertex| {
            let point = ir
                .geometry
                .points
                .iter()
                .find(|point| point.id == vertex.point)
                .expect("vertex point");
            let at = point.position();
            [at.x, at.y, at.z].map(|coordinate| (coordinate * 1.0e6).round())
        })
        .collect();
    corners.sort_by(|a, b| {
        a.iter()
            .zip(b)
            .map(|(a, b)| a.total_cmp(b))
            .find(|order| order.is_ne())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    corners.dedup();
    corners
}

/// The part and its Alibre-exported STEP twin are one solid, so the IR of the
/// part holds what the IR of the twin holds: the same vertices, the same
/// faces, and the same planar caps. The measurer states no box and no volume
/// for a body with B-spline faces, so the twin carries the design box and the
/// part is compared with the twin through its points and faces.
#[test]
fn the_loft_is_the_solid_its_step_twin_is() {
    let twin = cadmpeg_codec_step::StepCodec::default()
        .decode(&mut Cursor::new(LOFT_TWIN), &DecodeOptions::default())
        .unwrap();
    let part = decode(LOFT);
    let twin_body = measured_body(twin.ir());
    assert_box(
        &twin_body,
        [-LOFT_HALF_WIDTH, -LOFT_HALF_WIDTH, 0.0],
        [LOFT_HALF_WIDTH, LOFT_HALF_WIDTH, LOFT_HEIGHT],
    );
    let (part_corners, twin_corners) = (corner_points(part.ir()), corner_points(twin.ir()));
    assert_eq!(part_corners.len(), 8);
    assert_eq!(part_corners, twin_corners);
    assert_eq!(
        part.ir().geometry.faces.len(),
        twin.ir().geometry.faces.len()
    );
    assert_eq!(
        part.ir().geometry.edges.len(),
        twin.ir().geometry.edges.len()
    );
    let planes = |ir: &cadmpeg_ir::CadIr| {
        ir.geometry
            .surfaces
            .iter()
            .filter(|surface| {
                matches!(
                    surface.geometry.solved(),
                    Some(cadmpeg_ir::geometry::SolvedSurfaceGeometry::Plane(_))
                )
            })
            .count()
    };
    assert_eq!(planes(part.ir()), 2);
    assert_eq!(planes(twin.ir()), 2);
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

const FREE_SECTOR: u32 = 0xffff_ffff;

/// Where the sectors of the `SAB_DATA` stream of a version 3 or 4 compound
/// file with one FAT sector lie in the file, and the stream length.
struct SabSectors {
    offsets: Vec<usize>,
    length: usize,
    /// File offset of the FAT entry of the last sector.
    last_fat_entry: usize,
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn sab_sectors(bytes: &[u8]) -> SabSectors {
    let sector = 1_usize << u16::from_le_bytes([bytes[30], bytes[31]]);
    let sector_at = |id: u32| sector * (usize::try_from(id).unwrap() + 1);
    let fat = sector_at(u32_at(bytes, 76));
    let fat_next = |id: u32| u32_at(bytes, fat + 4 * usize::try_from(id).unwrap());
    let mut found = None;
    let mut directory = u32_at(bytes, 48);
    while directory < 0xffff_fff0 {
        for entry in 0..sector / 128 {
            let at = sector_at(directory) + entry * 128;
            let name_len = usize::from(u16::from_le_bytes([bytes[at + 64], bytes[at + 65]]));
            let name: Vec<u16> = bytes[at..at + name_len.saturating_sub(2)]
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            if String::from_utf16_lossy(&name) == "SAB_DATA" {
                let length = usize::try_from(u32_at(bytes, at + 120)).unwrap();
                found = Some((u32_at(bytes, at + 116), length));
            }
        }
        directory = fat_next(directory);
    }
    let (mut id, length) = found.expect("SAB_DATA entry");
    let mut offsets = Vec::new();
    let mut last_fat_entry = 0;
    while id < 0xffff_fff0 {
        offsets.push(sector_at(id));
        last_fat_entry = fat + 4 * usize::try_from(id).unwrap();
        id = u32_at(bytes, last_fat_entry);
    }
    SabSectors {
        offsets,
        length,
        last_fat_entry,
    }
}

/// The part with its `SAB_DATA` stream edited in place. The edit keeps the
/// stream length.
fn with_sab(bytes: &[u8], edit: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let sectors = sab_sectors(bytes);
    let sector = 1_usize << u16::from_le_bytes([bytes[30], bytes[31]]);
    let mut stream: Vec<u8> = sectors
        .offsets
        .iter()
        .flat_map(|&at| bytes[at..at + sector].iter().copied())
        .take(sectors.length)
        .collect();
    edit(&mut stream);
    assert_eq!(stream.len(), sectors.length);
    let mut patched = bytes.to_vec();
    for (chunk, &at) in stream.chunks(sector).zip(&sectors.offsets) {
        patched[at..at + chunk.len()].copy_from_slice(chunk);
    }
    patched
}

fn position(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
        .unwrap()
}

#[test]
fn an_unterminated_stream_chain_is_charged_as_noncanonical() {
    let sectors = sab_sectors(BOX);
    let mut patched = BOX.to_vec();
    patched[sectors.last_fat_entry..sectors.last_fat_entry + 4]
        .copy_from_slice(&FREE_SECTOR.to_le_bytes());
    let result = decode(&patched);
    assert!(result.report().geometry_transferred());
    assert!(loss_kinds(&result).contains(&AlibreLossCode::ContainerNoncanonical.kind()));
    assert!(!loss_kinds(&decode(BOX)).contains(&AlibreLossCode::ContainerNoncanonical.kind()));
}

#[test]
fn a_header_scale_other_than_the_centimetre_is_charged() {
    let patched = with_sab(BOX, |stream| {
        // The first header double, tagged `0x06`, is the scale 10.0.
        let scale = position(stream, &[0x06, 0, 0, 0, 0, 0, 0, 0x24, 0x40]);
        stream[scale + 1..scale + 9].copy_from_slice(&1.0_f64.to_le_bytes());
    });
    assert!(loss_kinds(&decode(&patched)).contains(&AlibreLossCode::HeaderScaleUnverified.kind()));
    assert!(!loss_kinds(&decode(BOX)).contains(&AlibreLossCode::HeaderScaleUnverified.kind()));
}

#[test]
fn a_header_tolerance_that_is_not_positive_is_charged_and_left_at_its_default() {
    let patched = with_sab(BOX, |stream| {
        // The header doubles are tagged `0x06`: the scale 10.0, then the
        // resolved absolute tolerance, then the resolved normal tolerance.
        let scale = position(stream, &[0x06, 0, 0, 0, 0, 0, 0, 0x24, 0x40]);
        for at in [scale + 9, scale + 18] {
            stream[at + 1..at + 9].copy_from_slice(&0.0_f64.to_le_bytes());
        }
    });
    let result = decode(&patched);
    let unresolved = loss_kinds(&result)
        .into_iter()
        .filter(|kind| *kind == AlibreLossCode::HeaderToleranceUnresolved.kind())
        .count();
    assert_eq!(
        unresolved, 2,
        "one note each for the linear and angular tolerance"
    );
    assert_eq!(
        result.ir().tolerances,
        cadmpeg_ir::units::Tolerances::default()
    );
}

#[test]
fn a_stream_whose_geometry_classes_are_unknown_decodes_no_carriers() {
    // The first use of each identifier loses its last letter to `z`: a class
    // the decoder does not know, so no record becomes a face, surface, or point.
    let patched = with_sab(BOX, |stream| {
        for name in ["face", "plane", "point"] {
            let first_use = format!("{name}%");
            let at = position(stream, first_use.as_bytes());
            stream[at + name.len() - 1] = b'z';
        }
    });
    let result = decode(&patched);
    assert!(!result.report().geometry_transferred());
    assert!(loss_kinds(&result).contains(&AlibreLossCode::GeometryFramedWithoutCarriers.kind()));
}
