// SPDX-License-Identifier: Apache-2.0
//! Native operation source ordering and small-sort admission.

use super::{design_object, entity_record, object_record};
use crate::native::entity_record::{CatiaEntityRecord, CatiaEntityValueProduction};
use crate::native::{
    CatiaDefinitionChainValue, CatiaDefinitionValue, CatiaDesignObject, CatiaEntityEvaluation,
    CatiaEntityEvaluationEncoding, CatiaEntitySchemaValue, CatiaEntitySuffixPayload,
    CatiaEntitySuffixSchemaValue, CatiaRangeInterval,
};
use cadmpeg_core::decode::{refusal_probe::RefusalProbe, DecodeContext, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::HashMap;

struct Fixture {
    object: CatiaDesignObject,
    entities: Vec<CatiaEntityRecord>,
    records: Vec<crate::native::CatiaObjectRecord>,
    label: &'static str,
    operation: &'static str,
}

impl Fixture {
    fn new(kind: usize, count: usize, tie: bool) -> Self {
        let mut object = design_object("operation", None);
        let mut entities = Vec::new();
        let mut records = Vec::new();
        for index in (1..=count).rev() {
            let id = format!("entity-{index}");
            let record_id = format!("record-{index}");
            let mut entity = entity_record(
                &id,
                &record_id,
                if tie {
                    1
                } else {
                    u64::try_from(index).expect("fixture offset")
                },
                1,
            );
            let schema = CatiaEntitySchemaValue {
                offset: 1,
                ordinal: 1,
                entry: "entry".to_owned(),
                value: "Value".to_owned(),
            };
            match kind {
                0 => {
                    object.definition_values.push(id.clone());
                    entity.value_production = Some(CatiaEntityValueProduction::DefinitionValue(
                        CatiaDefinitionValue {
                            definition: schema,
                            payload: CatiaEntitySuffixPayload::Evaluation {
                                opcode_offset: 2,
                                evaluation: CatiaEntityEvaluation::Scalar {
                                    bits: 1.0_f64.to_bits(),
                                },
                                encoding: CatiaEntityEvaluationEncoding::Direct,
                            },
                            schema_selection: None,
                        },
                    ));
                }
                1 => {
                    object.definition_chain_values.push(id.clone());
                    entity.value_production =
                        Some(CatiaEntityValueProduction::DefinitionChainValue(
                            CatiaDefinitionChainValue {
                                selector: schema.clone(),
                                role: schema,
                                value: CatiaEntitySuffixSchemaValue::Evaluation {
                                    opcode_offset: 2,
                                    evaluation: CatiaEntityEvaluation::Scalar {
                                        bits: 1.0_f64.to_bits(),
                                    },
                                },
                            },
                        ));
                }
                _ => {
                    object.fields.push(record_id.clone());
                    let mut record =
                        object_record(&record_id, Some("operation"), Some(1), Some(1), None, None);
                    record.entity.as_mut().expect("source entity").record = id.clone();
                    records.push(record);
                    entity.range_interval = Some(CatiaRangeInterval {
                        range: schema,
                        interval: crate::entity_table::RangeInterval {
                            prefix: crate::entity_table::RangeIntervalPrefix::Compact {
                                value: 1,
                                width: 1,
                            },
                            slots: None,
                        },
                        nominal: None,
                        incoming_references: Vec::new(),
                        incoming_storage_references: Vec::new(),
                    });
                }
            }
            entities.push(entity);
        }
        let (label, operation) = match kind {
            0 => (
                "catia_definition_value",
                "catia_feature_definition_values_sort",
            ),
            1 => (
                "catia_definition_chain_value",
                "catia_feature_definition_chain_values_sort",
            ),
            _ => ("catia_range", "catia_feature_range_intervals_sort"),
        };
        Self {
            object,
            entities,
            records,
            label,
            operation,
        }
    }

    fn project(&self, ctx: &DecodeContext<'_>) -> Result<Vec<String>, CodecError> {
        let entities = self
            .entities
            .iter()
            .map(|entity| (entity.id.as_str(), entity))
            .collect::<HashMap<_, _>>();
        let records = self
            .records
            .iter()
            .map(|record| (record.id.as_str(), record))
            .collect::<HashMap<_, _>>();
        let mut membership = ctx.reserve_scoped(0, "test operation records")?;
        let properties = super::super::native_operation_definition_properties(
            ctx,
            &mut membership,
            &self.object,
            &records,
            &entities,
            &[&self.object],
        )?;
        assert_eq!(
            properties.definition_value_count
                + properties.definition_chain_value_count
                + properties.range_count,
            self.entities.len()
        );
        Ok((0..self.entities.len())
            .map(|index| {
                properties.source_properties[format!("{}_{index}_entity", self.label).as_str()]
                    .clone()
            })
            .collect())
    }
}

#[test]
fn operation_source_small_sorts_skip_constant_numeric_work() {
    for kind in 0..3 {
        for count in 0..=2 {
            let fixture = Fixture::new(kind, count, false);
            let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, fixture.operation, None);
            let values = crate::test_support::with_work_limit(u64::MAX, |ctx| fixture.project(ctx))
                .expect("zero, singleton and two numeric keys need no sorting admission");
            assert_eq!(
                values,
                (1..=count)
                    .map(|index| format!("entity-{index}"))
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn operation_source_equal_numeric_keys_admit_one_identity_comparison() {
    for kind in 0..3 {
        let fixture = Fixture::new(kind, 2, true);
        let values = crate::test_support::with_service_context(|ctx| fixture.project(ctx))
            .expect("identity tie break");
        assert_eq!(values, ["entity-1", "entity-2"]);
        let error =
            crate::test_support::with_work_refusal(fixture.operation, |ctx| fixture.project(ctx))
                .expect_err("variable identity comparison requires admission");
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.operation == fixture.operation)
        );
    }
}

#[test]
fn operation_source_larger_sorts_preserve_refusal_and_order() {
    for kind in 0..3 {
        let fixture = Fixture::new(kind, 3, false);
        let values = crate::test_support::with_service_context(|ctx| fixture.project(ctx))
            .expect("three source values");
        assert_eq!(values, ["entity-1", "entity-2", "entity-3"]);
        let error =
            crate::test_support::with_work_refusal(fixture.operation, |ctx| fixture.project(ctx))
                .expect_err("larger sort requires admission");
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.operation == fixture.operation)
        );
    }
}

