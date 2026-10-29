// SPDX-License-Identifier: Apache-2.0
//! A Binary Parameter value that fails alone gives up only its value part
//! (#222; `iges.binary_parameter_value`; #215 rule 1).

use cadmpeg_ir::codec::DecodeResult;

use crate::test_support::binary_parts::{
    arc_with_real_start_then_point, double, point_with_real_y_then_point,
};

/// A double-precision real whose decimal order exceeds the single-precision
/// magnitude that the fixture Global declares (field 8 is 38). Its rendered
/// literal `1.00000000000000000E100` is a numeric literal that exceeds a
/// readable declaration (iges.md, Parameter Data numeric rules).
const UNADMITTED_REAL: f64 = 1e100;

fn entity(result: &DecodeResult, index: usize) -> serde_json::Value {
    let native = result.ir().native.namespace("iges").unwrap();
    serde_json::to_value(&native.arenas()["entities"][index]).unwrap()
}

/// The file offset of the one value group that encodes `value`.
fn group_offset(bytes: &[u8], value: f64) -> usize {
    let group = double(value);
    let hits: Vec<_> = bytes
        .windows(group.len())
        .enumerate()
        .filter(|(_, window)| *window == group.as_slice())
        .map(|(offset, _)| offset)
        .collect();
    assert_eq!(hits.len(), 1);
    hits[0]
}

/// The first entity keeps every Parameter slot but `field`, which is `null`.
/// One `iges.binary_parameter_value` of the 9-byte group at `offset` is the
/// only given-up part, inside its decoded `iges.binary_parameter_record`,
/// with one Error `record.unreadable`. The independent point is unchanged.
fn assert_only_value_given_up(
    baseline: &DecodeResult,
    decoded: &DecodeResult,
    field: usize,
    offset: usize,
) {
    let mut first = entity(decoded, 0);
    let mut expected = entity(baseline, 0);
    assert!(first["parameters"][field].is_null());
    assert!(!expected["parameters"][field].is_null());
    first["parameters"][field] = serde_json::Value::Null;
    expected["parameters"][field] = serde_json::Value::Null;
    // A Binary record states no comment. The normalized card pads the rest
    // of its 64 columns after the record delimiter, and the two rendered
    // literals differ in length, so the padding differs.
    for entity in [&mut first, &mut expected] {
        let padding = entity["comment"].as_str().unwrap();
        assert!(padding.len() % 2 == 0 && padding.as_bytes().chunks(2).all(|pair| pair == b"20"));
        entity["comment"] = serde_json::Value::Null;
    }
    assert_eq!(first, expected);
    assert_eq!(entity(decoded, 1), entity(baseline, 1));

    let native = decoded.ir().native.namespace("iges").unwrap();
    assert!(native.arenas()["quarantined_parameter_records"].is_empty());
    let fidelity = decoded.source_fidelity();
    let given_up = crate::test_support::given_up_parts(fidelity);
    assert_eq!(given_up.len(), 1);
    let parts = crate::test_support::given_up_parts(fidelity);
    assert_eq!(parts.len(), 1);
    let part = parts[0];
    assert!(!part.untyped().is_empty());
    assert_eq!(
        crate::test_support::given_up_count(decoded.report(), "iges.binary_parameter_value"),
        1
    );
    assert_eq!(part.offset(), u64::try_from(offset).unwrap());
    assert_eq!(part.byte_len(), 9);

    let unreadable: Vec<_> = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect();
    assert_eq!(unreadable.len(), 1);
    assert_eq!(unreadable[0].severity, cadmpeg_ir::report::Severity::Error);
    assert!(unreadable[0]
        .message
        .contains("iges.binary_parameter_value"));

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
}

#[test]
fn an_unadmitted_binary_point_coordinate_gives_up_only_its_value_part() {
    let baseline = crate::test_support::decode(point_with_real_y_then_point(2.0));
    let bytes = point_with_real_y_then_point(UNADMITTED_REAL);
    let decoded = crate::test_support::decode(bytes.clone());
    // Y is the second coordinate: token 2 after the entity type.
    assert_only_value_given_up(
        &baseline,
        &decoded,
        2,
        group_offset(&bytes, UNADMITTED_REAL),
    );
    // The point needs its Y coordinate, so it is not projected; the peer
    // stays.
    assert_eq!(decoded.ir().model.points.len(), 1);
    assert_eq!(baseline.ir().model.points.len(), 2);
}

#[test]
fn an_unadmitted_binary_arc_value_gives_up_only_its_value_part_and_no_record_is_quarantined() {
    let baseline = crate::test_support::decode(arc_with_real_start_then_point(1.0));
    let bytes = arc_with_real_start_then_point(UNADMITTED_REAL);
    let decoded = crate::test_support::decode(bytes.clone());
    // The arc start X is the fourth value: token 4 after the entity type.
    // A Type 100 record is not on the semantic handoff route; it is lexed in
    // its own part as well.
    assert_only_value_given_up(
        &baseline,
        &decoded,
        4,
        group_offset(&bytes, UNADMITTED_REAL),
    );
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| loss.code.local_code() != "parameter.data-quarantined"));
}
