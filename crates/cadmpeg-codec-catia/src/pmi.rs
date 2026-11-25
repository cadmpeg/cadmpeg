// SPDX-License-Identifier: Apache-2.0
//! Transfer source-closed CATIA product-manufacturing annotations.

pub(crate) mod geometric_tolerance;
mod source;

use std::cmp::Ordering;
use std::collections::HashSet;

use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::options::NativeOption;
use cadmpeg_ir::design::parameter::{Deviations, Tolerance};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::PmiId;
use cadmpeg_ir::pmi::{
    PmiAnnotation, PmiDefinition, PmiDimension, PmiDimensionKind, PmiQuantity, PmiValue,
};
use cadmpeg_ir::scalar::{FiniteReal, Length};

use crate::native::entity_record::CatiaEntityRecord;
use crate::native::{CatiaNative, CatiaRangeInterval};

/// Transfer complete CATIA dimension productions.
///
/// `Range` is used by several unrelated CATIA object families. A Range-only
/// `DiameterThread` or `FeatureRSUR` definition declares a diameter or
/// feature-size dimension when one paired
/// payload owner selects its complete finite nominal and deviation interval.
/// The range nominal and deviation slots remain independent native
/// productions, so incomplete or conflicting values are not transferred.
/// A range already bound to a proven sketch constraint stays in that sketch
/// lane; an unresolved source target is deliberately left empty rather than
/// guessed from an incoming class.
pub(crate) fn transfer_dimensions(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    native: &CatiaNative,
    graph_scope: &crate::decode::ModelingGraphScope,
    transferred_sketch_ranges: &HashSet<String>,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut transferred = 0;
    let mut present = HashSet::new();
    for annotation in &ir.model.pmi {
        present.insert(annotation.id.clone());
    }
    for entity in native
        .entity_records
        .iter()
        .filter(|entity| graph_scope.contains(entity.object_graph.as_str()))
    {
        if transferred_sketch_ranges.contains(&entity.object_record) {
            continue;
        }
        let Some((definition, stored)) = dimension_definition(entity) else {
            continue;
        };
        let id = pmi_id(entity.byte_offset)?;
        if !present.insert(id.clone()) {
            continue;
        }
        ctx.charge_entities(1, "admit CATIA PMI dimension")?;
        let mut annotation = PmiAnnotation::new(id, Vec::new(), definition);
        match stored {
            StoredDeviations::LowerFirst => {}
            StoredDeviations::UpperFirst => {
                annotation.source_settings.insert(
                    cadmpeg_ir::codec_name!("catia:tol_order"),
                    NativeOption::Text(Dimension::Literal(cadmpeg_core::nonblank_literal!(
                        "upper_lower"
                    ))),
                );
            }
            StoredDeviations::Equal { lower, upper } => {
                for (key, value) in [
                    (cadmpeg_ir::codec_name!("catia:lower_deviation"), lower),
                    (cadmpeg_ir::codec_name!("catia:upper_deviation"), upper),
                ] {
                    annotation.native_options.insert(
                        key,
                        NativeOption::Length(Dimension::Literal(Length::from_assigned_real(value))),
                    );
                }
            }
        }
        ir.model.pmi.push(annotation);
        transferred += 1;
    }
    Ok(transferred)
}

fn pmi_id(source_offset: u64) -> Result<PmiId, cadmpeg_core::CodecError> {
    let value = format!("catia:model:pmi#entity-record-{source_offset:010}");
    PmiId::mint(value).map_err(cadmpeg_core::CodecError::malformed)
}

/// How a Range stores its deviation pair (§5.12). Slot 0 is the lower
/// deviation and slot 1 the upper.
enum StoredDeviations {
    LowerFirst,
    /// The slots hold the upper deviation first; the annotation keeps the
    /// order as `catia:tol_order`.
    UpperFirst,
    /// The two deviations are equal: the dimension states no tolerance, and
    /// the annotation keeps both as native options.
    Equal {
        lower: FiniteReal,
        upper: FiniteReal,
    },
}

fn dimension_definition(entity: &CatiaEntityRecord) -> Option<(PmiDefinition, StoredDeviations)> {
    let range = entity.range_interval.as_ref()?;
    if entity.constraint_range().is_some() {
        return None;
    }
    range_only_dimension_definition(entity, range)
}

