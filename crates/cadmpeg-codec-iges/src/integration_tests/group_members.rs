// SPDX-License-Identifier: Apache-2.0
//! A Group whose member Directory Entry is given up (#222 Group count ruling).

use cadmpeg_ir::codec::DecodeResult;
use cadmpeg_ir::source_fidelity::parts::PartState;

use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};

/// Points A (D1), B (D3) and C (D5), and an unordered Group (D7) of them.
fn group_of_three_points() -> Vec<u8> {
    let point = |label: &str, parameters: &str| OwnedTestEntity {
        entity_type: 116,
        form: 0,
        label: label.into(),
        status: "00000000",
        parameters: parameters.into(),
    };
    owned_test_file(&[
        point("A", "116,1,2,3,0;"),
        point("B", "116,4,5,6,0;"),
        point("C", "116,7,8,9,0;"),
        OwnedTestEntity {
            entity_type: 402,
            form: 7,
            label: "GROUP".into(),
            status: "00000000",
            parameters: "402,3,1,3,5;".into(),
        },
    ])
}

/// The first card of the Directory Entry with the given ordinal.
fn directory_entry(bytes: &[u8], ordinal: usize) -> usize {
    bytes
        .chunks_exact(81)
        .position(|card| card[72] == b'D')
        .unwrap()
        * 81
        + ordinal * 162
}

fn group(decoded: &DecodeResult) -> serde_json::Value {
    let native = decoded.ir().native.namespace("iges").unwrap();
    let groups = &native.arenas()["groups"];
    assert_eq!(groups.len(), 1);
    serde_json::to_value(&groups[0]).unwrap()
}

/// The members of one damaged-Group decode, by Directory ordinal.
struct Members<'a> {
    group: u32,
    given_up: u32,
    peers: &'a [u32],
    points: &'a [u32],
}

