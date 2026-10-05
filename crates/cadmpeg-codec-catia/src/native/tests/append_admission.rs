// SPDX-License-Identifier: Apache-2.0
use crate::native::CatiaNative;
use crate::test_support::test_object_graph::{
    entity_table_record_with_value, object_graph_from_records, object_graph_record,
};
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;

#[test]
fn native_entity_record_append_preserves_pairing_and_refuses_move_work() {
    let records = [
        object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x82, 0x81], &[0xfe]),
    ];
    let mut bytes = entity_table_record_with_value(1, &[0xfe]);
    bytes.extend(entity_table_record_with_value(2, &[0xfe]));
    bytes.push(0xde);
    bytes.extend(object_graph_from_records(&records));
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        let result = CatiaNative::decode_with_record_sources(
            ctx,
            &bytes,
            &[],
            &mut crate::nurbs::LaneRefusals::new(),
        );
        if let Err(CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
        }
        result
    };
    let native = crate::test_support::with_service_context(decode)
        .expect("two paired native entities admitted");
    assert_eq!(native.object_graphs.len(), 1);
    let graph = &native.object_graphs[0];
    assert_eq!(graph.records.len(), 2);
    assert_eq!(graph.records[0].entity_id(), Some(1));
    assert_eq!(graph.records[1].entity_id(), Some(2));
    assert_eq!(native.entity_records.len(), 2);
    for (entity, object) in native.entity_records.iter().zip(&graph.records) {
        assert_eq!(entity.object_graph, graph.id);
        assert_eq!(entity.object_record, object.id);
    }
    let refusal = crate::test_support::with_work_refusal("catia_native_entity_records", decode);
    assert!(matches!(refusal, Err(CodecError::ResourceLimit(limit))
        if limit.operation == "catia_native_entity_records"));
}
