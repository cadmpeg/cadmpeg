// SPDX-License-Identifier: Apache-2.0

use super::integer_parameter_record;
use crate::global::GlobalTable;
use crate::test_support::{directory_target, with_entry_context};
use cadmpeg_core::CodecError;

#[test]
fn entry_fem_result_absent_or_invalid_count_preserves_original_refusal() {
    with_entry_context(|ctx, original| {
        for values in [&[][..], &[-1; 7][..], &[0; 7][..]] {
            let record = integer_parameter_record(1, values);
            for kind in [146, 148] {
                let result = super::super::fem_result_primary_end(&record, kind, ctx);
                if let Some(first) = original {
                    assert!(
                        matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)
                    );
                } else {
                    // Type 146 has six header fields and no item when its count is zero.
                    let end = if kind == 146 && values == &[0; 7] {
                        6
                    } else {
                        values.len()
                    };
                    assert_eq!(result.unwrap(), end);
                }
            }
        }
    });
}

#[test]
fn entry_font_absent_or_invalid_count_preserves_original_refusal() {
    with_entry_context(|ctx, original| {
        for values in [&[][..], &[-1; 7][..], &[0; 7][..]] {
            let record = integer_parameter_record(1, values);
            let result = super::super::text_font_primary_end(&record, ctx);
            if let Some(first) = original {
                assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first));
            } else {
                assert_eq!(result.unwrap(), values.len());
            }
        }
    });
}

#[test]
fn entry_tabular_data_absent_or_invalid_count_preserves_original_refusal() {
    with_entry_context(|ctx, original| {
        for values in [&[][..], &[-1; 7][..], &[0; 7][..]] {
            let record = integer_parameter_record(1, values);
            let result = super::super::tabular_data_primary_end(&record, ctx);
            if let Some(first) = original {
                assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first));
            } else {
                assert_eq!(result.unwrap(), values.len());
            }
        }
    });
}

#[test]
fn entry_associativity_definition_absent_or_invalid_count_preserves_original_refusal() {
    with_entry_context(|ctx, original| {
        for values in [&[][..], &[-1; 7][..], &[0; 7][..]] {
            let record = integer_parameter_record(1, values);
            let result = super::super::associativity_definition_primary_end(&record, ctx);
            if let Some(first) = original {
                assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first));
            } else {
                assert_eq!(result.unwrap(), values.len());
            }
        }
    });
}

#[test]
fn entry_attribute_definition_absent_or_invalid_count_preserves_original_refusal() {
    with_entry_context(|ctx, original| {
        for values in [&[][..], &[-1; 7][..], &[0; 7][..]] {
            let record = integer_parameter_record(1, values);
            let result = super::super::attribute_table_definition_primary_end(&record, 0, ctx);
            if let Some(first) = original {
                assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first));
            } else {
                assert_eq!(result.unwrap(), values.len());
            }
        }
    });
    let record = integer_parameter_record(1, &[322, 0, 0, 1]);
    with_entry_context(|ctx, original| {
        let result = super::super::attribute_table_definition_primary_end(&record, 3, ctx);
        if let Some(first) = original {
            assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            assert_eq!(result.unwrap(), 4);
        }
    });
}

#[test]
fn entry_loop_absent_or_invalid_count_preserves_original_refusal() {
    with_entry_context(|ctx, original| {
        for values in [&[][..], &[-1; 7][..], &[0; 7][..]] {
            let record = integer_parameter_record(1, values);
            let result = super::super::loop_primary_end(&record, ctx);
            if let Some(first) = original {
                assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first));
            } else {
                assert_eq!(result.unwrap(), values.len());
            }
        }
    });
}

#[test]
fn entry_boundary_absent_or_invalid_count_preserves_original_refusal() {
    with_entry_context(|ctx, original| {
        for values in [&[][..], &[-1; 7][..], &[0; 7][..]] {
            let record = integer_parameter_record(1, values);
            let result = super::super::boundary_primary_end(&record, ctx);
            if let Some(first) = original {
                assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first));
            } else {
                assert_eq!(result.unwrap(), values.len());
            }
        }
    });
}

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
fn entry_primary_layout_fixed_or_unknown_type_preserves_original_refusal() {
    let record = integer_parameter_record(1, &[]);
    with_entry_context(|ctx, original| {
        for (kind, expected) in [(116, Some(5)), (999, None)] {
            let entry = directory_target(1, kind);
            let result = super::super::entity_primary_end_for_entry(
                &record,
                &entry,
                GlobalTable::V5Later,
                ctx,
            );
            if let Some(first) = original {
                assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first));
            } else {
                assert_eq!(result.unwrap(), expected);
            }
        }
    });
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