/// #215 damaged-part test. The peer entities and the neutral points equal the
/// undamaged decode. The Group entity keeps its read parameters; its edge to
/// the given-up entry is unresolved (#222) and that entry leaves its resolved
/// links.
fn assert_peers_unchanged(baseline: &DecodeResult, decoded: &DecodeResult, members: &Members<'_>) {
    let entity = |ordinal: u32| format!("iges:entity:directory#{ordinal}");
    let row = |result: &DecodeResult, ordinal: u32| {
        let native = result.ir().native.namespace("iges").unwrap();
        native.arenas()["entities"]
            .iter()
            .find(|record| record.id() == entity(ordinal))
            .map(|record| serde_json::to_value(record).unwrap())
    };
    for ordinal in members.peers {
        assert!(row(baseline, *ordinal).is_some());
        assert_eq!(row(decoded, *ordinal), row(baseline, *ordinal));
    }
    assert!(row(decoded, members.given_up).is_none());
    let given_up = serde_json::json!(entity(members.given_up));
    let mut expected = row(baseline, members.group).unwrap();
    expected["links"]
        .as_array_mut()
        .unwrap()
        .retain(|link| *link != given_up);
    for edge in expected["references"].as_array_mut().unwrap() {
        if edge["target"] == given_up {
            edge["resolution"] = serde_json::json!("dangling");
            edge["target"] = serde_json::Value::Null;
        }
    }
    assert_eq!(row(decoded, members.group), Some(expected));
    let point_rows = |result: &DecodeResult| {
        result
            .ir()
            .model
            .points
            .iter()
            .filter(|record| {
                members.points.iter().any(|ordinal| {
                    record.id == crate::ids::point(&crate::ids::Stem::directory(*ordinal))
                })
            })
            .map(|record| serde_json::to_value(record).unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(point_rows(baseline).len(), members.points.len());
    assert_eq!(point_rows(decoded), point_rows(baseline));
}

fn loss_codes(decoded: &DecodeResult) -> Vec<String> {
    let mut codes: Vec<_> = decoded
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.to_string())
        .collect();
    codes.sort_unstable();
    codes
}

#[test]
fn a_group_keeps_its_count_and_every_slot_when_a_member_directory_entry_is_given_up() {
    let original = group_of_three_points();
    let baseline = crate::test_support::decode(original.clone());
    let mut damaged = original;
    // B's required transformation field (card 1 columns 49-56) is not an
    // integer, so its `iges.directory_field` part is given up and B's record
    // is not formed (#215 rule 1).
    let start = directory_entry(&damaged, 1);
    damaged[start + 48..start + 56].copy_from_slice(b"     abc");
    let decoded = crate::test_support::decode(damaged);

    let members = serde_json::json!([
        "iges:entity:directory#1",
        "iges:entity:directory#3",
        "iges:entity:directory#5"
    ]);
    // The undamaged decode keeps the Group as read.
    let expected = group(&baseline);
    assert_eq!(expected["declared_member_count"], 3);
    assert_eq!(expected["members"], members);
    // Ruling rule 1: the native Group keeps the source count and one slot per
    // read pointer, in source order, whether or not the target survives.
    assert_eq!(group(&decoded), expected);

    // Ruling rule 2: the neutral membership holds A and C only. B is absent,
    // and the loss codes below show that the Group charges no loss of its own.
    let point = |ordinal: u32| crate::ids::point(&crate::ids::Stem::directory(ordinal));
    assert_eq!(
        decoded
            .ir()
            .model
            .points
            .iter()
            .map(|record| record.id.clone())
            .collect::<Vec<_>>(),
        [point(1), point(5)]
    );

    // Exactly one `record.unreadable` names B's given-up part. The Group slot
    // that names B is an unresolved pointer, like one that names an absent
    // entry (#222).
    let unreadable: Vec<_> = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect();
    assert_eq!(unreadable.len(), 1);
    let given_up: Vec<_> = decoded
        .source_fidelity()
        .parts()
        .values()
        .filter(|part| part.state() == &PartState::GivenUp {})
        .collect();
    assert_eq!(given_up.len(), 1);
    assert_eq!(given_up[0].level().as_str(), "iges.directory_field");
    assert_eq!(given_up[0].offset(), u64::try_from(start + 48).unwrap());
    assert_eq!(
        unreadable[0].provenance.as_ref().unwrap().offset,
        given_up[0].offset()
    );
    let mut expected_codes = loss_codes(&baseline);
    expected_codes.extend([
        cadmpeg_ir::codec::DecodeLossCode::RecordUnreadable
            .note(String::new())
            .code
            .to_string(),
        crate::loss::IgesLossCode::PointerUnresolved
            .note(String::new())
            .code
            .to_string(),
    ]);
    expected_codes.sort_unstable();
    assert_eq!(loss_codes(&decoded), expected_codes);

    assert_peers_unchanged(
        &baseline,
        &decoded,
        &Members {
            group: 7,
            given_up: 3,
            peers: &[1, 5],
            points: &[1, 5],
        },
    );
}

#[test]
fn a_binary_group_keeps_its_count_and_every_slot_when_a_member_directory_record_is_given_up() {
    let original = crate::test_support::binary_parts::point_group_of_three(false);
    let bytes = crate::test_support::binary_parts::point_group_of_three(true);
    assert_eq!(original.len(), bytes.len());
    assert_eq!(
        original.iter().zip(&bytes).filter(|(a, b)| a != b).count(),
        1
    );
    let baseline = crate::test_support::decode(original);
    let decoded = crate::test_support::decode(bytes);
    let members = serde_json::json!([
        "iges:entity:directory#1",
        "iges:entity:directory#3",
        "iges:entity:directory#7"
    ]);
    let expected = group(&baseline);
    assert_eq!(expected["declared_member_count"], 3);
    assert_eq!(expected["members"], members);
    // Ruling rule 1: no pointer is rewritten or removed.
    assert_eq!(group(&decoded), expected);
    let point = |ordinal: u32| crate::ids::point(&crate::ids::Stem::directory(ordinal));
    assert_eq!(
        baseline
            .ir()
            .model
            .points
            .iter()
            .map(|record| record.id.clone())
            .collect::<Vec<_>>(),
        [point(1), point(7)]
    );
    // Ruling rule 2: the null entity is absent from the neutral membership,
    // and the points stay.
    assert_eq!(
        decoded
            .ir()
            .model
            .points
            .iter()
            .map(|record| record.id.clone())
            .collect::<Vec<_>>(),
        [point(1), point(7)]
    );
    let given_up: Vec<_> = decoded
        .source_fidelity()
        .parts()
        .values()
        .filter(|part| part.state() == &PartState::GivenUp {})
        .collect();
    assert_eq!(given_up.len(), 1);
    assert_eq!(given_up[0].level().as_str(), "iges.binary_directory_record");
    assert_eq!(
        decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .count(),
        1
    );
    let mut expected_codes = loss_codes(&baseline);
    expected_codes.extend([
        cadmpeg_ir::codec::DecodeLossCode::RecordUnreadable
            .note(String::new())
            .code
            .to_string(),
        crate::loss::IgesLossCode::PointerUnresolved
            .note(String::new())
            .code
            .to_string(),
    ]);
    expected_codes.sort_unstable();
    assert_eq!(loss_codes(&decoded), expected_codes);
    assert_peers_unchanged(
        &baseline,
        &decoded,
        &Members {
            group: 5,
            given_up: 3,
            peers: &[1, 7],
            points: &[1, 7],
        },
    );
}
