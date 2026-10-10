// SPDX-License-Identifier: Apache-2.0

use cadmpeg_test_support::native_serialization::assert_native_limit;

#[test]
fn physical_card_id_streams_once_with_native_retained_limit() {
    let bytes = crate::test_support::test_cards::fixed_ascii_with_global_cards(&[b",,;"]);
    let scan = crate::test_support::scan(&bytes).expect("valid card");
    let line = &scan.lines[0];
    let record = super::super::NativeCard { index: 0, line };
    assert_native_limit(
        &record,
        serde_json::json!({
            "id": "iges:physical:card#1", "offset": 0,
            "payload": cadmpeg_ir::hash::LowerHex(&line.physical().payload),
            "line_ending": "0a", "section": "start", "sequence": 1,
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

#[test]
fn optional_cards_spend_collection_slots_per_record_and_keep_exact_bytes() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    let original = crate::test_support::test_curves_and_surfaces::point_file_with_global(b",,;");
    let mut bytes = Vec::new();
    for sequence in 1..=500 {
        bytes.extend(crate::test_support::test_cards::card(
            b"Optional source text",
            b'S',
            sequence,
        ));
    }
    bytes.extend_from_slice(&original[81..]);
    // The source census includes the added Start records.
    let terminate = bytes.len() - 81;
    bytes[terminate..terminate + 8].copy_from_slice(b"S0000500");
    let mut options = DecodeOptions::default();
    options.policy.limits.max_collection_items = 20_000;
    let result = crate::IgesCodec
        .decode(&mut std::io::Cursor::new(&bytes), &options)
        .unwrap();
    assert_eq!(result.ir().model.points.len(), 1);
    let first = &result.ir().native.namespace("iges").unwrap().arenas()["cards"][0];
    let payload = first.field("payload").unwrap();
    let hex = payload.as_str().unwrap();
    assert_eq!(hex.len(), 160);
    let retained = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(retained, bytes[..80]);
    assert_eq!(first.field("line_ending").unwrap(), "0a");
}