fn parameter_ordering(
    ctx: &DecodeContext<'_>,
    count: usize,
    tie: bool,
    document: bool,
) -> Result<Vec<(String, u32)>, CodecError> {
    let feature = super::feature("owner", "owner-object").id;
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let mut entities = Vec::new();
    let mut records = Vec::new();
    let mut owners = HashMap::new();
    for index in (1..=count).rev() {
        let id = format!("value-{index}");
        let record_id = format!("record-{index}");
        let mut parameter = super::parameter(&id, &id);
        parameter.ordinal = if tie {
            0
        } else {
            u32::try_from(index).expect("fixture ordinal")
        };
        owners.insert(parameter.id.clone(), feature.clone());
        ir.model.parameters.push(parameter);
        let offset = if tie {
            0
        } else {
            u64::try_from(index).expect("fixture position")
        };
        entities.push(entity_record(&id, &record_id, offset, 1));
        let mut record = object_record(&record_id, None, Some(1), None, None, None);
        record.byte_offset = offset;
        records.push(record);
    }
    if document {
        super::super::assign_document_parameter_ordinals(ctx, &mut ir)?;
    } else {
        let entities = entities
            .iter()
            .map(|entity| (entity.id.as_str(), entity))
            .collect();
        let records = records
            .iter()
            .map(|record| (record.id.as_str(), record))
            .collect();
        super::super::assign_feature_parameter_ordinals(
            ctx, &mut ir, &entities, &records, &owners,
        )?;
    }
    assert_eq!(ctx.resource_refusal(), None);
    Ok(ir
        .model
        .parameters
        .into_iter()
        .map(|parameter| (parameter.name, parameter.ordinal))
        .collect())
}

#[test]
fn parameter_small_sorts_skip_numeric_comparison_admission() {
    for (document, operation) in [
        (false, "catia_feature_parameter_order_sort"),
        (true, "catia_document_parameter_order_sort"),
    ] {
        for count in 0..=2 {
            let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
            let values = crate::test_support::with_work_limit(u64::MAX, |ctx| {
                parameter_ordering(ctx, count, false, document)
            })
            .expect("numeric positions need no identity comparison");
            assert_eq!(
                values,
                (1..=count)
                    .rev()
                    .map(|index| (
                        format!("value-{index}"),
                        u32::try_from(index - 1).expect("fixture ordinal")
                    ))
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn parameter_equal_positions_admit_identity_tiebreak() {
    for (document, operation) in [
        (false, "catia_feature_parameter_order_sort"),
        (true, "catia_document_parameter_order_sort"),
    ] {
        let values = crate::test_support::with_service_context(|ctx| {
            parameter_ordering(ctx, 2, true, document)
        })
        .expect("identity tiebreak preserves order");
        assert_eq!(
            values,
            [("value-2".to_owned(), 1), ("value-1".to_owned(), 0)]
        );
        let error = crate::test_support::with_work_refusal(operation, |ctx| {
            parameter_ordering(ctx, 2, true, document)
        })
        .expect_err("identity tiebreak needs admission");
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.operation == operation));
    }
}

#[test]
fn parameter_larger_sorts_preserve_order_and_refusal() {
    for (document, operation) in [
        (false, "catia_feature_parameter_order_sort"),
        (true, "catia_document_parameter_order_sort"),
    ] {
        let values = crate::test_support::with_service_context(|ctx| {
            parameter_ordering(ctx, 3, false, document)
        })
        .expect("larger numeric order");
        assert_eq!(
            values,
            [
                ("value-3".to_owned(), 2),
                ("value-2".to_owned(), 1),
                ("value-1".to_owned(), 0)
            ]
        );
        let error = crate::test_support::with_work_refusal(operation, |ctx| {
            parameter_ordering(ctx, 3, false, document)
        })
        .expect_err("larger sort needs admission");
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.operation == operation));
    }
}
