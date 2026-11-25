// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::records::feature::assembly_features::DesignComponentOccurrence;

const STREAM: &str = "f3d:Design/BulkStream.dat";
const OWNER: u64 = 7;

/// The fixed untyped frame of the matrix or identity form with one zero
/// ordinal-prefix byte after the path terminator.
fn unmarked_frame(matrix: bool, ordinal: u32) -> Vec<u8> {
    let mut frame = common(if matrix { 357 } else { 229 }, ordinal);
    let owner_marker = if matrix { 346 } else { 218 };
    if matrix {
        for (i, value) in [
            1.0_f64, 0.0, 0.0, 2.0, 0.0, 1.0, 0.0, 3.0, 0.0, 0.0, 1.0, 4.0, 0.0, 0.0, 0.0, 1.0,
        ]
        .into_iter()
        .enumerate()
        {
            frame[209 + i * 8..217 + i * 8].copy_from_slice(&value.to_le_bytes());
        }
    } else {
        frame[208] = 1;
    }
    frame[owner_marker] = 1;
    frame[owner_marker + 1..owner_marker + 9].copy_from_slice(&OWNER.to_le_bytes());
    frame.insert(40, 0);
    frame
}

fn decode(bytes: &[u8]) -> Option<DesignComponentOccurrence> {
    crate::test_support::with_decode_context(|ctx| {
        super::super::attributed_component_occurrence_frame(ctx, bytes, 0, bytes.len(), STREAM)
    })
}

#[test]
fn a_path_terminator_in_place_of_the_attribute_run_reads_a_placed_occurrence() {
    let bytes = unmarked_frame(true, 2);
    assert_eq!(bytes.len(), 358);
    let occurrence = decode(&bytes).unwrap();
    assert!(occurrence.lacks_attribute_run());
    assert_eq!(occurrence.occurrence_ordinal(), 2);
    assert_eq!(occurrence.component_record_index, 10);
    assert_eq!(occurrence.component_guid.as_str(), COMPONENT);
    assert_eq!(occurrence.occurrence_guid.as_str(), OCCURRENCE);
    assert_eq!(occurrence.owner_entity, OWNER);
    let transform = occurrence.transform().unwrap();
    assert_eq!(transform.value.rows()[0][3], 2.0);
    assert_eq!(transform.value.rows()[1][3], 3.0);
    assert_eq!(transform.value.rows()[2][3], 4.0);
    assert_eq!(transform.offset, 210);
    let wire = serde_json::to_value(&occurrence).unwrap();
    assert_eq!(wire["attributed"]["ordinal_prefix"], 0);
    assert_eq!(
        wire["attributed"]["attributes"].as_array().unwrap().len(),
        0
    );
    let restored: DesignComponentOccurrence = serde_json::from_value(wire).unwrap();
    assert_eq!(restored, occurrence);
}

#[test]
fn the_identity_flag_reads_the_base_placement() {
    let bytes = unmarked_frame(false, 1);
    assert_eq!(bytes.len(), 230);
    let occurrence = decode(&bytes).unwrap();
    assert!(occurrence.lacks_attribute_run());
    assert_eq!(occurrence.occurrence_ordinal(), 1);
    assert!(occurrence.transform().is_none());
    assert_eq!(occurrence.owner_entity, OWNER);
}

#[test]
fn a_frame_that_breaks_the_unmarked_grammar_stays_native() {
    let good = unmarked_frame(true, 2);
    // A nonzero prefix byte is not the zero ordinal prefix.
    let mut bytes = good.clone();
    bytes[40] = 1;
    assert!(decode(&bytes).is_none());
    // The repeated component reference has to equal the first.
    let mut bytes = good.clone();
    bytes[199] ^= 1;
    assert!(decode(&bytes).is_none());
    // The extent has to close after the owner entity.
    let mut bytes = good;
    bytes.push(0);
    assert!(decode(&bytes).is_none());
}

#[test]
fn only_a_registered_record_is_admitted() {
    let bytes = unmarked_frame(true, 2);
    let occurrence = decode(&bytes).unwrap();
    let key = (u64::from(occurrence.record_index), occurrence.byte_offset());
    let registered = std::collections::BTreeSet::from([key]);
    let primary = registered.clone();
    let mut kept = vec![occurrence.clone()];
    assert_eq!(
        super::super::admit_occurrence_envelopes(&mut kept, &primary, &registered),
        0
    );
    assert_eq!(kept.len(), 1);
    let mut dropped = vec![occurrence];
    let none = std::collections::BTreeSet::new();
    assert_eq!(
        super::super::admit_occurrence_envelopes(&mut dropped, &primary, &none),
        1
    );
    assert!(dropped.is_empty());
}