fn range_only_dimension_definition(
    entity: &CatiaEntityRecord,
    range: &CatiaRangeInterval,
) -> Option<(PmiDefinition, StoredDeviations)> {
    if entity.lead != 2 {
        return None;
    }
    let [definition] = entity.definition_schema_selections.as_slice() else {
        return None;
    };
    let [selection] = entity.value_schema_selections.as_slice() else {
        return None;
    };
    if selection.name != "Range"
        || range.range.entry != selection.entry
        || range.range.ordinal != selection.ordinal
        || range.range.offset != selection.offset
        || !range.incoming_storage_references.is_empty()
    {
        return None;
    }
    let [owner] = range.incoming_references.as_slice() else {
        return None;
    };
    let source = owner.source_entity.as_ref()?;
    if source.is_null() || source.entity().is_none() {
        return None;
    }
    let dimension = match definition.name.as_deref()? {
        "DiameterThread" => PmiDimensionKind::Diameter,
        "FeatureRSUR" => PmiDimensionKind::Size,
        _ => return None,
    };
    let nominal = finite_length(range.nominal.as_ref()?.bits)?;
    let [lower, upper] = range.interval.slots.as_ref()?;
    let lower_deviation = finite_length(lower.bits())?;
    let upper_deviation = finite_length(upper.bits())?;
    let (lower, upper) = (lower_deviation.value, upper_deviation.value);
    let mut dimension = PmiDimension::new(dimension);
    dimension.nominal = Some(Dimension::Literal(nominal));
    let stored = match lower.get().partial_cmp(&upper.get()) {
        Some(Ordering::Less) => {
            dimension.tolerance = Some(Tolerance::Deviations(Deviations { lower, upper }));
            StoredDeviations::LowerFirst
        }
        Some(Ordering::Greater) => {
            dimension.tolerance = Some(Tolerance::Deviations(Deviations {
                lower: upper,
                upper: lower,
            }));
            StoredDeviations::UpperFirst
        }
        Some(Ordering::Equal) | None => StoredDeviations::Equal { lower, upper },
    };
    Some((PmiDefinition::Dimension(dimension), stored))
}

fn finite_length(bits: u64) -> Option<PmiValue> {
    let value = f64::from_bits(bits);
    PmiValue::new(value, PmiQuantity::Length)
}

