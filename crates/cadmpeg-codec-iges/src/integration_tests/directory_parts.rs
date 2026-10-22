// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::NativeUnknownRecord;

use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};

fn points() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "FIRST".into(),
            status: "00000000",
            parameters: "116,1,2,3,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "SECOND".into(),
            status: "00000000",
            parameters: "116,4,5,6,0;".into(),
        },
    ])
}

fn second_entry(bytes: &[u8]) -> usize {
    bytes
        .chunks_exact(81)
        .position(|card| card[72] == b'D')
        .unwrap()
        * 81
        + 162
}

#[test]
fn public_fixed_ascii_decode_composes_shared_failure_and_keeps_real_point_content() {
    let original = points();
    let baseline = crate::test_support::decode(original.clone());
    let mut damaged = original;
    let start = second_entry(&damaged);
    damaged[start + 48..start + 56].copy_from_slice(b"     abc");
    let decoded = crate::test_support::decode(damaged.clone());
    assert_eq!(decoded.ir().model.points.len(), 1);
    assert_eq!(
        serde_json::to_value(&decoded.ir().model.points[0]).unwrap(),
        serde_json::to_value(&baseline.ir().model.points[0]).unwrap()
    );
    let native = decoded.ir().native.namespace("iges").unwrap();
    assert!(native
        .arenas()
        .get("quarantined_directory_records")
        .is_none_or(Vec::is_empty));
    let unknowns = native.arena_as::<NativeUnknownRecord>("unknowns").unwrap();
    assert_eq!(unknowns.len(), 1);
    assert_eq!(
        decoded
            .source_fidelity()
            .retained_record(unknowns[0].id.as_str())
            .unwrap()
            .data(),
        Some(&damaged[start..start + 162])
    );
    assert_eq!(
        decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .count(),
        1
    );
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != crate::loss::IgesLossCode::DirectoryRecordQuarantined.kind()));
}
