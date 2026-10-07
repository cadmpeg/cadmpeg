// SPDX-License-Identifier: Apache-2.0

use cadmpeg_test_support::native_serialization::assert_native_limit;

#[test]
fn physical_card_id_streams_once_with_native_retained_limit() {
    let bytes = crate::test_support::test_cards::fixed_ascii_with_global_cards(&[b",,;"]);
    let scan = crate::test_support::scan(&bytes).expect("valid card");
    let card = &scan.cards()[0];
    let record = super::super::NativeCard {
        index: 0,
        line: &card.line,
        card: Some((card.section, card.sequence)),
    };
    assert_native_limit(
        &record,
        serde_json::json!({
            "id": "iges:physical:card#1", "offset": 0,
            "payload": card.line.payload,
            "line_ending": [10], "section": "start", "sequence": 1,
        }),
    );
}

#[test]
fn quarantined_record_id_streams_once_with_native_retained_limit() {
    #[derive(serde::Serialize)]
    struct Record {
        id: super::super::QuarantinedId,
    }

    let record = Record {
        id: super::super::QuarantinedId {
            section: "directory",
            sequence: 3,
        },
    };
    assert_native_limit(
        &record,
        serde_json::json!({"id": "iges:quarantine:directory#3"}),
    );
}
