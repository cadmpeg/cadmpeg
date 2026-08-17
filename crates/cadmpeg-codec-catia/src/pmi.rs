// SPDX-License-Identifier: Apache-2.0
//! Transfer of complete CATIA product-manufacturing dimensions.

use std::collections::HashSet;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{format_identity, PmiId};
use cadmpeg_ir::pmi::{DimensionKind, PmiAnnotation, PmiDefinition, PmiQuantity, PmiValue};

use crate::entity_table::{RangeInterval, RangeIntervalSlot};
use crate::native::{
    CatiaConstraintRange, CatiaConstraintRangeFraming, CatiaEntityEvaluation, CatiaNative,
    CatiaRangeInterval,
};

/// Transfer the complete two-selector CATIA dimension production.
///
/// `Range` is used by several unrelated CATIA object families. The exact
/// `Range`/`CstAttr_Dimension` pair, however, declares a dimension value and
/// its direct scalar evaluation. The range nominal and both nullable
/// deviation slots remain independent native productions, so a dimension is
/// emitted only when the nominal and constraint evaluation agree bit-for-bit.
/// A range already bound to a proven sketch constraint stays in that sketch
/// lane; an unresolved source target is deliberately left empty rather than
/// guessed from an incoming class.
pub(crate) fn transfer_dimensions(
    ir: &mut CadIr,
    native: &CatiaNative,
    graph_scope: Option<&HashSet<String>>,
    transferred_sketch_ranges: &HashSet<String>,
) -> usize {
    let mut transferred = 0;
    for entity in native.entity_records.iter().filter(|entity| {
        graph_scope.is_none_or(|scope| scope.contains(entity.object_graph.as_str()))
    }) {
        if transferred_sketch_ranges.contains(&entity.object_record) {
            continue;
        }
        let Some(range) = entity.range_interval.as_ref() else {
            continue;
        };
        let Some(constraint) = entity.constraint_range.as_ref() else {
            continue;
        };
        let Some(definition) = dimension_definition(range, constraint) else {
            continue;
        };
        let id = pmi_id(entity.byte_offset);
        if ir.model.pmi.iter().any(|annotation| annotation.id == id) {
            continue;
        }
        ir.model.pmi.push(PmiAnnotation {
            id,
            name: None,
            targets: Vec::new(),
            definition,
        });
        transferred += 1;
    }
    transferred
}

fn pmi_id(source_offset: u64) -> PmiId {
    PmiId(
        format_identity(
            "catia",
            "model",
            "pmi",
            format!("entity-record-{source_offset:010}"),
        )
        .expect("CATIA PMI source offset produces a valid identity"),
    )
}

fn dimension_definition(
    range: &CatiaRangeInterval,
    constraint: &CatiaConstraintRange,
) -> Option<PmiDefinition> {
    if constraint.constraint.value != "CstAttr_Dimension"
        || !matches!(
            constraint.framing,
            CatiaConstraintRangeFraming::DimensionB8
                | CatiaConstraintRangeFraming::DimensionC1
                | CatiaConstraintRangeFraming::DimensionDC
        )
    {
        return None;
    }
    let nominal = range.nominal.as_ref()?.bits;
    let CatiaEntityEvaluation::Scalar {
        bits: evaluated_nominal,
    } = constraint.evaluation
    else {
        return None;
    };
    if nominal != evaluated_nominal {
        return None;
    }
    let nominal = finite_length(nominal)?;
    let (lower_deviation, upper_deviation) = deviations(&range.interval)?;
    Some(PmiDefinition::Dimension {
        dimension: DimensionKind::Other(constraint.constraint.value.clone()),
        nominal: Some(nominal),
        lower_deviation,
        upper_deviation,
        limits_and_fits: None,
    })
}

fn deviations(interval: &RangeInterval) -> Option<(Option<PmiValue>, Option<PmiValue>)> {
    let Some([lower, upper]) = interval.slots.as_ref() else {
        return Some((None, None));
    };
    let lower = match slot_value(lower) {
        DeviationSlot::Unset => None,
        DeviationSlot::Finite(value) => Some(value),
        DeviationSlot::Invalid => return None,
    };
    let upper = match slot_value(upper) {
        DeviationSlot::Unset => None,
        DeviationSlot::Finite(value) => Some(value),
        DeviationSlot::Invalid => return None,
    };
    Some((lower, upper))
}

enum DeviationSlot {
    Unset,
    Finite(PmiValue),
    Invalid,
}

fn slot_value(slot: &RangeIntervalSlot) -> DeviationSlot {
    match slot {
        RangeIntervalSlot::Binary64 { bits, .. } => {
            finite_length(*bits).map_or(DeviationSlot::Invalid, DeviationSlot::Finite)
        }
        RangeIntervalSlot::Unset { .. } => DeviationSlot::Unset,
    }
}

