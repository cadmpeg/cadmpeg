// SPDX-License-Identifier: Apache-2.0

use crate::object_graph::{PayloadField, PayloadSubtype};
use crate::test_support::test_object_graph::{object_graph_record, object_graph_from_records};

#[test]
fn native_payload_bulk_classification_skips_unread_fields() {
    let mut native = crate::native::CatiaNative::decode(&object_graph_from_records(&[
        object_graph_record(&[0x04, 0x01, 0x81], &[0xfe]),
    ]));
    let record = &mut native.object_graphs[0].records[0];
    record.payload.fields = vec![PayloadField::BulkTable {
        count: 0,
        rows: Vec::new(),
        offset: 0,
    }];
    record.payload.fields.extend((0..4096).map(|_| PayloadField::Terminator));
    crate::test_support::with_work_limit(1, |ctx| {
        assert_eq!(record.subtype_charged(ctx).expect("only the bulk field is visited"),
            PayloadSubtype::BulkTable);
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn native_empty_payload_classification_preserves_sticky_refusal() {
    let mut native = crate::native::CatiaNative::decode(&object_graph_from_records(&[
        object_graph_record(&[0x04, 0x01, 0x81], &[0xfe]),
    ]));
    let record = &mut native.object_graphs[0].records[0];
    record.payload.fields.clear();
    crate::test_support::with_work_limit(0, |ctx| {
        assert_eq!(record.subtype_charged(ctx).expect("empty payload requires no work"),
            PayloadSubtype::Empty);
        record.payload.fields.push(PayloadField::Terminator);
        let error = record.subtype_charged(ctx).expect_err("first field requires work");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("resource refusal required")
        };
        assert_eq!(limit.operation, "catia_object_payload_classification");
        record.payload.fields.clear();
        assert!(matches!(record.subtype_charged(ctx), Err(cadmpeg_core::CodecError::ResourceLimit(ref sticky)) if sticky == &limit));
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}
