use super::{records, validate};
use crate::wire::records::consolidated_records;

#[test]
fn counted_lists_preserve_child_ordinals_and_reference_forms() {
    let bytes = [0xb2, 0x03, 0x60, 5, 0x49, 0x84, 0x03, 0x07, 0x0b, 0x0f];
    let native = crate::test_support::with_service_context(|ctx| {
        records(ctx, &bytes, &consolidated_records(&bytes))
    })
    .expect("complete counted list");
    let [record] = native.as_slice() else {
        panic!("one record")
    };
    assert_eq!(
        record
            .references
            .iter()
            .map(|r| r.value)
            .collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
    assert!(record
        .references
        .iter()
        .all(|r| r.encoding == crate::native::CatiaAllocationReferenceEncoding::OwnedChild));
    validate(&native).expect("valid native list");
    let mut changed = native;
    changed[0].references[0].value = 1;
    assert!(validate(&changed).is_err());
    let wire = serde_json::to_value(&changed[0]).expect("changed reference wire");
    assert!(serde_json::from_value::<super::CatiaConsolidatedClass60Record>(wire).is_err());
}

#[test]
fn counted_lists_abstain_on_nonclosure_and_charge_references() {
    for payload in [
        &[0x81, 0x08][..],
        &[0x81, 0x05, 0x03],
        &[0x01, 0x05],
        &[0x84, 0x03],
    ] {
        let mut bytes = vec![
            0xb2,
            0x03,
            0x60,
            u8::try_from(payload.len()).expect("small payload"),
            5,
        ];
        bytes.extend_from_slice(payload);
        assert!(crate::test_support::with_service_context(|ctx| records(
            ctx,
            &bytes,
            &consolidated_records(&bytes)
        ))
        .expect("bounded admission")
        .is_empty());
    }
    let bytes = [0xb2, 0x03, 0x60, 2, 5, 0x81, 0x09];
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        records(ctx, &bytes, &consolidated_records(&bytes))
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error)) if error.operation == "catia_class60_references")
    );
}

#[test]
fn counted_lists_preserve_logical_order_across_reordered_extents() {
    let bytes = [
        0xb2, 0x03, 0x60, 2, 5, 0x81, 0x09, 0xb2, 0x03, 0x60, 2, 5, 0x81, 0x0d,
    ];
    let native = crate::test_support::with_service_context(|ctx| {
        let sources = [vec![
            crate::wire::records::SourceExtent::within(&bytes, 7, 14).expect("first extent"),
            crate::wire::records::SourceExtent::within(&bytes, 0, 7).expect("second extent"),
        ]];
        crate::native::CatiaNative::decode_with_record_sources(
            ctx,
            &bytes,
            &sources,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("bounded native decode");
    assert_eq!(
        native
            .consolidated_class60_records
            .iter()
            .map(|record| record.frame.pos)
            .collect::<Vec<_>>(),
        [7, 0]
    );
    assert_eq!(
        native
            .consolidated_class60_records
            .iter()
            .map(|record| (record.source_index, record.source_offset))
            .collect::<Vec<_>>(),
        [(0, 0), (0, 7)]
    );
    let mut changed = native.consolidated_class60_records.clone();
    changed[1].source_offset = 0;
    assert!(validate(&changed).is_err());
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store logical source order");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load logical source order"),
        native
    );
}
