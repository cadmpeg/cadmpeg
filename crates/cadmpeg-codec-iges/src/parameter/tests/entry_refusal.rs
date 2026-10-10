// SPDX-License-Identifier: Apache-2.0

use super::integer_parameter_record;
use crate::global::GlobalTable;
use crate::test_support::directory_target;
use cadmpeg_core::CodecError;

#[test]
fn attribute_table_instance_primary_end_refuses_its_definition_lookup() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let definition = directory_target(1, 322);
    let directory = std::collections::BTreeMap::from([(1, &definition)]);
    let records = std::collections::BTreeMap::new();
    let record = integer_parameter_record(3, &[422, 7]);
    let mut entry = directory_target(3, 422);
    entry.structure = -1;
    let operation = "iges attribute definition directory lookup";
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits, operation, |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut widths = super::super::AttributeDefinitionWidths::new(&ctx)?;
            super::super::attribute_table_instance_primary_end(&record, &entry, &directory, &records, &mut widths, &ctx)
        },
    );
    let CodecError::ResourceLimit(limit) = error else { panic!("expected definition lookup refusal"); };
    assert_eq!(limit.operation, operation);
    assert_eq!(limit.additional, 4);
}

#[test]
fn entity_primary_end_with_records_for_entry_refuses_its_definition_lookup() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let definition = directory_target(1, 322);
    let directory = std::collections::BTreeMap::from([(1, &definition)]);
    let records = std::collections::BTreeMap::new();
    let record = integer_parameter_record(3, &[422, 7]);
    let mut entry = directory_target(3, 422);
    entry.structure = -1;
    let operation = "iges attribute definition directory lookup";
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits, operation, |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut widths = super::super::AttributeDefinitionWidths::new(&ctx)?;
            super::super::entity_primary_end_with_records_for_entry(&record, &entry, &directory, &records, GlobalTable::V5Later, &mut widths, &ctx)
        },
    );
    let CodecError::ResourceLimit(limit) = error else { panic!("expected definition lookup refusal"); };
    assert_eq!(limit.operation, operation);
    assert_eq!(limit.additional, 4);
}
