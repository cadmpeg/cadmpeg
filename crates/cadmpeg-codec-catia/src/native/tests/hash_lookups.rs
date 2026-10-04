// SPDX-License-Identifier: Apache-2.0
//! Charged native hash-map lookup tests.

use crate::test_support::test_object_graph::{
    entity_table_record_with_definition_and_value, entity_table_record_with_value,
    object_graph_from_records, object_graph_record,
};

fn reference_cohort_native() -> crate::native::CatiaNative {
    let mut distinct_value = [
        0x32, 3, 0, 0, 0, 0x82, 0xe8, 0xe0, 0x0a, 0x37, 0x85, 0x81, b'2', b'(', b'E', b')',
        0xfe, 0x32, 4, 0, 0, 0, 0x82, 0xe9, 0xe0, 0x17, 0x08, 0x37, 0xfe, 0xfe, 0xfe,
    ];
    let records = [
        object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x82, 0x81], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x83, 0x81], &[0xfe]),
    ];
    let mut bytes = entity_table_record_with_value(1, &distinct_value);
    bytes.extend(entity_table_record_with_value(2, &distinct_value));
    distinct_value[1] = 5;
    distinct_value[18] = 6;
    bytes.extend(entity_table_record_with_definition_and_value(
        3,
        &[0x01],
        &distinct_value,
    ));
    bytes.push(0xde);
    bytes.extend(object_graph_from_records(&records));
    crate::native::CatiaNative::decode(&bytes)
}

fn derive_cohorts(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    native: &crate::native::CatiaNative,
) -> Result<Vec<crate::native::CatiaReferenceSignatureCohort>, cadmpeg_core::CodecError> {
    super::super::derive_reference_signature_cohorts(ctx, &native.entity_records)
}

#[test]
fn native_cohort_ordinal_lookup_refuses_work_and_preserves_cohorts() {
    let native = reference_cohort_native();
    let service = crate::test_support::with_service_context(|ctx| derive_cohorts(ctx, &native))
        .expect("service profile admits reference cohorts");
    assert_eq!(service.len(), 2);
    assert_eq!(service[0].ordinal, 0);
    assert_eq!(service[1].ordinal, 1);
    assert_eq!(service[0].first_reference(), 3);
    assert_eq!(service[1].first_reference(), 5);
    assert_eq!(
        service[1].members.as_slice(),
        std::slice::from_ref(&native.entity_records[2].id)
    );

    let refused = crate::test_support::with_work_refusal(
        "catia_native_cohort_ordinal_lookup",
        |ctx| {
            let result = derive_cohorts(ctx, &native);
            if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result {
                assert_eq!(
                    limit.dimension,
                    cadmpeg_core::decode::ResourceDimension::WorkUnits
                );
                assert_eq!(ctx.resource_refusal(), Some(*limit));
            }
            result
        },
    );
    assert!(matches!(
        refused,
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_native_cohort_ordinal_lookup"
    ));
}
