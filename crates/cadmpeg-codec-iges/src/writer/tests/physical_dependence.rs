// SPDX-License-Identifier: Apache-2.0
//! Every physically dependent entity the writer emits has a parent.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::CadIr;

use crate::test_support::test_drawing_and_trimming::test_mixed_sense::{
    composite_edge_outer_loop_model, edge_direction_parameter_curve_boundary_model,
    edge_direction_parameter_curve_brep_model, mixed_sense_outer_loop_model,
    reversed_closed_outer_edge_model,
};
use crate::test_support::test_drawing_and_trimming::{
    reversed_face_use_multi_pcurve_loop_file, trimmed_plane_with_inner_loop_file,
};
use crate::{IgesCodec, IgesVersion};

/// One written entity: its type, its Directory subordinate status, its
/// Directory transformation pointer, and its Parameter Data tokens.
struct WrittenEntity {
    entity_type: u32,
    subordinate: [u8; 2],
    transform: u32,
    tokens: Vec<String>,
}

/// The fixed-ASCII field of `card` over the column range `columns`.
fn field(card: &[u8], columns: std::ops::Range<usize>) -> &str {
    std::str::from_utf8(&card[columns])
        .expect("ASCII field")
        .trim()
}

/// The entities of a written fixed-ASCII file, keyed by Directory sequence.
fn written_entities(bytes: &[u8]) -> BTreeMap<u32, WrittenEntity> {
    let cards = bytes
        .split(|byte| *byte == b'\n')
        .filter(|card| card.len() >= 80)
        .collect::<Vec<_>>();
    let mut parameters = BTreeMap::<u32, String>::new();
    for card in cards.iter().filter(|card| card[72] == b'P') {
        let owner = field(card, 64..72)
            .parse::<u32>()
            .expect("Directory pointer");
        parameters
            .entry(owner)
            .or_default()
            .push_str(std::str::from_utf8(&card[..64]).expect("ASCII data"));
    }
    let directory = cards
        .iter()
        .filter(|card| card[72] == b'D')
        .collect::<Vec<_>>();
    let mut entities = BTreeMap::new();
    for pair in directory.chunks_exact(2) {
        let sequence = field(pair[0], 73..80)
            .parse::<u32>()
            .expect("Directory sequence");
        let text = parameters.remove(&sequence).expect("Parameter Data record");
        let body = text.split(';').next().expect("terminated record");
        entities.insert(
            sequence,
            WrittenEntity {
                entity_type: field(pair[0], 0..8).parse().expect("entity type"),
                subordinate: [pair[0][66], pair[0][67]],
                transform: field(pair[0], 48..56).parse().unwrap_or(0),
                tokens: body
                    .split(',')
                    .map(|token| token.trim().to_owned())
                    .collect(),
            },
        );
    }
    entities
}

/// The Directory pointers in the Parameter Data of `entity`.
fn pointers(entity: &WrittenEntity) -> Vec<u32> {
    let token = |index: usize| -> i64 {
        entity
            .tokens
            .get(index)
            .and_then(|token| token.parse().ok())
            .expect("integer field")
    };
    let mut indices = Vec::new();
    match entity.entity_type {
        102 => indices.extend(2..2 + usize::try_from(token(1)).expect("count")),
        141 => {
            indices.push(3);
            let mut index = 5;
            for _ in 0..token(4) {
                let pcurves = usize::try_from(token(index + 2)).expect("count");
                indices.push(index);
                indices.extend(index + 3..index + 3 + pcurves);
                index += 3 + pcurves;
            }
        }
        142 => indices.extend([2, 3, 4]),
        143 => {
            indices.push(2);
            indices.extend(4..4 + usize::try_from(token(3)).expect("count"));
        }
        144 => {
            indices.extend([1, 4]);
            indices.extend(5..5 + usize::try_from(token(3)).expect("count"));
        }
        190 => indices.extend(1..entity.tokens.len()),
        504 => {
            let count = usize::try_from(token(1)).expect("count");
            for edge in 0..count {
                let start = 2 + 5 * edge;
                indices.extend([start, start + 1, start + 3]);
            }
        }
        508 => {
            let mut index = 2;
            for _ in 0..token(1) {
                let pcurves = usize::try_from(token(index + 4)).expect("count");
                indices.push(index + 1);
                indices.extend((0..pcurves).map(|pcurve| index + 6 + 2 * pcurve));
                index += 5 + 2 * pcurves;
            }
        }
        510 => {
            indices.push(1);
            indices.extend(4..4 + usize::try_from(token(2)).expect("count"));
        }
        514 => {
            let count = usize::try_from(token(1)).expect("count");
            indices.extend((0..count).map(|face| 2 + 2 * face));
        }
        110 | 116 | 123 | 126 | 128 | 502 => {}
        _ => panic!("the written file has an entity type this check does not read"),
    }
    let mut pointers = indices
        .into_iter()
        .map(|index| u32::try_from(token(index)).expect("Directory pointer"))
        .collect::<Vec<_>>();
    if entity.transform != 0 {
        pointers.push(entity.transform);
    }
    pointers
}