#[cfg(test)]
mod tests {
    use super::transfer_dimensions;
    use crate::entity_table::{RangeInterval, RangeIntervalPrefix, RangeIntervalSlot};
    use crate::native::entity_record::{CatiaEntityRecord, CatiaEntityRecordBody};
    use crate::native::CatiaNative;
    use crate::native::CatiaRangeInterval;
    use crate::native::{
        CatiaConstraintRange, CatiaConstraintRangeFraming, CatiaDefinitionSchemaSelection,
        CatiaEntityEvaluation, CatiaEntityIncomingReference, CatiaEntityReference,
        CatiaEntitySchemaValue, CatiaEntityValueSchemaSelection, CatiaObjectRecordReferenceSource,
        CatiaRangeNominal, CatiaRangeNominalFraming,
    };
    use crate::test_support::with_service_context;
    use cadmpeg_ir::design::parameter::Tolerance;
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::pmi::PmiDefinition;
    use cadmpeg_ir::pmi::PmiDimensionKind as DimensionKind;
    use std::collections::HashSet;

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
            lead: 2,
            body: CatiaEntityRecordBody::empty_nested(),
            definition_schema_selections: Vec::new(),
            entity_id: 1,
            value_schema_selections: Vec::new(),
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
                project_target: None,
            }),
            value_production: Some(
                crate::native::entity_record::CatiaEntityValueProduction::ConstraintRange(
                    CatiaConstraintRange {
                        range: schema_value("Range"),
                        constraint: schema_value("CstAttr_Dimension"),
                        framing: CatiaConstraintRangeFraming::DimensionDC,
                        evaluation: CatiaEntityEvaluation::Scalar { bits: nominal },
                        evaluation_opcode_offset: 0,
                        incoming_references: Vec::new(),
                        incoming_storage_references: Vec::new(),
                    },
                ),
            ),
            object_production: None,

            reference_signature: None,
            suffix: None,
            suffix_schema_selection: None,
        }
    }

    fn range_only_entity(definition: &str) -> CatiaEntityRecord {
        let mut entity = entity_record();
        entity.definition_schema_selections = vec![CatiaDefinitionSchemaSelection {
            offset: 3,
            ordinal: 17,
            entry: Some("definition-entry".to_string()),
            name: Some(definition.to_string()),
        }];
        entity.value_schema_selections = vec![CatiaEntityValueSchemaSelection {
            offset: 5,
            ordinal: 23,
            entry: "range-entry".to_string(),
            name: "Range".to_string(),
            encoded_value: Vec::new(),
            packets: Vec::new(),
        }];
        let range = entity
            .range_interval
            .as_mut()
            .expect("synthetic Range interval");
        range.range = CatiaEntitySchemaValue {
            offset: 5,
            ordinal: 23,
            entry: "range-entry".to_string(),
            value: "Range".to_string(),
        };
        range.incoming_references = vec![CatiaEntityIncomingReference {
            object_record: "catia:object#owner".to_string(),
            source_entity: Some(CatiaEntityReference::resolved_or_unresolved(
                2,
                Some("catia:entity#owner".to_string()),
                None,
            )),
            payload_offset: 7,
            source: CatiaObjectRecordReferenceSource::Field,
        }];
        entity.value_production = None;
        entity
    }

    #[test]
    fn constraint_ranges_remain_native_without_a_physical_quantity() {
        let mut ir = CadIr::empty();
        let native = CatiaNative {
            entity_records: vec![entity_record()],
            ..CatiaNative::default()
        };

        assert_eq!(
            with_service_context(|ctx| transfer_dimensions(
                ctx,
                &mut ir,
                &native,
                &crate::decode::ModelingGraphScope::Unscoped,
                &HashSet::new()
            )
            .expect("transfer dimensions")),
            0
        );
        assert!(ir.model.pmi.is_empty());
    }

    #[test]
    fn transfers_source_closed_range_only_size_dimensions() {
        let diameter = range_only_entity("DiameterThread");
        let mut size = range_only_entity("FeatureRSUR");
        size.byte_offset = 1;
        let native = CatiaNative {
            entity_records: vec![diameter, size],
            ..CatiaNative::default()
        };
        let mut ir = CadIr::empty();

        assert_eq!(
            with_service_context(|ctx| transfer_dimensions(
                ctx,
                &mut ir,
                &native,
                &crate::decode::ModelingGraphScope::Unscoped,
                &HashSet::new()
            )
            .expect("transfer dimensions")),
            2
        );
        let dimensions = ir
            .model
            .pmi
            .iter()
            .map(|annotation| match &annotation.definition {
                PmiDefinition::Dimension(relation) => {
                    let nominal = relation.literal_nominal().expect("dimension nominal");
                    let Some(Tolerance::Deviations(deviations)) = &relation.tolerance else {
                        panic!("dimension tolerance");
                    };
                    (
                        &relation.dimension,
                        nominal.value.get(),
                        deviations.lower.get(),
                        deviations.upper.get(),
                    )
                }
                _ => panic!("dimension annotation"),
            })
            .collect::<Vec<_>>();
        assert_eq!(dimensions[0], (&DimensionKind::Diameter, 12.7, -0.1, 0.2));
        assert_eq!(dimensions[1], (&DimensionKind::Size, 12.7, -0.1, 0.2));
    }

    #[test]
    fn pmi_dimension_entity_limit_refuses_before_second_dimension() {
        let diameter = range_only_entity("DiameterThread");
        let mut size = range_only_entity("FeatureRSUR");
        size.byte_offset = 1;
        let native = CatiaNative {
            entity_records: vec![diameter, size],
            ..CatiaNative::default()
        };
        let mut ir = CadIr::empty();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_entities = 1;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty test root fits service input limit");
        let error = transfer_dimensions(
            &ctx,
            &mut ir,
            &native,
            &crate::decode::ModelingGraphScope::Unscoped,
            &HashSet::new(),
        )
        .expect_err("two dimensions exceed one entity");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities
                && limit.operation == "admit CATIA PMI dimension")
        );
        assert_eq!(ir.model.pmi.len(), 1);
    }

    #[test]
    fn range_only_dimensions_require_exact_schema_and_owner_closure() {
        let wrong_definition = range_only_entity("Hole_Diameter");
        let mut wrong_lead = range_only_entity("DiameterThread");
        wrong_lead.lead = 1;
        let mut extra_selector = range_only_entity("DiameterThread");
        extra_selector
            .value_schema_selections
            .push(CatiaEntityValueSchemaSelection {
                offset: 9,
                ordinal: 24,
                entry: "other-entry".to_string(),
                name: "Other".to_string(),
                encoded_value: Vec::new(),
                packets: Vec::new(),
            });
        let mut duplicate_owner = range_only_entity("DiameterThread");
        let second_owner = duplicate_owner
            .range_interval
            .as_ref()
            .expect("Range interval")
            .incoming_references[0]
            .clone();
        duplicate_owner
            .range_interval
            .as_mut()
            .expect("Range interval")
            .incoming_references
            .push(second_owner);
        let mut unresolved_owner = range_only_entity("FeatureRSUR");
        let unresolved_source = unresolved_owner
            .range_interval
            .as_mut()
            .expect("Range interval")
            .incoming_references[0]
            .source_entity
            .as_mut()
            .expect("source entity");
        *unresolved_source = unresolved_source.clone().without_entity();
        let native = CatiaNative {
            entity_records: vec![
                wrong_definition,
                wrong_lead,
                extra_selector,
                duplicate_owner,
                unresolved_owner,
            ],
            ..CatiaNative::default()
        };
        let mut ir = CadIr::empty();

        assert_eq!(
            with_service_context(|ctx| transfer_dimensions(
                ctx,
                &mut ir,
                &native,
                &crate::decode::ModelingGraphScope::Unscoped,
                &HashSet::new()
            )
            .expect("transfer dimensions")),
            0
        );
        assert!(ir.model.pmi.is_empty());
    }

    #[test]
    fn a_zero_literal_deviation_slot_is_a_zero_deviation() {
        let mut zero_deviation = range_only_entity("DiameterThread");
        zero_deviation
            .range_interval
            .as_mut()
            .expect("Range interval")
            .interval
            .slots
            .as_mut()
            .expect("deviation slots")[0] = RangeIntervalSlot::ZeroLiteral { offset: 0 };
        let native = CatiaNative {
            entity_records: vec![zero_deviation],
            ..CatiaNative::default()
        };
        let mut ir = CadIr::empty();
        crate::test_support::with_service_context(|ctx| {
            transfer_dimensions(
                ctx,
                &mut ir,
                &native,
                &crate::decode::ModelingGraphScope::Unscoped,
                &HashSet::new(),
            )
        })
        .expect("zero-deviation dimension");
        let [annotation] = ir.model.pmi.as_slice() else {
            panic!("one PMI entry");
        };
        assert!(matches!(
            &annotation.definition,
            PmiDefinition::Dimension(dimension)
                if matches!(
                    &dimension.tolerance,
                    Some(Tolerance::Deviations(deviations)) if deviations.lower.get() == 0.0
                )
        ));
    }

    #[test]
    fn a_zero_zero_range_states_no_tolerance_and_keeps_both_deviations() {
        let mut zero_zero = range_only_entity("DiameterThread");
        *zero_zero
            .range_interval
            .as_mut()
            .expect("Range interval")
            .interval
            .slots
            .as_mut()
            .expect("deviation slots") = [
            RangeIntervalSlot::ZeroLiteral { offset: 0 },
            RangeIntervalSlot::ZeroLiteral { offset: 8 },
        ];
        let native = CatiaNative {
            entity_records: vec![zero_zero],
            ..CatiaNative::default()
        };
        let mut ir = CadIr::empty();
        crate::test_support::with_service_context(|ctx| {
            transfer_dimensions(
                ctx,
                &mut ir,
                &native,
                &crate::decode::ModelingGraphScope::Unscoped,
                &HashSet::new(),
            )
        })
        .expect("zero-zero dimension");
        let [annotation] = ir.model.pmi.as_slice() else {
            panic!("one PMI entry");
        };
        let PmiDefinition::Dimension(dimension) = &annotation.definition else {
            panic!("dimension annotation");
        };
        assert_eq!(dimension.tolerance, None);
        let zero = cadmpeg_ir::design::options::NativeOption::Length(
            cadmpeg_ir::design::dimension::Dimension::Literal(cadmpeg_ir::scalar::Length::ZERO),
        );
        let options = annotation
            .native_options
            .iter()
            .map(|(key, value)| (key.as_str(), value))
            .collect::<Vec<_>>();
        assert_eq!(
            options,
            [
                ("catia:lower_deviation", &zero),
                ("catia:upper_deviation", &zero),
            ]
        );
    }

    #[test]
    fn a_range_stored_upper_first_states_its_deviations_in_order_and_keeps_the_order() {
        let mut upper_first = range_only_entity("DiameterThread");
        *upper_first
            .range_interval
            .as_mut()
            .expect("Range interval")
            .interval
            .slots
            .as_mut()
            .expect("deviation slots") = [
            RangeIntervalSlot::Binary64 {
                bits: 0.1_f64.to_bits(),
                offset: 0,
            },
            RangeIntervalSlot::Binary64 {
                bits: (-0.1_f64).to_bits(),
                offset: 8,
            },
        ];
        let native = CatiaNative {
            entity_records: vec![upper_first],
            ..CatiaNative::default()
        };
        let mut ir = CadIr::empty();
        crate::test_support::with_service_context(|ctx| {
            transfer_dimensions(
                ctx,
                &mut ir,
                &native,
                &crate::decode::ModelingGraphScope::Unscoped,
                &HashSet::new(),
            )
        })
        .expect("upper-first dimension");
        let [annotation] = ir.model.pmi.as_slice() else {
            panic!("one PMI entry");
        };
        let PmiDefinition::Dimension(dimension) = &annotation.definition else {
            panic!("dimension annotation");
        };
        let Some(Tolerance::Deviations(deviations)) = &dimension.tolerance else {
            panic!("dimension tolerance");
        };
        assert_eq!(
            (deviations.lower.get(), deviations.upper.get()),
            (-0.1, 0.1)
        );
        assert!(annotation.native_options.is_empty());
        let settings = annotation
            .source_settings
            .iter()
            .map(|(key, value)| (key.as_str(), value))
            .collect::<Vec<_>>();
        assert_eq!(
            settings,
            [(
                "catia:tol_order",
                &cadmpeg_ir::design::options::NativeOption::Text(
                    cadmpeg_ir::design::dimension::Dimension::Literal(
                        cadmpeg_core::nonblank_literal!("upper_lower")
                    )
                )
            )]
        );
    }

    #[test]
    fn one_thread_diameter_record_adds_one_pmi_entry() {
        let native = CatiaNative {
            entity_records: vec![range_only_entity("DiameterThread")],
            ..CatiaNative::default()
        };
        let mut ir = CadIr::empty();
        crate::test_support::with_service_context(|ctx| {
            transfer_dimensions(
                ctx,
                &mut ir,
                &native,
                &crate::decode::ModelingGraphScope::Unscoped,
                &HashSet::new(),
            )
        })
        .expect("first dimension");

        assert_eq!(ir.model.pmi.len(), 1);
    }

    #[test]
    fn many_dimensions_transfer_once_per_identity() {
        const DIMENSIONS: u64 = 4_096;
        let mut entity_records = Vec::new();
        for byte_offset in (0..DIMENSIONS).chain(0..DIMENSIONS / 2) {
            let mut entity = range_only_entity("DiameterThread");
            entity.byte_offset = byte_offset;
            entity_records.push(entity);
        }
        let native = CatiaNative {
            entity_records,
            ..CatiaNative::default()
        };
        let mut ir = CadIr::empty();
        let transfer = |ir: &mut CadIr| {
            with_service_context(|ctx| {
                transfer_dimensions(
                    ctx,
                    ir,
                    &native,
                    &crate::decode::ModelingGraphScope::Unscoped,
                    &HashSet::new(),
                )
            })
            .expect("transfer dimensions")
        };
        assert_eq!(transfer(&mut ir), 4_096);
        ir.model.pmi.truncate(1);
        assert_eq!(transfer(&mut ir), 4_095);
        let ids = ir
            .model
            .pmi
            .iter()
            .map(|annotation| annotation.id.as_str().to_owned())
            .collect::<Vec<_>>();
        let expected = (0..DIMENSIONS)
            .map(|offset| format!("catia:model:pmi#entity-record-{offset:010}"))
            .collect::<Vec<_>>();
        assert_eq!(ids, expected);
    }

    #[test]
    fn transferred_dimension_id_names_its_entity_record() {
        let native = CatiaNative {
            entity_records: vec![range_only_entity("DiameterThread")],
            ..CatiaNative::default()
        };
        let mut ir = CadIr::empty();
        assert!(ir.model.pmi.is_empty());
        let admitted = with_service_context(|ctx| {
            transfer_dimensions(
                ctx,
                &mut ir,
                &native,
                &crate::decode::ModelingGraphScope::Unscoped,
                &HashSet::new(),
            )
        })
        .expect("transfer dimensions");
        assert_eq!(admitted, 1);
        assert_eq!(
            ir.model.pmi[0].id.as_str(),
            "catia:model:pmi#entity-record-0000000000"
        );
    }
}
