// SPDX-License-Identifier: Apache-2.0

use cadmpeg_test_support::native_serialization::assert_native_limit;

#[test]
fn native_store_output_keeps_canonical_records_retained() {
    use std::io::Cursor;

    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
    use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions};

    use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
    use crate::IgesCodec;

    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 123,
        form: 0,
        label: "DIR".into(),
        status: "00000000",
        parameters: "123,1,0,0;".into(),
    }]);
    let operation = "serialize native record";
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        operation,
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            IgesCodec
                .decode(
                    &mut Cursor::new(&bytes),
                    &DecodeOptions {
                        policy,
                        ..DecodeOptions::default()
                    },
                )
                .map(|_| ())
                .map_err(|failure| match failure {
                    DecodeFailure::Codec(error) => error,
                    other => panic!("unexpected decode failure: {other:?}"),
                })
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == operation
    ));

    let decoded = IgesCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        decoded.ir().native.namespace("iges").unwrap().arenas()["directions"].len(),
        1
    );
}

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