fn finite_length(bits: u64) -> Option<PmiValue> {
    let value = f64::from_bits(bits);
    value.is_finite().then_some(PmiValue {
        value,
        quantity: PmiQuantity::Length,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity_table::{RangeIntervalPrefix, RangeIntervalSlot};
    use crate::native::{
        CatiaConstraintRange, CatiaEntityRecord, CatiaEntitySchemaValue, CatiaRangeNominal,
        CatiaRangeNominalFraming,
    };
    use cadmpeg_ir::pmi::PmiDefinition;
    use cadmpeg_ir::units::Units;

    fn schema_value(value: &str) -> CatiaEntitySchemaValue {
        CatiaEntitySchemaValue {
            offset: 0,
            ordinal: 0,
            entry: "entry".to_string(),
            value: value.to_string(),
        }
    }

    fn entity_record() -> CatiaEntityRecord {
        let nominal = 12.7_f64.to_bits();
        let lower = (-0.1_f64).to_bits();
        let upper = 0.2_f64.to_bits();
        CatiaEntityRecord {
            id: "catia:outer:entity-record#0000000000".to_string(),
            object_graph: "graph".to_string(),
            object_record: "catia:object#dimension".to_string(),
            ordinal: 0,
            byte_offset: 0,
            byte_len: 0,
            lead: 2,
            inline_body: None,
            definition_len: 0,
            definition_prefix: Vec::new(),
            definition_schema_selections: Vec::new(),
            entity_id: 1,
            definition_suffix: Vec::new(),
            value_len: 0,
            value_payload: Vec::new(),
            value_fields: Vec::new(),
            value_schema_selections: Vec::new(),
            relation_expression: None,
            parameter_value: None,
            range_interval: Some(CatiaRangeInterval {
                range: schema_value("Range"),
                interval: RangeInterval {
                    prefix: RangeIntervalPrefix::Compact { value: 7, width: 1 },
                    slots: Some([
                        RangeIntervalSlot::Binary64 {
                            bits: lower,
                            offset: 0,
                        },
                        RangeIntervalSlot::Binary64 {
                            bits: upper,
                            offset: 0,
                        },
                    ]),
                },
                nominal: Some(CatiaRangeNominal {
                    framing: CatiaRangeNominalFraming::DCToken81DB,
                    bits: nominal,
                    evaluation_opcode_offset: 0,
                }),
                incoming_references: Vec::new(),
                incoming_storage_references: Vec::new(),
            }),
            constraint_range: Some(CatiaConstraintRange {
                range: schema_value("Range"),
                constraint: schema_value("CstAttr_Dimension"),
                framing: CatiaConstraintRangeFraming::DimensionDC,
                evaluation: CatiaEntityEvaluation::Scalar { bits: nominal },
                evaluation_opcode_offset: 0,
                incoming_references: Vec::new(),
                incoming_storage_references: Vec::new(),
            }),
            definition_value: None,
            definition_chain_value: None,
            relation_program_instance: None,
            schema_configuration_record: None,
            schema_configuration_row_link: None,
            formula_relation: None,
            value_packets: Vec::new(),
            numeric_pair: None,
            reference_signature: None,
            record_suffix: Vec::new(),
            suffix_value: None,
            suffix_framing: None,
            suffix_schema_selection: None,
        }
    }

    #[test]
    fn transfers_complete_dimension_values_and_nullable_deviations() {
        let mut ir = CadIr::empty(Units::default());
        let native = CatiaNative {
            entity_records: vec![entity_record()],
            ..CatiaNative::default()
        };

        assert_eq!(
            transfer_dimensions(&mut ir, &native, None, &HashSet::new()),
            1
        );
        let PmiDefinition::Dimension {
            dimension,
            nominal,
            lower_deviation,
            upper_deviation,
            ..
        } = &ir.model.pmi[0].definition
        else {
            panic!("dimension definition");
        };
        assert_eq!(
            dimension,
            &DimensionKind::Other("CstAttr_Dimension".to_string())
        );
        assert_eq!(nominal.expect("nominal").value, 12.7);
        assert_eq!(lower_deviation.expect("lower").value, -0.1);
        assert_eq!(upper_deviation.expect("upper").value, 0.2);
        assert_eq!(
            ir.model.pmi[0].id.0,
            "catia:model:pmi#entity-record-0000000000"
        );
    }

    #[test]
    fn refuses_mismatched_or_sketch_bound_ranges() {
        let mut mismatched = entity_record();
        mismatched
            .constraint_range
            .as_mut()
            .expect("constraint range")
            .evaluation = CatiaEntityEvaluation::Scalar {
            bits: 1.0_f64.to_bits(),
        };
        let native = CatiaNative {
            entity_records: vec![mismatched, entity_record()],
            ..CatiaNative::default()
        };
        let mut ir = CadIr::empty(Units::default());
        let excluded = HashSet::from(["catia:object#dimension".to_string()]);
        assert_eq!(transfer_dimensions(&mut ir, &native, None, &excluded), 0);
        assert!(ir.model.pmi.is_empty());
    }
}
