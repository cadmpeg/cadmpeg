// SPDX-License-Identifier: Apache-2.0
//! End-to-end contracts for NX assembly placement frames and their neutral
//! occurrences.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::products::{ExternalDocument, Occurrence, OccurrenceParent, PrototypeReference};
use serde_json::Value;

use crate::loss::NxLossCode;
use crate::test_support::test_om::offset_only_indexed_om_section;
use crate::test_support::test_prt::prt_with_named_payloads;
use crate::NxCodec;

const FRAME_UUID: &str = "01234567-89ab-cdef-0123-456789abcdef";
const PROTOTYPE: &str = "pin";

fn append_fastload_string(bytes: &mut Vec<u8>, tag: u8, value: &str) {
    bytes.push(tag);
    bytes.push(u8::try_from(value.len() + 2).unwrap());
    bytes.extend(value.as_bytes());
    bytes.push(0);
}

fn fastload_structure_with_two_same_prototype_uses(uuid: &str) -> Vec<u8> {
    let mut bytes = vec![1, 2, 0x42, 0];
    bytes.extend([1, 2]);
    append_fastload_string(&mut bytes, 4, "MODEL");
    bytes.extend([1, 3, 0, 0, 1, 3, b'9', b'9']);
    bytes.extend([1, 2, 0xff, 0xff, 0xff, 0xff, 1, 2]);
    append_fastload_string(&mut bytes, 4, PROTOTYPE);
    bytes.extend([1, 3, 1, 1]);
    bytes.extend([1, 2, 3, 38]);
    bytes.extend(uuid.as_bytes());
    bytes.push(0);
    bytes.extend([1, 3, 1, 1]);

    let mut structure = vec![0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0];
    structure.extend(u32::try_from(bytes.len()).unwrap().to_be_bytes());
    structure.extend(bytes);
    structure
}

/// Eight-byte placement atom: the big-endian binary64 of `4 * value` with
/// `0x10` subtracted from its first byte.
fn placement_atom(value: f64) -> Vec<u8> {
    let mut atom = (4.0 * value).to_be_bytes().to_vec();
    atom[0] -= 0x10;
    atom
}

fn placement_with_translation(uuid: &str, translation: [f64; 3]) -> Vec<u8> {
    let mut frame = crate::layout::assembly_placement_prefix::MAGIC_VALUE.to_vec();
    for (row, value) in translation.into_iter().enumerate() {
        for column in 0..3 {
            frame.push(u8::from(row == column));
        }
        frame.extend(placement_atom(value));
    }
    frame.extend([0, 0, 0, 1, 1, 0xf0, 0, 0, 0]);
    append_fastload_string(&mut frame, 3, "instance");
    append_fastload_string(&mut frame, 3, uuid);
    frame
}

fn decode(file: Vec<u8>) -> DecodeResult {
    NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap()
}

fn has_loss(result: &DecodeResult, code: NxLossCode) -> bool {
    result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == code.kind())
}

fn component_occurrences(result: &DecodeResult) -> Vec<&Occurrence> {
    result
        .ir()
        .model
        .occurrences
        .iter()
        .filter(|occurrence| matches!(occurrence.parent, OccurrenceParent::Occurrence { .. }))
        .collect()
}

fn native_counts(result: &DecodeResult) -> (usize, usize) {
    let namespace = result.ir().native.namespace("nx").unwrap();
    let placements: Vec<Value> = namespace.arena_as("assembly_placements").unwrap();
    let uses: Vec<Value> = namespace
        .arena_as("fast_load_component_occurrences")
        .unwrap();
    (placements.len(), uses.len())
}

fn two_placement_file(om_section: Option<Vec<u8>>, placement_count: usize) -> Vec<u8> {
    let mut part = om_section.unwrap_or_default();
    for index in 0..placement_count {
        let offset = f64::from(u32::try_from(index).unwrap());
        part.extend(placement_with_translation(
            FRAME_UUID,
            [12.5 + offset, -3.25, 40.0],
        ));
    }
    prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", part),
        (
            "/Root/FastLoad/Structure",
            fastload_structure_with_two_same_prototype_uses(FRAME_UUID),
        ),
    ])
}

