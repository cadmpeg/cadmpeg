// SPDX-License-Identifier: Apache-2.0
//! A Parameter Data literal that fails alone gives up only its field (#222).

use cadmpeg_ir::codec::DecodeResult;

use crate::test_support::test_owned::{owned_test_file_with_raw_parameters, OwnedTestEntity};

/// A Line (D1) with the given parameters and an independent Point (D3).
fn line_and_point(line: String) -> Vec<u8> {
    owned_test_file_with_raw_parameters(&[
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "LINE".into(),
            status: "00000000",
            parameters: line,
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "PEER".into(),
            status: "00000000",
            parameters: "116,4,5,6,0;".into(),
        },
    ])
}

fn entity(result: &DecodeResult, index: usize) -> serde_json::Value {
    let native = result.ir().native.namespace("iges").unwrap();
    serde_json::to_value(&native.arenas()["entities"][index]).unwrap()
}

/// The Line keeps every field but `failed`, which is absent; one
/// `iges.parameter_field` part at `offset` with `byte_len` bytes is the only
/// given-up part, with one `record.unreadable`; the peer is unchanged.
fn assert_only_field_given_up(
    baseline: &DecodeResult,
    decoded: &DecodeResult,
    failed: usize,
    offset: u64,
    byte_len: u64,
) {
    let mut line = entity(decoded, 0);
    let mut expected = entity(baseline, 0);
    assert!(line["parameters"][failed].is_null());
    assert!(!expected["parameters"][failed].is_null());
    assert_eq!(
        line["parameters"].as_array().unwrap().len(),
        expected["parameters"].as_array().unwrap().len()
    );
    line["parameters"][failed] = serde_json::Value::Null;
    expected["parameters"][failed] = serde_json::Value::Null;
    assert_eq!(line["parameters"], expected["parameters"]);
    assert_eq!(entity(decoded, 1), entity(baseline, 1));
    let peer = crate::ids::point(&crate::ids::Stem::directory(3_u32));
    let peer_point = |result: &DecodeResult| {
        result
            .ir()
            .model
            .points
            .iter()
            .find(|point| point.id == peer)
            .map(|point| serde_json::to_value(point).unwrap())
    };
    assert!(peer_point(baseline).is_some());
    assert_eq!(peer_point(decoded), peer_point(baseline));

    let fidelity = decoded.source_fidelity();
    let parts = crate::test_support::given_up_parts(fidelity);
    assert_eq!(parts.len(), 1);
    let part = parts[0];
    assert!(!part.untyped().is_empty());
    assert_eq!(part.offset(), offset);
    assert_eq!(part.byte_len(), byte_len);
    let unreadable: Vec<_> = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect();
    assert_eq!(unreadable.len(), 1);
    assert!(unreadable[0].message.contains("iges.parameter_field"));
    // The Line needs the absent coordinate, so its curve is not projected
    // and charges `geometry.not-projected` (iges.md, Parameter Data numeric
    // rules). No other loss is new.
    assert!(decoded.ir().model.curves.is_empty());
    assert!(!baseline.ir().model.curves.is_empty());
    let mut codes: Vec<_> = decoded
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.local_code().to_owned())
        .collect();
    codes.sort_unstable();
    assert!(baseline.report().losses.is_empty());
    assert_eq!(codes, ["geometry.not-projected", "record.unreadable"]);
}

#[test]
fn a_nonnumeric_line_coordinate_gives_up_only_its_parameter_field() {
    let baseline = crate::test_support::decode(line_and_point("110,0,0,0,9,0,0;".into()));
    let decoded = crate::test_support::decode(line_and_point("110,0,0,0,X,0,0;".into()));
    // Field 4 is `X,` at logical offset 10.
    assert_only_field_given_up(&baseline, &decoded, 4, 10, 2);
}

#[test]
fn a_line_coordinate_that_crosses_a_card_gives_up_only_its_parameter_field() {
    let mut x = "0".repeat(59);
    x.push('1');
    let baseline = crate::test_support::decode(line_and_point("110,1,0,0,9,0,0;".into()));
    let decoded = crate::test_support::decode(line_and_point(format!("110,{x},0,0,9,0,0;")));
    // Field 1 starts at logical offset 4; its delimiter is the first byte of
    // the second card.
    assert_only_field_given_up(&baseline, &decoded, 1, 4, 61);
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "record.unreadable"
            && loss
                .message
                .contains("numeric field or its delimiter crosses a card boundary")));
}

#[test]
fn a_nonnumeric_compressed_coordinate_gives_up_only_its_parameter_field() {
    let original = crate::test_support::compressed_parts::points(false, false);
    let baseline = crate::test_support::decode(original.clone());
    let record = original
        .windows(b"116,1.0,2.0,3.0;".len())
        .position(|bytes| bytes == b"116,1.0,2.0,3.0;")
        .expect("first Parameter source");
    let field = record + b"116,1.0,".len();
    let mut bytes = original;
    bytes[field..field + 3].copy_from_slice(b"2.X");
    let decoded = crate::test_support::decode(bytes.clone());

    // The record keeps its other fields; field 2 is absent.
    let mut first = entity(&decoded, 0);
    let mut expected = entity(&baseline, 0);
    assert!(first["parameters"][2].is_null());
    assert!(!expected["parameters"][2].is_null());
    first["parameters"][2] = serde_json::Value::Null;
    expected["parameters"][2] = serde_json::Value::Null;
    assert_eq!(first["parameters"], expected["parameters"]);
    assert_eq!(entity(&decoded, 1), entity(&baseline, 1));

    let fidelity = decoded.source_fidelity();
    let parts = crate::test_support::given_up_parts(fidelity);
    assert_eq!(parts.len(), 1);
    let part = parts[0];
    assert!(!part.untyped().is_empty());
    assert_eq!(
        crate::test_support::given_up_count(decoded.report(), "iges.compressed_parameter_field"),
        1
    );
    assert_eq!(part.offset(), u64::try_from(field).unwrap());
    assert_eq!(part.byte_len(), 4);
    assert_eq!(part.data(), Some(b"2.X,".as_slice()));
    // The point needs the absent coordinate, so it is not projected; the
    // peer point stays. Only the given-up field and the projection of its
    // own record charge a loss.
    let peer = crate::ids::point(&crate::ids::Stem::directory(3_u32));
    assert_eq!(
        decoded
            .ir()
            .model
            .points
            .iter()
            .map(|point| point.id.clone())
            .collect::<Vec<_>>(),
        [peer]
    );
    assert!(baseline.report().losses.is_empty());
    let losses = &decoded.report().losses;
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .count(),
        1
    );
    assert!(losses
        .iter()
        .all(|loss| loss.code.local_code() == "record.unreadable"
            || (loss.code.local_code() == "geometry.not-projected"
                && loss.message.contains("D1"))));
}