fn decoded(bytes: Vec<u8>) -> CadIr {
    IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the fixture")
        .ir()
        .clone()
}

/// Write the valid neutral model `ir` as IGES 5.3 and require a parent for
/// every entity whose subordinate status is physically dependent.
fn assert_dependents_have_parents(ir: &CadIr) {
    let validation =
        cadmpeg_ir::validate_neutral(ir, Vec::new()).expect("validate the neutral model");
    assert!(validation.is_ok());
    let generated = super::super::synthesize(ir, IgesVersion::V5_3).expect("write the model");
    let entities = written_entities(&generated.bytes);
    let referenced = entities
        .values()
        .flat_map(pointers)
        .collect::<BTreeSet<_>>();
    let mut dependents = 0_usize;
    for (sequence, entity) in &entities {
        if matches!(&entity.subordinate, b"01" | b"03") {
            dependents += 1;
            assert!(
                referenced.contains(sequence),
                "a physically dependent entity has a parent"
            );
        }
    }
    assert!(dependents > 0);
}

#[test]
fn trimmed_inner_loop_writes_no_dependent_entity_without_a_parent() {
    assert_dependents_have_parents(&decoded(trimmed_plane_with_inner_loop_file()));
}

#[test]
fn reversed_face_use_writes_no_dependent_entity_without_a_parent() {
    assert_dependents_have_parents(&decoded(reversed_face_use_multi_pcurve_loop_file()));
}

/// The reversed closed edge and its parameter curve take one reversed copy
/// in either boundary direction, so one stored carrier has no use.
#[test]
fn reversed_closed_outer_edge_writes_no_dependent_entity_without_a_parent() {
    assert_dependents_have_parents(&reversed_closed_outer_edge_model());
}

/// The Type 142 boundary writes the reversed edge through a reversed copy of
/// its curve, so the stored edge curve has no use.
#[test]
fn mixed_sense_outer_loop_writes_no_dependent_entity_without_a_parent() {
    assert_dependents_have_parents(&mixed_sense_outer_loop_model());
}

/// A Type 102 composite cannot be a constituent of the Type 102 model curve
/// of a Type 142 boundary, so the boundary takes the constituents of each
/// composite edge curve.
#[test]
fn composite_edges_of_a_composed_boundary_write_no_dependent_entity_without_a_parent() {
    assert_dependents_have_parents(&composite_edge_outer_loop_model());
}

/// The Type 141 boundary writes the parameter curve in the traversal, through
/// a reversed copy, so the stored parameter curve has no use.
#[test]
fn edge_direction_parameter_curve_of_a_boundary_writes_no_dependent_entity_without_a_parent() {
    assert_dependents_have_parents(&edge_direction_parameter_curve_boundary_model());
}

/// The Type 508 loop writes the parameter curves in the traversal, through
/// reversed copies, so the stored parameter curves have no use.
#[test]
fn edge_direction_parameter_curves_of_a_brep_loop_write_no_dependent_entity_without_a_parent() {
    assert_dependents_have_parents(&edge_direction_parameter_curve_brep_model());
}