#[test]
fn millimeter_part_emits_placements_of_a_single_prototype_uuid_group() {
    let result = decode(two_placement_file(
        Some(offset_only_indexed_om_section()),
        2,
    ));

    let occurrences = component_occurrences(&result);
    assert_eq!(occurrences.len(), 2);
    for (ordinal, occurrence) in occurrences.iter().enumerate() {
        assert_eq!(occurrence.name.as_deref(), Some(PROTOTYPE));
        assert_eq!(
            occurrence.prototype,
            PrototypeReference::External {
                document: ExternalDocument::Missing {},
                object: None,
            }
        );
        let rows = occurrence.transform.affine_rows();
        let x = 12.5 + f64::from(u32::try_from(ordinal).unwrap());
        assert!((rows[0][3] - x).abs() < 1e-12);
        assert!((rows[1][3] + 3.25).abs() < 1e-12);
        assert!((rows[2][3] - 40.0).abs() < 1e-12);
        assert!((rows[0][0] - 1.0).abs() < 1e-12);
    }
    assert!(has_loss(
        &result,
        NxLossCode::AssemblyPlacementUnitFromExpressions
    ));
    assert!(has_loss(
        &result,
        NxLossCode::AssemblyExternalDocumentUnresolved
    ));
    assert!(!has_loss(
        &result,
        NxLossCode::AssemblyPlacementFrameUnresolved
    ));
}

#[test]
fn part_without_length_expressions_withholds_its_placements() {
    let result = decode(two_placement_file(None, 2));

    assert!(result.ir().model.occurrences.is_empty());
    assert!(result.ir().model.product_definitions.is_empty());
    assert_eq!(native_counts(&result), (2, 2));
    assert!(has_loss(
        &result,
        NxLossCode::AssemblyPlacementFrameUnresolved
    ));
    assert!(!has_loss(
        &result,
        NxLossCode::AssemblyPlacementUnitFromExpressions
    ));
}

#[test]
fn unequal_placement_and_use_counts_withhold_the_uuid_group() {
    let result = decode(two_placement_file(
        Some(offset_only_indexed_om_section()),
        1,
    ));

    assert!(result.ir().model.occurrences.is_empty());
    assert_eq!(native_counts(&result), (1, 2));
    assert!(has_loss(
        &result,
        NxLossCode::AssemblyPlacementFrameUnresolved
    ));
}

/// A STEP twin occurrence: child filename, translation, and the sign of the
/// child z axis along the assembly z axis. Its rotation turns about the child
/// z axis or reverses it, and fixes the off-axis centres of the box, cover,
/// plate, and window.
struct StepUse {
    child: &'static str,
    translation: [f64; 3],
    z_sign: f64,
}

const fn step_use(child: &'static str, translation: [f64; 3], z_sign: f64) -> StepUse {
    StepUse {
        child,
        translation,
        z_sign,
    }
}

const NUT: &str = "90591A141 HEX NUT.prt";
const CAP_SCREW_SHORT: &str = "91274A118 SHCS.prt";
const CAP_SCREW_LONG: &str = "91274A141 SHCS.prt";
const FLAT_HEAD_SCREW: &str = "91304A112 FHSCS.prt";
const BOX: &str = "NIST mtc crada box.prt";
const COVER: &str = "NIST mtc crada cover.prt";
const PLATE: &str = "NIST mtc crada plate.prt";
const WINDOW: &str = "NIST mtc crada polycarb.prt";

