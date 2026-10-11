use super::*;

#[test]
fn class_id_native_writer_streams_the_existing_hex_bytes() {
    #[derive(serde::Serialize)]
    struct Record<'a> {
        id: &'static str,
        value: &'a ClassId,
    }
    let class_id = ClassId([0xab; 16]);
    let owned = class_id
        .into_text(
            &cadmpeg_test_support::service_decode_context(),
            "retain Inventor class id native writer test text",
        )
        .expect("service class id text");
    assert_eq!(
        serde_json::to_vec(&class_id).expect("borrowed class id"),
        serde_json::to_vec(&owned).expect("owned class id")
    );
    let record = Record {
        id: "inventor:pmdc:class-id#1",
        value: &class_id,
    };
    cadmpeg_test_support::native_serialization::assert_native_limit(
        &record,
        serde_json::json!({"id": record.id, "value": owned}),
    );
}

#[test]
fn feature_label_native_writer_refuses_retained_limit_before_clone() {
    #[derive(serde::Serialize)]
    struct Record<'a> {
        id: &'static str,
        value: &'a PmDcFeatureLabelPayload,
    }
    let reference =
        crate::pmdc::PmDcReference::new(1, false).expect("test reference index fits 31 bits");
    let ctx = cadmpeg_test_support::service_decode_context();
    let label = PmDcFeatureLabelPayloadWire::<String> {
        save_version_major: 16,
        header: PmDcLinkedHeader {
            header_value: 0,
            header_id: 18,
            values: [0, 0],
            owner: reference,
            parent: reference,
            next: reference,
        },
        index: 3,
        participants: crate::pmdc::PmDcReferenceList::new(
            8,
            Some(crate::pmdc::PmDcListMetadata::U16([1, 2])),
            vec![reference],
        )
        .expect("paired participants"),
        name: "Extrude1".to_owned(),
        class_id: "ab".repeat(16),
    }
    .into_record(&ctx)
    .expect("feature label");
    let owned = label
        .clone()
        .into_wire(&ctx)
        .expect("contextful owned feature-label wire");
    assert_eq!(
        serde_json::to_vec(&label).expect("borrowed feature label"),
        serde_json::to_vec(&owned).expect("owned feature label")
    );
    let record = Record {
        id: "inventor:pmdc:feature-label#1",
        value: &label,
    };
    crate::pmdc::PMDC_LIST_CLONE_COUNT.with(|count| count.set(0));
    cadmpeg_test_support::native_serialization::assert_native_limit(
        &record,
        serde_json::json!({"id": record.id, "value": owned}),
    );
    crate::pmdc::PMDC_LIST_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
}
