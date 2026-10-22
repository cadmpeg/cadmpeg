// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::NativeUnknownRecord;

use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};

fn points(first: &str) -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "FIRST".into(),
            status: "00000000",
            parameters: first.into(),
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

#[test]
fn public_parameter_failure_keeps_one_shared_source_home_and_checked_peer_content() {
    let baseline = crate::test_support::decode(points("116,1,2,3,0;"));
    let bytes = points("110,1,2,3,0;");
    let decoded = crate::test_support::decode(bytes.clone());
    assert_eq!(decoded.ir().model.points.len(), 1);
    let peer = &decoded.ir().model.points[0];
    let original = baseline
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id == peer.id)
        .unwrap();
    assert_eq!(
        serde_json::to_value(peer).unwrap(),
        serde_json::to_value(original).unwrap()
    );
    let native = decoded.ir().native.namespace("iges").unwrap();
    assert!(native
        .arenas()
        .get("quarantined_parameter_records")
        .is_none_or(Vec::is_empty));
    let unknowns = native.arena_as::<NativeUnknownRecord>("unknowns").unwrap();
    assert_eq!(unknowns.len(), 1);
    let first = bytes
        .chunks_exact(81)
        .position(|card| card[72] == b'P')
        .unwrap()
        * 81;
    let fidelity = decoded.source_fidelity();
    assert_eq!(
        fidelity
            .retained_record(unknowns[0].id.as_str())
            .unwrap()
            .data(),
        Some(&bytes[first..first + 81])
    );
    assert!(!fidelity.record_layout()["iges:entity:directory#3"]
        .reads()
        .is_empty());
    assert!(!fidelity
        .record_layout()
        .contains_key("iges:entity:directory#1"));
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
        .all(|loss| loss.code != crate::loss::IgesLossCode::ParameterDataQuarantined.kind()));
}