/// Child placements of the NIST MTC assembly STEP twin, which `SolidWorks` wrote
/// with its own child-part frames.
const STEP_USES: [StepUse; 36] = [
    step_use(NUT, [83.35, 22.225, 87.275], 1.0),
    step_use(NUT, [83.35, 79.375, 87.275], 1.0),
    step_use(NUT, [96.35, 65.0875, 87.275], 1.0),
    step_use(NUT, [96.35, 36.5125, 87.275], 1.0),
    step_use(NUT, [61.35, 22.225, 87.275], 1.0),
    step_use(NUT, [39.35, 22.225, 87.275], 1.0),
    step_use(NUT, [26.35, 36.5125, 87.275], 1.0),
    step_use(NUT, [26.35, 65.0875, 87.275], 1.0),
    step_use(NUT, [39.35, 79.375, 87.275], 1.0),
    step_use(NUT, [61.35, 79.375, 87.275], 1.0),
    step_use(CAP_SCREW_SHORT, [146.9, 5.5, 64.0], -1.0),
    step_use(CAP_SCREW_SHORT, [146.9, 96.1, 64.0], -1.0),
    step_use(CAP_SCREW_SHORT, [5.5, 96.1, 64.0], -1.0),
    step_use(CAP_SCREW_SHORT, [5.5, 5.5, 64.0], -1.0),
    step_use(CAP_SCREW_SHORT, [26.35, 36.5125, 83.05], -1.0),
    step_use(CAP_SCREW_SHORT, [26.35, 65.0875, 83.05], -1.0),
    step_use(CAP_SCREW_SHORT, [39.35, 79.375, 83.05], -1.0),
    step_use(CAP_SCREW_SHORT, [61.35, 79.375, 83.05], -1.0),
    step_use(CAP_SCREW_SHORT, [83.35, 79.375, 83.05], -1.0),
    step_use(CAP_SCREW_SHORT, [96.35, 65.0875, 83.05], -1.0),
    step_use(CAP_SCREW_SHORT, [96.35, 36.5125, 83.05], -1.0),
    step_use(CAP_SCREW_SHORT, [83.35, 22.225, 83.05], -1.0),
    step_use(CAP_SCREW_SHORT, [61.35, 22.225, 83.05], -1.0),
    step_use(CAP_SCREW_SHORT, [39.35, 22.225, 83.05], -1.0),
    step_use(CAP_SCREW_LONG, [131.680_835, 20.719_164_9, -8.6], -1.0),
    step_use(CAP_SCREW_LONG, [20.719_164_9, 20.719_164_9, -8.6], -1.0),
    step_use(CAP_SCREW_LONG, [131.680_835, 80.880_835_1, -8.6], -1.0),
    step_use(CAP_SCREW_LONG, [20.719_164_9, 80.880_835_1, -8.6], -1.0),
    step_use(FLAT_HEAD_SCREW, [117.930_835, 20.719_164_9, 22.0], 1.0),
    step_use(FLAT_HEAD_SCREW, [117.930_835, 80.879_164_9, 22.0], 1.0),
    step_use(FLAT_HEAD_SCREW, [34.470_835_1, 20.719_164_9, 22.0], 1.0),
    step_use(FLAT_HEAD_SCREW, [34.470_835_1, 80.879_164_9, 22.0], 1.0),
    step_use(BOX, [0.0, 0.0, 0.0], 1.0),
    step_use(COVER, [0.0, 0.0, 70.0], 1.0),
    step_use(PLATE, [16.200_835_1, 15.219_164_9, 17.4], 1.0),
    step_use(WINDOW, [20.85, 16.6875, 88.875], 1.0),
];

/// The centre of each child part, in the NX child frame and in the STEP twin's
/// child frame. Fastener centres lie on the child z axis.
fn child_centres(child: &str) -> ([f64; 3], [f64; 3]) {
    match child {
        NUT => ([0.0, 0.0, 1.6], [0.0, 0.0, 0.0]),
        CAP_SCREW_SHORT => ([0.0, 0.0, -4.0], [0.0, 0.0, -8.0]),
        CAP_SCREW_LONG => ([0.0, 0.0, -7.0], [0.0, 0.0, -13.0]),
        FLAT_HEAD_SCREW => ([0.0, 0.0, -5.0], [0.0, 0.0, -5.0]),
        BOX => ([72.2, 46.8, 33.5], [76.2, 50.8, 33.5]),
        COVER => ([76.2, 50.8, 12.525], [76.2, 50.8, 12.525]),
        PLATE => ([35.58, 60.0, 2.5], [60.0, 35.58, 2.5]),
        WINDOW => ([40.5, 34.1, -1.5875], [40.5, 34.1, 1.5875]),
        _ => panic!("child {child} has no STEP twin"),
    }
}

/// The NX re-creation of the assembly mates its fasteners up to 0.05 mm and
/// 0.006 rad away from the `SolidWorks` original.
const MTC_CENTRE_TOLERANCE_MM: f64 = 0.06;
const MTC_AXIS_TOLERANCE_RAD: f64 = 0.01;

fn add(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [left[0] + right[0], left[1] + right[1], left[2] + right[2]]
}

fn distance(left: [f64; 3], right: [f64; 3]) -> f64 {
    let d = [left[0] - right[0], left[1] - right[1], left[2] - right[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

fn apply(rows: [[f64; 4]; 3], point: [f64; 3]) -> [f64; 3] {
    let mut out = [0.0; 3];
    for (row, value) in rows.iter().zip(&mut out) {
        *value = row[0] * point[0] + row[1] * point[1] + row[2] * point[2] + row[3];
    }
    out
}

fn child_filename(occurrence: &Occurrence) -> &str {
    match &occurrence.prototype {
        PrototypeReference::External {
            document: ExternalDocument::Path { path },
            ..
        } => path.as_str(),
        other => panic!("MTC occurrence prototype {other:?} has no child filename"),
    }
}

#[test]
fn mtc_occurrences_place_every_child_where_the_step_twin_does() {
    let bytes = include_bytes!("../../../../corpus/nist-pmi/nx/NIST mtc crada assembly.prt");
    let result = decode(bytes.to_vec());
    let occurrences = component_occurrences(&result);
    assert_eq!(occurrences.len(), STEP_USES.len());
    assert!(!has_loss(
        &result,
        NxLossCode::AssemblyPlacementFrameUnresolved
    ));
    assert!(!has_loss(
        &result,
        NxLossCode::AssemblyOccurrencesUnresolved
    ));

    let world = |occurrence: &Occurrence| {
        let rows = occurrence.transform.affine_rows();
        let centre = apply(rows, child_centres(child_filename(occurrence)).0);
        let axis = [rows[0][2], rows[1][2], rows[2][2]];
        (centre, axis)
    };
    let box_occurrence = occurrences
        .iter()
        .find(|occurrence| child_filename(occurrence) == BOX)
        .expect("MTC places the box");
    let step_box_centre = child_centres(BOX).1;
    let nx_box_centre = world(box_occurrence).0;
    let registration = [
        step_box_centre[0] - nx_box_centre[0],
        step_box_centre[1] - nx_box_centre[1],
        step_box_centre[2] - nx_box_centre[2],
    ];

    let mut matched = [false; STEP_USES.len()];
    for occurrence in &occurrences {
        let child = child_filename(occurrence);
        let (centre, axis) = world(occurrence);
        let centre = add(centre, registration);
        let candidates: Vec<usize> = STEP_USES
            .iter()
            .enumerate()
            .filter(|(index, step)| {
                let local = child_centres(step.child).1;
                let step_centre = add(
                    step.translation,
                    [local[0], local[1], step.z_sign * local[2]],
                );
                !matched[*index]
                    && step.child == child
                    && distance(centre, step_centre) < MTC_CENTRE_TOLERANCE_MM
                    && axis[2].abs().min(1.0).acos() < MTC_AXIS_TOLERANCE_RAD
            })
            .map(|(index, _)| index)
            .collect();
        let [index] = candidates.as_slice() else {
            panic!("{child} at {centre:?} matches STEP uses {candidates:?}");
        };
        matched[*index] = true;
    }
    assert!(matched.iter().all(|matched| *matched));
}
