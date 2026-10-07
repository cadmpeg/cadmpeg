// SPDX-License-Identifier: Apache-2.0
//! STEP semantic product-manufacturing information.

use crate::ids::kind;
use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU32;

use super::reference::{first_matching, references};
use super::{find_record_value, named_parameter, source_numeric_id, RecordExt, ValueExt};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::PmiId;
use cadmpeg_ir::pmi::{
    DatumReference, DatumTargetForm, DimensionKind, DimensionTolerance, GeometricToleranceKind,
    LimitsAndFits, PmiDefinition, PmiDimension, PmiQuantity, PmiTarget, PmiValue,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::transform::Transform;

use crate::ids;
use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

use super::decode_text_charged;
use super::geometry::GeometryData;
use super::topology::TopologyData;
use super::StageOutcome;

mod annotations;

use annotations::{AnnotationDraft, AnnotationIndex, Annotations};

struct MeasureContext<'a> {
    length_scale: f64,
    angle_scale: f64,
    graph_limit: usize,
    losses: &'a mut Vec<LossNote>,
}

fn collect_pmi_references(
    values: &[Value],
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<Vec<u64>, CodecError> {
    let mut ids = Vec::new();
    for value in ctx.admit_iter(values, "STEP collect pmi references traversal")? {
        for id in references(value, ctx) {
            let id = id?;
            ctx.push_vec(&mut ids, id, operation)?;
        }
    }
    Ok(ids)
}

pub(super) fn decode(
    exchange: &Exchange,
    geometry: &GeometryData,
    topology: &TopologyData,
    ir: &mut CadIr,
    ctx: &DecodeContext<'_>,
) -> Result<StageOutcome<()>, CodecError> {
    if !exchange.has_entity_matching(ctx, is_pmi_entity_name)? {
        return Ok(StageOutcome {
            value: (),
            claims: BTreeSet::new(),
            losses: Vec::new(),
            notes: Vec::new(),
        });
    }
    let mut base_aspects = BTreeSet::new();
    for entity in exchange.entities_any(ctx, &["SHAPE_ASPECT", "DATUM_FEATURE", "DATUM"])? {
        let (id, _) = entity?;
        ctx.charge_work(1, "step_pmi_base_aspects")?;
        ctx.insert_btree_set(&mut base_aspects, id, "step_pmi_base_aspects")?;
    }
    let mut shape_aspects = BTreeSet::new();
    for id in exchange.matching_entity_ids(ctx, is_shape_aspect_name)? {
        let id = id?;
        ctx.charge_work(1, "step_pmi_shape_aspects")?;
        ctx.insert_btree_set(&mut shape_aspects, id, "step_pmi_shape_aspects")?;
    }
    let mut typed = BTreeSet::new();
    let mut losses = Vec::new();
    let mut annotations = Annotations::default();
    let hidden_presentation_annotations = hidden_presentation_annotation_ids(exchange, ctx)?;

    let mut presentation_semantics = BTreeMap::<u64, Vec<u64>>::new();
    let graph_limit = super::record_graph_limit(ctx);
    let characteristic_values =
        characteristic_values(exchange, geometry, &mut losses, graph_limit, ctx)?;
    for (id, record) in exchange.entities(ctx, "DATUM")? {
        let identification = named_parameter(ctx, record, "DATUM", 0)?
            .map(|value| {
                decode_text_charged(
                    exchange,
                    value,
                    &mut losses,
                    id,
                    "datum identification",
                    StepLossCode::MetadataStringInvalid,
                    ctx,
                )
            })
            .transpose()?
            .flatten()
            .unwrap_or_else(|| format!("#{id}"));
        annotations.push(
            ctx,
            ir,
            id,
            AnnotationDraft {
                name: shape_aspect_parameter(ctx, record, 0)?
                    .map(|value| {
                        decode_text_charged(
                            exchange,
                            value,
                            &mut losses,
                            id,
                            "datum name",
                            StepLossCode::MetadataStringInvalid,
                            ctx,
                        )
                    })
                    .transpose()?
                    .flatten(),
                targets: targets([Ok(id)], ctx)?,
                visible: None,
                definition: PmiDefinition::Datum { identification },
            },
        )?;
        ctx.insert_btree_set(&mut typed, id, "step_pmi_typed_claims")?;
    }

    for entity in exchange.matching_entity_ids(ctx, is_datum_target_name)? {
        let id = entity?;
        let Some(record) = exchange.records().get(&id) else {
            continue;
        };
        let form = shape_aspect_parameter(ctx, record, 1)?
            .map(|value| {
                decode_text_charged(
                    exchange,
                    value,
                    &mut losses,
                    id,
                    "datum target form",
                    StepLossCode::MetadataStringInvalid,
                    ctx,
                )
            })
            .transpose()?
            .flatten()
            .unwrap_or_default();
        let identification = datum_target_identification_parameter(ctx, record)?
            .map(|value| {
                decode_text_charged(
                    exchange,
                    value,
                    &mut losses,
                    id,
                    "datum target identification",
                    StepLossCode::MetadataStringInvalid,
                    ctx,
                )
            })
            .transpose()?
            .flatten()
            .unwrap_or_else(|| format!("#{id}"));
        annotations.push(
            ctx,
            ir,
            id,
            AnnotationDraft {
                name: shape_aspect_parameter(ctx, record, 0)?
                    .map(|value| {
                        decode_text_charged(
                            exchange,
                            value,
                            &mut losses,
                            id,
                            "datum target name",
                            StepLossCode::MetadataStringInvalid,
                            ctx,
                        )
                    })
                    .transpose()?
                    .flatten(),
                targets: targets([Ok(id)], ctx)?,
                visible: None,
                definition: PmiDefinition::DatumTarget {
                    form: datum_target_form(&form, ctx)?,
                    identification,
                    basis: Vec::new(),
                },
            },
        )?;
        ctx.insert_btree_set(&mut typed, id, "step_pmi_typed_claims")?;
    }

    for (id, record) in exchange.entities(ctx, "DATUM_SYSTEM")? {
        let constituents = ctx
            .admit_iter(&(record.parameters())[..], "STEP decode traversal")?
            .rev()
            .find_map(ValueExt::list)
            .unwrap_or_default();
        let mut datum_records = BTreeSet::new();
        let mut measurements = measure_context(geometry, id, &mut losses, graph_limit);
        let mut datum_references = Vec::new();
        for (index, constituent) in ctx
            .admit_iter(&constituents[..], "STEP decode traversal")?
            .enumerate()
        {
            let Some(precedence) = u32::try_from(index + 1).ok().and_then(NonZeroU32::new) else {
                continue;
            };
            for reference in datum_references_for_compartment(
                constituent,
                precedence,
                exchange,
                &annotations,
                &mut datum_records,
                &mut measurements,
                ctx,
            )? {
                ctx.push_vec(
                    &mut datum_references,
                    reference,
                    "step_pmi_datum_system_references",
                )?;
            }
        }
        admit_datum_reference_maps(&datum_references, ctx)?;
        let datum_references = match datum_references.try_into() {
            Ok(references) => references,
            Err(error) => {
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::PmiDatumSystemInvalid.note(ctx.format_retained(
                        format_args!("DATUM_SYSTEM #{id} omitted: {error}"),
                        "STEP decode text",
                    )?),
                    "step_pmi_losses",
                )?;
                continue;
            }
        };
        annotations.push(
            ctx,
            ir,
            id,
            AnnotationDraft {
                name: shape_aspect_parameter(ctx, record, 0)?
                    .map(|value| {
                        decode_text_charged(
                            exchange,
                            value,
                            &mut losses,
                            id,
                            "datum system name",
                            StepLossCode::MetadataStringInvalid,
                            ctx,
                        )
                    })
                    .transpose()?
                    .flatten(),
                targets: targets(
                    record
                        .parameters()
                        .iter()
                        .flat_map(|value| references(value, ctx))
                        .filter(|id| match id {
                            Ok(id) => base_aspects.contains(id),
                            Err(_) => true,
                        }),
                    ctx,
                )?,
                visible: None,
                definition: PmiDefinition::DatumSystem {
                    references: datum_references,
                },
            },
        )?;
        ctx.insert_btree_set(&mut typed, id, "step_pmi_typed_claims")?;
        super::claim_records(ctx, &mut typed, datum_records, "step_pmi_typed_claims")?;
    }

    for entity in exchange.matching_entity_ids(ctx, is_dimension_name)? {
        let id = entity?;
        let Some(record) = exchange.records().get(&id) else {
            continue;
        };
        let Some((dimension_name, mut kind)) = dimension_descriptor(record, ctx)? else {
            continue;
        };
        let mut name = None;
        'record_parameters: for partial in
            ctx.admit_iter(&record.partials[..], "STEP decode traversal")?
        {
            for value in ctx.admit_iter(
                partial.parameters.as_slice(),
                "STEP record parameter traversal",
            )? {
                if let Some(text) = decode_text_charged(
                    exchange,
                    value,
                    &mut losses,
                    id,
                    "dimension name",
                    StepLossCode::MetadataStringInvalid,
                    ctx,
                )? {
                    name = Some(text);
                    break 'record_parameters;
                }
            }
        }
        if matches!(kind, DimensionKind::Size) {
            let category = if dimension_name.starts_with("DIMENSIONAL_SIZE_WITH_DATUM_FEATURE") {
                let mut category = None;
                'record_parameters: for partial in ctx
                    .admit_iter(&record.partials[..], "STEP decode traversal")?
                    .map(|partial| -> Result<Option<_>, CodecError> {
                        Ok((ctx.equal(
                            partial.name.as_str(),
                            dimension_name,
                            "STEP PMI dimension name equality",
                        )?)
                        .then_some(partial))
                    })
                    .find_map(Result::transpose)
                    .transpose()?
                    .into_iter()
                {
                    for value in ctx
                        .admit_iter(
                            partial.parameters.as_slice(),
                            "STEP record parameter traversal",
                        )?
                        .rev()
                    {
                        if let Some(text) = decode_text_charged(
                            exchange,
                            value,
                            &mut losses,
                            id,
                            "dimension category",
                            StepLossCode::MetadataStringInvalid,
                            ctx,
                        )? {
                            category = Some(text);
                            break 'record_parameters;
                        }
                    }
                }
                category
            } else {
                None
            };
            let category = category.as_deref().or(name.as_deref());
            kind = if category
                .map(|value| {
                    ctx.eq_ignore_ascii_case(
                        value,
                        "diameter",
                        "STEP dimension category case equality",
                    )
                })
                .transpose()?
                .unwrap_or(false)
            {
                DimensionKind::Diameter
            } else if category
                .map(|value| {
                    ctx.eq_ignore_ascii_case(
                        value,
                        "radius",
                        "STEP dimension category case equality",
                    )
                })
                .transpose()?
                .unwrap_or(false)
            {
                DimensionKind::Radius
            } else {
                kind
            };
        }
        let nominal = characteristic_values.get(&id).copied();
        let definition = PmiDimension::new(kind, nominal, None)
            .map_err(|error| CodecError::malformed(format_args!("dimension #{id}: {error}")))?;
        let mut aspect_targets = Vec::new();
        let mut aspect_ids = BTreeSet::new();
        for partial in ctx.admit_iter(
            &record.partials[..],
            "STEP dimension aspect partial traversal",
        )? {
            for value in ctx.admit_iter(
                partial.parameters.as_slice(),
                "STEP dimension aspect parameter traversal",
            )? {
                for reference in references(value, ctx) {
                    let id = reference?;
                    if shape_aspects.contains(&id) && !aspect_ids.contains(&id) {
                        ctx.insert_btree_set(&mut aspect_ids, id, "step_pmi_target_ids")?;
                        ctx.push_vec(
                            &mut aspect_targets,
                            PmiTarget::ShapeAspect {
                                source_id: super::step_source_id(ctx, id)?,
                            },
                            "step_pmi_target_items",
                        )?;
                    }
                }
            }
        }
        annotations.push(
            ctx,
            ir,
            id,
            AnnotationDraft {
                name,
                targets: aspect_targets,
                visible: None,
                definition: PmiDefinition::Dimension(definition),
            },
        )?;
        ctx.insert_btree_set(&mut typed, id, "step_pmi_typed_claims")?;
    }

    for (id, record) in exchange.entities(ctx, "PLUS_MINUS_TOLERANCE")? {
        let refs =
            collect_pmi_references(record.parameters(), ctx, "step_pmi_plus_minus_references")?;
        let dimension = ctx
            .admit_iter(&refs[..], "STEP decode traversal")?
            .find_map(|reference| annotations.get(*reference));
        let limits = ctx
            .admit_iter(&refs[..], "STEP decode traversal")?
            .find_map(|reference| {
                exchange
                    .records()
                    .get(reference)
                    .filter(|candidate| candidate.simple_name() == Some("TOLERANCE_VALUE"))
            });
        let fit = ctx
            .admit_iter(&refs[..], "STEP decode traversal")?
            .find_map(|reference| {
                let record = exchange.records().get(reference)?;
                (record.simple_name() == Some("LIMITS_AND_FITS")).then(
                    || -> Result<_, CodecError> {
                        Ok((
                            *reference,
                            LimitsAndFits {
                                form_variance: record
                                    .parameter(0)
                                    .map(|value| {
                                        decode_text_charged(
                                            exchange,
                                            value,
                                            &mut losses,
                                            *reference,
                                            "limits-and-fits form variance",
                                            StepLossCode::MetadataStringInvalid,
                                            ctx,
                                        )
                                    })
                                    .transpose()?
                                    .flatten()
                                    .unwrap_or_default(),
                                zone_variance: record
                                    .parameter(1)
                                    .map(|value| {
                                        decode_text_charged(
                                            exchange,
                                            value,
                                            &mut losses,
                                            *reference,
                                            "limits-and-fits zone variance",
                                            StepLossCode::MetadataStringInvalid,
                                            ctx,
                                        )
                                    })
                                    .transpose()?
                                    .flatten()
                                    .unwrap_or_default(),
                                grade: record
                                    .parameter(2)
                                    .map(|value| {
                                        decode_text_charged(
                                            exchange,
                                            value,
                                            &mut losses,
                                            *reference,
                                            "limits-and-fits grade",
                                            StepLossCode::MetadataStringInvalid,
                                            ctx,
                                        )
                                    })
                                    .transpose()?
                                    .flatten()
                                    .unwrap_or_default(),
                                source: record
                                    .parameter(3)
                                    .map(|value| {
                                        decode_text_charged(
                                            exchange,
                                            value,
                                            &mut losses,
                                            *reference,
                                            "limits-and-fits source",
                                            StepLossCode::MetadataStringInvalid,
                                            ctx,
                                        )
                                    })
                                    .transpose()?
                                    .flatten()
                                    .unwrap_or_default(),
                            },
                        ))
                    },
                )
            })
            .transpose()?;
        if let (Some(index), Some(limits)) = (dimension, limits) {
            let mut measurements = measure_context(geometry, id, &mut losses, graph_limit);
            let lower = limits
                .parameters()
                .first()
                .map(|value| measure(value, exchange, &mut measurements, ctx))
                .transpose()?
                .flatten();
            let upper = limits
                .parameters()
                .get(1)
                .map(|value| measure(value, exchange, &mut measurements, ctx))
                .transpose()?
                .flatten();
            if let (Some(lower), Some(upper)) = (lower, upper) {
                if set_dimension_tolerance(
                    &mut ir.model.pmi[index.get()].definition,
                    DimensionTolerance::PlusMinus { lower, upper },
                )
                .map_err(|error| {
                    CodecError::malformed(format_args!("PLUS_MINUS_TOLERANCE #{id}: {error}"))
                })? {
                    ctx.insert_btree_set(&mut typed, id, "step_pmi_typed_claims")?;
                    super::claim_records(ctx, &mut typed, refs, "step_pmi_typed_claims")?;
                } else {
                    ctx.push_vec(
                        &mut losses,
                        StepLossCode::DecodeWarning.note(format!(
                        "PLUS_MINUS_TOLERANCE #{id} is an additional tolerance for one dimension"
                    )),
                        "step_pmi_losses",
                    )?;
                }
            } else {
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::DecodeWarning.note(format!(
                        "PLUS_MINUS_TOLERANCE #{id} does not contain both deviation values"
                    )),
                    "step_pmi_losses",
                )?;
            }
        } else if let (Some(index), Some((fit_id, fit))) = (dimension, fit) {
            if set_dimension_tolerance(
                &mut ir.model.pmi[index.get()].definition,
                DimensionTolerance::Fit { fit },
            )
            .map_err(|error| {
                CodecError::malformed(format_args!("PLUS_MINUS_TOLERANCE #{id}: {error}"))
            })? {
                super::claim_records(ctx, &mut typed, [id, fit_id], "step_pmi_typed_claims")?;
            } else {
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::DecodeWarning.note(format!(
                        "PLUS_MINUS_TOLERANCE #{id} is an additional tolerance for one dimension"
                    )),
                    "step_pmi_losses",
                )?;
            }
        } else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "PLUS_MINUS_TOLERANCE #{id} has no resolvable dimension and limits"
                )),
                "step_pmi_losses",
            )?;
        }
    }

    for entity in exchange.matching_entity_ids(ctx, |name| tolerance_kind(Some(name)).is_some())? {
        let id = entity?;
        let Some(record) = exchange.records().get(&id) else {
            continue;
        };
        let Some(tolerance) = ctx
            .admit_iter(&record.partials[..], "STEP decode traversal")?
            .find_map(|partial| {
                (partial.name != "GEOMETRIC_TOLERANCE")
                    .then(|| tolerance_kind(Some(&partial.name)))
                    .flatten()
            })
            .map_or_else(
                || {
                    Ok::<_, CodecError>(
                        ctx.admit_iter(
                            &record.partials[..],
                            "STEP PMI fallback partial traversal",
                        )?
                        .find_map(|partial| tolerance_kind(Some(&partial.name))),
                    )
                },
                |value| Ok(Some(value)),
            )?
        else {
            continue;
        };
        let reference_values = ctx
            .admit_iter(&record.partials[..], "STEP decode traversal")?
            .find(|partial| partial.name == "GEOMETRIC_TOLERANCE")
            .map_or(record.parameters(), |partial| partial.parameters.as_slice());
        let refs = collect_pmi_references(
            reference_values,
            ctx,
            "step_pmi_geometric_tolerance_references",
        )?;
        let mut measurements = measure_context(geometry, id, &mut losses, graph_limit);
        let magnitude = first_measure(
            ctx.admit_iter(
                record
                    .partial(ctx, "GEOMETRIC_TOLERANCE")?
                    .map(|partial| partial.parameters.as_slice())
                    .unwrap_or_default(),
                "STEP tolerance measure parameter traversal",
            )?,
            exchange,
            &mut measurements,
            ctx,
        )?;
        let magnitude = match magnitude {
            Some(magnitude) => Some(magnitude),
            None => ctx
                .admit_iter(
                    &record.partials[..],
                    "STEP tolerance fallback partial traversal",
                )?
                .filter(|partial| partial.name != "GEOMETRIC_TOLERANCE")
                .map(|partial| {
                    first_measure(
                        ctx.admit_iter(
                            partial.parameters.as_slice(),
                            "STEP tolerance fallback parameter traversal",
                        )?,
                        exchange,
                        &mut measurements,
                        ctx,
                    )
                })
                .filter_map(Result::transpose)
                .next()
                .transpose()?,
        };
        let Some(magnitude) = magnitude.and_then(cadmpeg_ir::pmi::PmiMagnitude::new) else {
            let display_name = ctx.join_display_retained(
                record.partials.iter().map(|partial| partial.name.as_str()),
                "+",
                "step_record_display_name",
            )?;
            let message = ctx.format_retained(
                format_args!("{display_name} #{id} has no numeric magnitude"),
                "step_pmi_invalid_tolerance_text",
            )?;
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(message),
                "step_pmi_losses",
            )?;
            continue;
        };
        let defined_unit = ctx
            .admit_iter(&record.partials[..], "STEP decode traversal")?
            .find(|partial| partial.name == "GEOMETRIC_TOLERANCE_WITH_DEFINED_UNIT")
            .and_then(|partial| partial.parameters.first())
            .map(|value| measure(value, exchange, &mut measurements, ctx))
            .transpose()?
            .flatten();
        let (defined_area_unit, defined_area_second_unit) = if let Some(partial) = ctx
            .admit_iter(&record.partials[..], "STEP decode traversal")?
            .find(|partial| partial.name == "GEOMETRIC_TOLERANCE_WITH_DEFINED_AREA_UNIT")
        {
            let area = partial
                .parameters
                .first()
                .and_then(ValueExt::enumeration)
                .map(|name| {
                    let mut name =
                        ctx.copy_retained_text(name, "step_pmi_defined_area_unit_text")?;
                    name.make_ascii_lowercase();
                    Ok::<_, CodecError>(name)
                })
                .transpose()?;
            let second = partial
                .parameters
                .get(1)
                .map(|value| measure(value, exchange, &mut measurements, ctx))
                .transpose()?
                .flatten();
            (area, second)
        } else {
            (None, None)
        };
        // A complex tolerance keeps its base targets in GEOMETRIC_TOLERANCE,
        // while GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE carries the datum
        // system as a separate aggregate.
        let datum_system = first_matching(
            ctx.admit_iter(
                record
                    .partial(ctx, "GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE")?
                    .map(|partial| partial.parameters.as_slice())
                    .unwrap_or_default(),
                "STEP tolerance datum parameter traversal",
            )?,
            ctx,
            |id| {
                Ok({
                    annotations.get(id).is_some_and(|index| {
                        matches!(
                            ir.model.pmi[index.get()].definition,
                            PmiDefinition::DatumSystem { .. }
                        )
                    })
                })
            },
        )?
        .and_then(|id| {
            annotations
                .get(id)
                .map(|index| &ir.model.pmi[index.get()].id)
        })
        .map(|id| id.try_clone_for_decode(ctx, "step_pmi_datum_system_identity_copy"))
        .transpose()?;
        annotations.push(
            ctx,
            ir,
            id,
            AnnotationDraft {
                name: named_parameter(ctx, record, "GEOMETRIC_TOLERANCE", 0)?
                    .or_else(|| record.parameter(0))
                    .map(|value| {
                        decode_text_charged(
                            exchange,
                            value,
                            &mut losses,
                            id,
                            "geometric tolerance name",
                            StepLossCode::MetadataStringInvalid,
                            ctx,
                        )
                    })
                    .transpose()?
                    .flatten(),
                targets: targets(
                    refs.iter()
                        .copied()
                        .filter(|id| base_aspects.contains(id))
                        .map(Ok),
                    ctx,
                )?,
                visible: None,
                definition: PmiDefinition::GeometricTolerance {
                    tolerance,
                    magnitude,
                    defined_unit,
                    defined_area_unit,
                    defined_area_second_unit,
                    datum_system,
                    modifiers: tolerance_modifiers(record, ctx)?,
                },
            },
        )?;
        ctx.insert_btree_set(&mut typed, id, "step_pmi_typed_claims")?;
        for reference in ctx
            .admit_iter(
                refs.as_slice(),
                "STEP tolerance measure reference traversal",
            )?
            .copied()
        {
            if let Some(record) = exchange.records().get(&reference) {
                if is_measure_record(ctx, record)? {
                    ctx.insert_btree_set(&mut typed, reference, "step_pmi_typed_claims")?;
                }
            }
        }
        for partial in ctx.admit_iter(&record.partials[..], "STEP decode traversal")? {
            for value in ctx.admit_iter(
                partial.parameters.as_slice(),
                "STEP record parameter traversal",
            )? {
                for reference in references(value, ctx) {
                    let reference = reference?;
                    if let Some(record) = exchange.records().get(&reference) {
                        if is_measure_record(ctx, record)? {
                            ctx.insert_btree_set(&mut typed, reference, "step_pmi_typed_claims")?;
                        }
                    }
                }
            }
        }
    }

    for (id, record) in exchange.entities(ctx, "DRAUGHTING_MODEL_ITEM_ASSOCIATION")? {
        let Some(definition) =
            named_parameter(ctx, record, "DRAUGHTING_MODEL_ITEM_ASSOCIATION", 2)?
                .and_then(ValueExt::reference)
        else {
            continue;
        };
        if annotations.get(definition).is_some() {
            if let Some(items) =
                named_parameter(ctx, record, "DRAUGHTING_MODEL_ITEM_ASSOCIATION", 4)?
            {
                for item in references(items, ctx) {
                    let item = item?;
                    ctx.push_btree_group(
                        &mut presentation_semantics,
                        item,
                        definition,
                        "step_pmi_presentation_semantic_groups",
                        "step_pmi_presentation_semantic_members",
                    )?;
                }
            }
            ctx.insert_btree_set(&mut typed, id, "step_pmi_typed_claims")?;
        }
    }

    for entity in exchange.matching_entity_ids(ctx, is_presentation_annotation)? {
        let id = entity?;
        let Some(record) = exchange.records().get(&id) else {
            continue;
        };
        let Some(name) = presentation_annotation_name(ctx, record)? else {
            continue;
        };
        let mut text_records = BTreeSet::new();
        let text = find_annotation_text(
            id,
            exchange,
            &mut BTreeSet::new(),
            &mut text_records,
            &mut losses,
            0,
            ctx,
        )?;
        // Placement identity is the carrier key; the transform value cannot
        // make two source carriers one semantic carrier.
        let mut placement_candidates = BTreeMap::new();
        let mut placement_visited = BTreeMap::new();
        for partial in ctx.admit_iter(&record.partials[..], "STEP PMI record partial traversal")? {
            for parameter in ctx.admit_iter(
                partial.parameters.as_slice(),
                "STEP PMI record parameter traversal",
            )? {
                for reference in references(parameter, ctx) {
                    let reference = reference?;
                    collect_placement_candidates(
                        reference,
                        exchange,
                        geometry,
                        &mut placement_visited,
                        &mut placement_candidates,
                        0,
                        ctx,
                    )?;
                }
            }
        }
        let placement = match placement_candidates.len() {
            0 => None,
            1 => placement_candidates.values().next().copied(),
            count => {
                ctx.push_vec(&mut losses, StepLossCode::PresentationAnnotationPlacementAmbiguous.note(
                    format!(
                        "presentation annotation #{id} has {count} reachable placement carriers with no unique placement"
                    ),
                ), "step_pmi_losses")?;
                None
            }
        };
        let mut semantics = Vec::new();
        for partial in ctx.admit_iter(&record.partials[..], "STEP PMI record partial traversal")? {
            for parameter in ctx.admit_iter(
                partial.parameters.as_slice(),
                "STEP PMI record parameter traversal",
            )? {
                for reference in references(parameter, ctx) {
                    let reference = reference?;
                    if annotations.get(reference).is_some() {
                        ctx.push_vec(
                            &mut semantics,
                            pmi_id(reference),
                            "step_pmi_presentation_semantics",
                        )?;
                    }
                }
            }
        }
        if let Some(items) = presentation_semantics.get(&id) {
            for semantic in ctx.admit_iter(items, "STEP optional collection traversal")? {
                ctx.push_vec(
                    &mut semantics,
                    pmi_id(*semantic),
                    "step_pmi_presentation_semantics",
                )?;
            }
        }
        annotations.push(
            ctx,
            ir,
            id,
            AnnotationDraft {
                name: named_parameter(ctx, record, name, 0)?
                    .map_or_else(
                        || named_parameter(ctx, record, "REPRESENTATION_ITEM", 0),
                        |value| Ok(Some(value)),
                    )?
                    .or_else(|| record.parameter(0))
                    .map(|value| {
                        decode_text_charged(
                            exchange,
                            value,
                            &mut losses,
                            id,
                            "presentation annotation name",
                            StepLossCode::MetadataStringInvalid,
                            ctx,
                        )
                    })
                    .transpose()?
                    .flatten(),
                targets: Vec::new(),
                visible: hidden_presentation_annotations
                    .contains(&id)
                    .then_some(false),
                definition: PmiDefinition::Presentation {
                    text,
                    placement,
                    semantics,
                },
            },
        )?;
        ctx.insert_btree_set(&mut typed, id, "step_pmi_typed_claims")?;
        super::claim_records(ctx, &mut typed, text_records, "step_pmi_typed_claims")?;
    }
    for entity in exchange.entities_any(
        ctx,
        &["DRAUGHTING_MODEL", "ANNOTATION_PLANE", "DRAUGHTING_CALLOUT"],
    )? {
        let (id, _) = entity?;
        ctx.insert_btree_set(&mut typed, id, "step_pmi_typed_claims")?;
    }

    resolve_feature_for_datum_target_relationships(exchange, &annotations, ir, &mut typed, ctx)?;
    let points_by_source = point_sources(ir, ctx)?;
    let curves_by_source = curve_sources(ir, ctx)?;
    let geometry_sources = GeometrySources {
        points: &points_by_source,
        curves: &curves_by_source,
    };
    resolve_geometric_item_usages(
        exchange,
        topology,
        geometry_sources,
        (&shape_aspects, &annotations),
        ir,
        &mut typed,
        ctx,
    )?;

    let mut targeted_aspects = BTreeSet::new();
    for target in ir
        .model
        .pmi
        .iter()
        .flat_map(|annotation| &annotation.targets)
    {
        let id = match target {
            PmiTarget::ShapeAspect { source_id } => match source_id.as_str().strip_prefix('#') {
                Some(number) => ctx
                    .parse_text::<u64>(number, "STEP PMI targeted aspect number parse")?
                    .ok(),
                None => None,
            },
            _ => None,
        };
        if let Some(id) = id {
            ctx.insert_btree_set(&mut targeted_aspects, id, "step_pmi_targeted_aspects")?;
        }
    }
    for &id in ctx.admit_iter(&targeted_aspects, "step_pmi_typed_claims")? {
        if ctx.contains_btree_set(&shape_aspects, &id, "step_pmi_typed_claims")? {
            ctx.insert_btree_set(&mut typed, id, "step_pmi_typed_claims")?;
        }
    }
    mark_characteristic_representations(exchange, &annotations, &mut typed, ctx)?;
    Ok(StageOutcome {
        value: (),
        claims: typed,
        losses,
        notes: Vec::new(),
    })
}

fn set_dimension_tolerance(
    definition: &mut PmiDefinition,
    value: DimensionTolerance,
) -> Result<bool, String> {
    let PmiDefinition::Dimension(dimension) = definition else {
        return Ok(false);
    };
    dimension.merge_tolerance(value)
}

fn mark_characteristic_representations(
    exchange: &Exchange,
    annotations: &Annotations,
    typed: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    for (id, record) in exchange.entities(ctx, "DIMENSIONAL_CHARACTERISTIC_REPRESENTATION")? {
        let Some(_) = find_record_value(record, ctx, |value| {
            first_matching([value], ctx, |reference| {
                Ok(annotations.get(reference).is_some())
            })
        })?
        else {
            continue;
        };
        ctx.insert_btree_set(typed, id, "step_pmi_typed_claims")?;
        for partial in ctx.admit_iter(&record.partials[..], "STEP PMI record partial traversal")? {
            for parameter in ctx.admit_iter(
                partial.parameters.as_slice(),
                "STEP PMI record parameter traversal",
            )? {
                for representation_id in references(parameter, ctx) {
                    let representation_id = representation_id?;
                    let Some(representation) = exchange.records().get(&representation_id) else {
                        continue;
                    };
                    if !ctx
                        .admit_iter(
                            &(representation.partials)[..],
                            "STEP mark characteristic representations traversal",
                        )?
                        .any(|partial| partial.name == "SHAPE_DIMENSION_REPRESENTATION")
                    {
                        continue;
                    }
                    ctx.insert_btree_set(typed, representation_id, "step_pmi_typed_claims")?;
                    for partial in ctx.admit_iter(
                        &representation.partials[..],
                        "STEP PMI record partial traversal",
                    )? {
                        for parameter in ctx.admit_iter(
                            partial.parameters.as_slice(),
                            "STEP PMI record parameter traversal",
                        )? {
                            for reference in references(parameter, ctx) {
                                let reference = reference?;
                                if let Some(record) = exchange.records().get(&reference) {
                                    if is_measure_record(ctx, record)? {
                                        ctx.insert_btree_set(
                                            typed,
                                            reference,
                                            "step_pmi_typed_claims",
                                        )?;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn resolve_feature_for_datum_target_relationships(
    exchange: &Exchange,
    annotations: &Annotations,
    ir: &mut CadIr,
    typed: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    for (id, record) in exchange.entities(ctx, "FEATURE_FOR_DATUM_TARGET_RELATIONSHIP")? {
        let Some((relating, related)) = relationship_endpoints(record, ctx)? else {
            continue;
        };
        let Some(annotation_index) = annotations.get(related) else {
            continue;
        };
        let annotation = &mut ir.model.pmi[annotation_index.get()];
        let PmiDefinition::DatumTarget { basis, .. } = &mut annotation.definition else {
            continue;
        };
        push_target(
            basis,
            PmiTarget::ShapeAspect {
                source_id: super::step_source_id(ctx, relating)?,
            },
            ctx,
            "step_pmi_datum_basis_targets",
        )?;
        super::claim_records(ctx, typed, [id, relating], "step_pmi_typed_claims")?;
    }
    Ok(())
}

fn resolve_geometric_item_usages(
    exchange: &Exchange,
    topology: &TopologyData,
    geometry_sources: GeometrySources<'_>,
    (shape_aspects, annotations): (&BTreeSet<u64>, &Annotations),
    ir: &mut CadIr,
    typed: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let mut aspect_annotations = BTreeMap::<u64, BTreeSet<AnnotationIndex>>::new();
    for (&annotation_id, record) in ctx.admit_iter(
        exchange.records(),
        "STEP resolve geometric item usages traversal",
    )? {
        let Some(annotation_index) = annotations.get(annotation_id) else {
            continue;
        };
        if shape_aspects.contains(&annotation_id) {
            ctx.insert_btree_group_set(
                &mut aspect_annotations,
                annotation_id,
                annotation_index,
                "step_pmi_aspect_annotation_groups",
                "step_pmi_aspect_annotation_members",
            )?;
        }
        for partial in ctx.admit_iter(&record.partials[..], "STEP PMI record partial traversal")? {
            for parameter in ctx.admit_iter(
                partial.parameters.as_slice(),
                "STEP PMI record parameter traversal",
            )? {
                for reference in references(parameter, ctx) {
                    let reference = reference?;
                    if shape_aspects.contains(&reference) {
                        ctx.insert_btree_group_set(
                            &mut aspect_annotations,
                            reference,
                            annotation_index,
                            "step_pmi_aspect_annotation_groups",
                            "step_pmi_aspect_annotation_members",
                        )?;
                    }
                }
            }
        }
    }

    let mut relationship_aspects = BTreeMap::<u64, BTreeSet<u64>>::new();
    for record in ctx
        .admit_iter(
            exchange.records(),
            "STEP resolve geometric item usages map traversal",
        )?
        .map(|(_, value)| value)
    {
        let Some((relating, related)) = relationship_endpoints(record, ctx)? else {
            continue;
        };
        ctx.insert_btree_group_set(
            &mut relationship_aspects,
            relating,
            related,
            "step_pmi_relationship_aspect_groups",
            "step_pmi_relationship_aspect_members",
        )?;
        ctx.insert_btree_group_set(
            &mut relationship_aspects,
            related,
            relating,
            "step_pmi_relationship_aspect_groups",
            "step_pmi_relationship_aspect_members",
        )?;
    }

    for (&id, record) in ctx.admit_iter(
        exchange.records(),
        "STEP resolve geometric item usages traversal",
    )? {
        let Some(partial) = ctx
            .admit_iter(
                &(record.partials)[..],
                "STEP resolve geometric item usages traversal",
            )?
            .find(|partial| partial.name == "GEOMETRIC_ITEM_SPECIFIC_USAGE")
        else {
            continue;
        };
        let Some(definition) = first_matching(partial.parameters.get(2), ctx, |_| Ok(true))? else {
            continue;
        };
        let Some(identified_item) = first_matching(partial.parameters.get(4), ctx, |_| Ok(true))?
        else {
            continue;
        };
        let mut annotation_indices = BTreeSet::new();
        if let Some(items) = aspect_annotations.get(&definition) {
            for &index in ctx.admit_iter(items, "STEP optional collection traversal")? {
                ctx.insert_btree_set(
                    &mut annotation_indices,
                    index,
                    "step_pmi_usage_annotation_indices",
                )?;
            }
        }
        if let Some(aspects) = relationship_aspects.get(&definition) {
            for aspect in aspects {
                if let Some(items) = aspect_annotations.get(aspect) {
                    for &index in ctx.admit_iter(items, "STEP optional collection traversal")? {
                        ctx.insert_btree_set(
                            &mut annotation_indices,
                            index,
                            "step_pmi_usage_annotation_indices",
                        )?;
                    }
                }
            }
        }
        if annotation_indices.is_empty() {
            continue;
        }
        let targets = topology_targets(identified_item, topology, geometry_sources, ctx)?;
        if targets.is_empty() {
            continue;
        }
        for annotation_index in annotation_indices {
            let annotation = &mut ir.model.pmi[annotation_index.get()];
            for target in ctx.admit_iter(
                &(targets)[..],
                "STEP resolve geometric item usages traversal",
            )? {
                push_target(
                    &mut annotation.targets,
                    copy_pmi_target(target, ctx, "step_pmi_geometric_usage_identity")?,
                    ctx,
                    "step_pmi_geometric_usage_targets",
                )?;
            }
        }
        ctx.insert_btree_set(typed, id, "step_pmi_typed_claims")?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct GeometrySources<'a> {
    points: &'a BTreeMap<u64, Vec<cadmpeg_ir::ids::PointId>>,
    curves: &'a BTreeMap<u64, Vec<cadmpeg_ir::ids::CurveId>>,
}

fn topology_targets(
    id: u64,
    topology: &TopologyData,
    geometry_sources: GeometrySources<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<PmiTarget>, CodecError> {
    let mut targets = Vec::new();
    if let Some(items) = topology.body_by_root.get(&id) {
        for body in ctx.admit_iter(items, "STEP optional collection traversal")? {
            let body = body.try_clone_for_decode(ctx, "step_pmi_topology_identity")?;
            push_target(
                &mut targets,
                PmiTarget::Body { body },
                ctx,
                "step_pmi_topology_targets",
            )?;
        }
    }
    if let Some(items) = topology.faces_by_source.get(&id) {
        for face in ctx.admit_iter(items, "STEP optional collection traversal")? {
            let face = face.try_clone_for_decode(ctx, "step_pmi_topology_identity")?;
            push_target(
                &mut targets,
                PmiTarget::Face { face },
                ctx,
                "step_pmi_topology_targets",
            )?;
        }
    }
    if let Some(items) = topology.edges_by_source.get(&id) {
        for edge in ctx.admit_iter(items, "STEP optional collection traversal")? {
            let edge = edge.try_clone_for_decode(ctx, "step_pmi_topology_identity")?;
            push_target(
                &mut targets,
                PmiTarget::Edge { edge },
                ctx,
                "step_pmi_topology_targets",
            )?;
        }
    }
    if let Some(items) = topology.vertices_by_source.get(&id) {
        for vertex in ctx.admit_iter(items, "STEP optional collection traversal")? {
            let vertex = vertex.try_clone_for_decode(ctx, "step_pmi_topology_identity")?;
            push_target(
                &mut targets,
                PmiTarget::Vertex { vertex },
                ctx,
                "step_pmi_topology_targets",
            )?;
        }
    }
    if let Some(items) = geometry_sources.points.get(&id) {
        for point in ctx.admit_iter(items, "STEP optional collection traversal")? {
            let point = point.try_clone_for_decode(ctx, "step_pmi_topology_identity")?;
            push_target(
                &mut targets,
                PmiTarget::Point { point },
                ctx,
                "step_pmi_topology_targets",
            )?;
        }
    }
    if let Some(items) = geometry_sources.curves.get(&id) {
        for curve in ctx.admit_iter(items, "STEP optional collection traversal")? {
            let curve = curve.try_clone_for_decode(ctx, "step_pmi_topology_identity")?;
            push_target(
                &mut targets,
                PmiTarget::Curve { curve },
                ctx,
                "step_pmi_topology_targets",
            )?;
        }
    }
    Ok(targets)
}

fn push_target(
    targets: &mut Vec<PmiTarget>,
    target: PmiTarget,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<(), CodecError> {
    if !targets.contains(&target) {
        ctx.push_vec(targets, target, operation)?;
    }
    Ok(())
}

fn relationship_endpoints(
    record: &RawRecord,
    ctx: &DecodeContext<'_>,
) -> Result<Option<(u64, u64)>, CodecError> {
    let Some(parameters) = ctx
        .admit_iter(
            &(record.partials)[..],
            "STEP relationship endpoints traversal",
        )?
        .find_map(|partial| {
            matches!(
                partial.name.as_str(),
                "SHAPE_ASPECT_RELATIONSHIP" | "FEATURE_FOR_DATUM_TARGET_RELATIONSHIP"
            )
            .then_some(partial.parameters.as_slice())
        })
    else {
        return Ok(None);
    };
    let Some(relating) = first_matching(parameters.get(2), ctx, |_| Ok(true))? else {
        return Ok(None);
    };
    let Some(related) = first_matching(parameters.get(3), ctx, |_| Ok(true))? else {
        return Ok(None);
    };
    Ok(Some((relating, related)))
}

fn point_sources(
    ir: &CadIr,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<u64, Vec<cadmpeg_ir::ids::PointId>>, CodecError> {
    let mut points = BTreeMap::new();
    for point in ctx.admit_iter(&ir.model.points[..], "STEP point sources traversal")? {
        let Some(source) = source_numeric_id(ctx, point.id.as_str(), "point")? else {
            continue;
        };
        let id = point
            .id
            .try_clone_for_decode(ctx, "step_pmi_point_source_identity")?;
        ctx.push_btree_group(
            &mut points,
            source,
            id,
            "step_pmi_point_source_groups",
            "step_pmi_point_source_items",
        )?;
    }
    Ok(points)
}

fn curve_sources(
    ir: &CadIr,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<u64, Vec<cadmpeg_ir::ids::CurveId>>, CodecError> {
    let mut curves = BTreeMap::new();
    for curve in ctx.admit_iter(&ir.model.curves[..], "STEP curve sources traversal")? {
        let Some(source) = source_numeric_id(ctx, curve.id.as_str(), "curve")? else {
            continue;
        };
        let id = curve
            .id
            .try_clone_for_decode(ctx, "step_pmi_curve_source_identity")?;
        ctx.push_btree_group(
            &mut curves,
            source,
            id,
            "step_pmi_curve_source_groups",
            "step_pmi_curve_source_items",
        )?;
    }
    Ok(curves)
}

fn datum_references_for_compartment(
    value: &Value,
    precedence: NonZeroU32,
    exchange: &Exchange,
    annotations: &Annotations,
    typed: &mut BTreeSet<u64>,
    measurements: &mut MeasureContext<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<DatumReference>, CodecError> {
    let Some(compartment_id) = value.reference() else {
        return Ok(Vec::new());
    };
    let Some(compartment) = exchange.records().get(&compartment_id) else {
        return Ok(Vec::new());
    };
    if compartment
        .partial(ctx, "DATUM_REFERENCE_COMPARTMENT")?
        .is_none()
        && compartment
            .partial(ctx, "DATUM_REFERENCE_ELEMENT")?
            .is_none()
    {
        return Ok(Vec::new());
    }
    ctx.insert_btree_set(typed, compartment_id, "step_pmi_typed_claims")?;
    let mut compartment_modifiers = Vec::new();
    for modifier in ctx.admit_iter(
        datum_modifiers(ctx, compartment)?
            .and_then(ValueExt::list)
            .unwrap_or_default(),
        "STEP datum modifier traversal",
    )? {
        if let Some(text) = modifier_text(modifier, exchange, typed, measurements, ctx)? {
            ctx.push_vec(
                &mut compartment_modifiers,
                text,
                "step_pmi_datum_modifier_items",
            )?;
        }
    }
    let base = datum_base(ctx, compartment)?;
    let mut output = Vec::new();
    if is_common_datum_list(base) {
        let Some(Value::Typed(_, members)) = base else {
            return Ok(output);
        };
        let members = members.list().unwrap_or_default();
        let common_group = (ctx
            .admit_iter(
                &(members)[..],
                "STEP datum references for compartment traversal",
            )?
            .filter_map(ValueExt::reference)
            .count()
            >= 2)
            .then_some(precedence.get());
        for element_id in ctx
            .admit_iter(
                &(members)[..],
                "STEP datum references for compartment traversal",
            )?
            .filter_map(ValueExt::reference)
        {
            let Some(element) = exchange.records().get(&element_id) else {
                continue;
            };
            if element.partial(ctx, "DATUM_REFERENCE_ELEMENT")?.is_none() {
                continue;
            }
            let Some(datum) = datum_base(ctx, element)?.and_then(ValueExt::reference) else {
                continue;
            };
            if annotations.get(datum).is_none() {
                continue;
            }
            let mut modifiers = ctx.try_collect_vec(
                compartment_modifiers
                    .iter()
                    .map(|value| ctx.copy_retained_text(value, "step_pmi_datum_modifier_copy")),
                "step_pmi_datum_modifier_items",
            )?;
            for modifier in ctx.admit_iter(
                datum_modifiers(ctx, element)?
                    .and_then(ValueExt::list)
                    .unwrap_or_default(),
                "STEP datum modifier traversal",
            )? {
                if let Some(text) = modifier_text(modifier, exchange, typed, measurements, ctx)? {
                    ctx.push_vec(&mut modifiers, text, "step_pmi_datum_modifier_items")?;
                }
            }
            super::claim_records(ctx, typed, [element_id, datum], "step_pmi_typed_claims")?;
            ctx.push_vec(
                &mut output,
                DatumReference {
                    datum: pmi_id(datum),
                    precedence,
                    common_group,
                    modifiers,
                },
                "step_pmi_datum_reference_items",
            )?;
        }
        return Ok(output);
    }
    if let Some(base) = base {
        visit_datum_ids(base, ctx, &mut |datum| {
            if annotations.get(datum).is_none() {
                return Ok(());
            }
            ctx.insert_btree_set(typed, datum, "step_pmi_typed_claims")?;
            ctx.push_vec(
                &mut output,
                DatumReference {
                    datum: pmi_id(datum),
                    precedence,
                    common_group: None,
                    modifiers: ctx.try_collect_vec(
                        compartment_modifiers.iter().map(|value| {
                            ctx.copy_retained_text(value, "step_pmi_datum_modifier_copy")
                        }),
                        "step_pmi_datum_modifier_items",
                    )?,
                },
                "step_pmi_datum_reference_items",
            )
        })?;
    }
    Ok(output)
}

fn admit_datum_reference_maps(
    references: &[DatumReference],
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    for (index, reference) in ctx
        .admit_iter(
            &(references)[..],
            "STEP admit datum reference maps traversal",
        )?
        .enumerate()
    {
        let prior = &references[..index];
        if !ctx
            .admit_iter(&prior[..], "STEP admit datum reference maps traversal")?
            .map(|other| -> Result<Option<_>, CodecError> {
                Ok((ctx.equal(
                    &other.precedence,
                    &reference.precedence,
                    "STEP admit datum reference maps equality",
                )?)
                .then_some(()))
            })
            .find_map(Result::transpose)
            .transpose()?
            .is_some()
        {
            ctx.charge_collection_items(1, "step_pmi_datum_compartments")?;
        }
        if let Some(group) = reference.common_group {
            if !ctx
                .admit_iter(&prior[..], "STEP admit datum reference maps traversal")?
                .any(|other| other.common_group == Some(group))
            {
                ctx.charge_collection_items(1, "step_pmi_datum_common_groups")?;
            }
        }
    }
    Ok(())
}

fn datum_base<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
) -> Result<Option<&'a Value>, CodecError> {
    Ok(ctx
        .admit_iter(&record.partials[..], "STEP datum base traversal")?
        .find(|partial| partial.name == "GENERAL_DATUM_REFERENCE")
        .and_then(|partial| partial.parameters.first())
        .or_else(|| record.parameter(4)))
}

fn datum_modifiers<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
) -> Result<Option<&'a Value>, CodecError> {
    Ok(ctx
        .admit_iter(&record.partials[..], "STEP datum modifiers traversal")?
        .find(|partial| partial.name == "GENERAL_DATUM_REFERENCE")
        .and_then(|partial| partial.parameters.get(1))
        .or_else(|| record.parameter(5)))
}

fn is_common_datum_list(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Typed(kind, _)) if kind == "COMMON_DATUM_LIST")
}

fn visit_datum_ids(
    value: &Value,
    ctx: &DecodeContext<'_>,
    visitor: &mut impl FnMut(u64) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    let _nested = ctx.enter_nested("step_pmi_datum_id_walk")?;
    match value {
        Value::Reference(id) => visitor(*id)?,
        Value::List(values) => {
            for value in
                ctx.admit_iter(values.as_slice(), "STEP visit datum ids value traversal")?
            {
                visit_datum_ids(value, ctx, visitor)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn modifier_text(
    value: &Value,
    exchange: &Exchange,
    typed: &mut BTreeSet<u64>,
    measurements: &mut MeasureContext<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<String>, CodecError> {
    let _nested = ctx.enter_nested("step_pmi_datum_modifier_walk")?;
    match value {
        Value::Enumeration(value) => {
            let mut text = ctx.copy_retained_text(value, "step_pmi_datum_modifier_text")?;
            text.make_ascii_lowercase();
            Ok(Some(text))
        }
        Value::Typed(_, value) => modifier_text(value, exchange, typed, measurements, ctx),
        Value::Reference(id) => {
            let Some(record) = exchange.records().get(id) else {
                return Ok(None);
            };
            let Some(parameters) = ctx
                .admit_iter(&record.partials[..], "STEP modifier text traversal")?
                .find(|partial| partial.name == "DATUM_REFERENCE_MODIFIER_WITH_VALUE")
            else {
                return Ok(None);
            };
            let parameters = parameters.parameters.as_slice();
            ctx.insert_btree_set(typed, *id, "step_pmi_typed_claims")?;
            let Some(kind) = parameters.first().and_then(ValueExt::enumeration) else {
                return Ok(None);
            };
            let Some(measure_id) = parameters.get(1).and_then(ValueExt::reference) else {
                return Ok(None);
            };
            let Some(value) = measure(&Value::Reference(measure_id), exchange, measurements, ctx)?
            else {
                return Ok(None);
            };
            let value = value.value.get();
            ctx.insert_btree_set(typed, measure_id, "step_pmi_typed_claims")?;
            let mut text = ctx.format_retained(
                format_args!("{kind}:{value}"),
                "step_pmi_datum_modifier_value_text",
            )?;
            text.make_ascii_lowercase();
            Ok(Some(text))
        }
        _ => Ok(None),
    }
}

pub(super) fn is_presentation_annotation(name: &str) -> bool {
    name.starts_with("ANNOTATION_")
        && (name.ends_with("_OCCURRENCE") || name.ends_with("_OCCURRENCE_WITH_LEADER_LINE"))
        || matches!(
            name,
            "TESSELLATED_ANNOTATION_OCCURRENCE"
                | "LEADER_CURVE"
                | "LEADER_DIRECTED_CALLOUT"
                | "LEADER_DIRECTED_DIMENSION"
        )
}

fn presentation_annotation_name<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
) -> Result<Option<&'a str>, CodecError> {
    Ok(ctx
        .admit_iter(
            &record.partials[..],
            "STEP presentation annotation name traversal",
        )?
        .find_map(|partial| {
            is_presentation_annotation(&partial.name).then_some(partial.name.as_str())
        }))
}

pub(super) fn is_supported_invisibility_target(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
) -> Result<bool, CodecError> {
    Ok(presentation_annotation_name(ctx, record)?.is_some())
}

fn hidden_presentation_annotation_ids(
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeSet<u64>, CodecError> {
    let mut hidden = BTreeSet::new();
    for record in ctx
        .admit_iter(
            exchange.records(),
            "STEP hidden presentation annotation ids map traversal",
        )?
        .map(|(_, value)| value)
    {
        let Some(items) = ctx
            .admit_iter(
                &(record.partials)[..],
                "STEP hidden presentation annotation ids traversal",
            )?
            .find(|partial| partial.name == "INVISIBILITY")
            .and_then(|partial| partial.parameters.first())
        else {
            continue;
        };
        for target in references(items, ctx) {
            let target = target?;
            if let Some(record) = exchange.records().get(&target) {
                if is_supported_invisibility_target(ctx, record)? {
                    ctx.insert_btree_set(&mut hidden, target, "step_pmi_hidden_annotation_ids")?;
                }
            }
        }
    }
    Ok(hidden)
}

fn collect_typed_placement_candidates(
    record: &RawRecord,
    geometry: &GeometryData,
    candidates: &mut BTreeMap<u64, Transform>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let has_annotation_text = ctx
        .admit_iter(
            &(record.partials)[..],
            "STEP collect typed placement candidates traversal",
        )?
        .any(|partial| {
            partial.name == "ANNOTATION_TEXT"
                || partial.name == "ANNOTATION_TEXT_CHARACTER"
                || partial.name.starts_with("ANNOTATION_TEXT_WITH_")
        });
    for partial in ctx.admit_iter(
        &(record.partials)[..],
        "STEP collect typed placement candidates traversal",
    )? {
        let is_carrier = match partial.name.as_str() {
            "DEFINED_CHARACTER_GLYPH"
            | "SYMBOL_TARGET"
            | "TEXT_LITERAL"
            | "DRAUGHTING_TEXT_LITERAL_WITH_DELINEATION" => true,
            name if name.starts_with("TEXT_LITERAL_WITH_") => true,
            "MAPPED_ITEM" | "ANNOTATION_TEXT" | "ANNOTATION_TEXT_CHARACTER" => has_annotation_text,
            name if has_annotation_text && name.starts_with("ANNOTATION_TEXT_WITH_") => true,
            _ => false,
        };
        if !is_carrier {
            continue;
        }
        for reference in ctx
            .admit_iter(
                &(partial.parameters)[..],
                "STEP collect typed placement candidates traversal",
            )?
            .flat_map(|value| references(value, ctx))
        {
            let reference = reference?;
            if let Some(&(origin, z_axis, x_axis)) = geometry.placements.get(&reference) {
                if let Some(transform) =
                    super::geometry::placement_transform((origin, z_axis, x_axis))
                {
                    ctx.insert_btree_map(
                        candidates,
                        reference,
                        transform,
                        "step_pmi_placement_candidates",
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn find_annotation_text(
    id: u64,
    exchange: &Exchange,
    visited: &mut BTreeSet<u64>,
    used: &mut BTreeSet<u64>,
    losses: &mut Vec<LossNote>,
    depth: usize,
    ctx: &DecodeContext<'_>,
) -> Result<Option<String>, CodecError> {
    let mut candidates = BTreeMap::new();
    collect_annotation_text(id, exchange, visited, &mut candidates, losses, depth, ctx)?;
    let Some((text_id, text)) = candidates.pop_first() else {
        return Ok(None);
    };
    if candidates.is_empty() {
        ctx.insert_btree_set(used, text_id, "step_pmi_annotation_text_used")?;
        Ok(Some(text))
    } else {
        let count = candidates.len() + 1;
        ctx.push_vec(losses, StepLossCode::PresentationAnnotationTextUnordered.note(format!(
                    "presentation annotation #{id} has {count} reachable text carriers with no ordered composition"
                )), "step_pmi_losses")?;
        Ok(None)
    }
}

fn collect_annotation_text(
    id: u64,
    exchange: &Exchange,
    visited: &mut BTreeSet<u64>,
    candidates: &mut BTreeMap<u64, String>,
    losses: &mut Vec<LossNote>,
    depth: usize,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    if depth >= 256 || visited.contains(&id) {
        return Ok(());
    }
    let _depth_guard = ctx.enter_nested("step_pmi_annotation_text_walk")?;
    ctx.insert_btree_set(visited, id, "step_pmi_annotation_text_visited")?;
    let Some(record) = exchange.records().get(&id) else {
        return Ok(());
    };
    if let Some(value) = named_parameter(ctx, record, "TEXT_LITERAL", 0)?.map_or_else(
        || named_parameter(ctx, record, "TEXT_LITERAL_WITH_ASSOCIATED_CURVES", 0),
        |value| Ok(Some(value)),
    )? {
        if let Some(text) = decode_text_charged(
            exchange,
            value,
            losses,
            id,
            "PMI annotation text",
            StepLossCode::MetadataStringInvalid,
            ctx,
        )? {
            ctx.insert_btree_map(candidates, id, text, "step_pmi_annotation_text_candidates")?;
        }
    }
    for partial in ctx.admit_iter(&record.partials[..], "STEP PMI record partial traversal")? {
        for value in ctx.admit_iter(
            partial.parameters.as_slice(),
            "STEP PMI record parameter traversal",
        )? {
            for reference in references(value, ctx) {
                let reference = reference?;
                collect_annotation_text(
                    reference,
                    exchange,
                    visited,
                    candidates,
                    losses,
                    depth + 1,
                    ctx,
                )?;
            }
        }
    }
    Ok(())
}

fn collect_placement_candidates(
    id: u64,
    exchange: &Exchange,
    geometry: &GeometryData,
    visited: &mut BTreeMap<u64, usize>,
    candidates: &mut BTreeMap<u64, Transform>,
    depth: usize,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    // Retain the shortest depth so the bounded traversal is independent of
    // aggregate member order when a graph has alternate paths.
    if depth >= 256
        || visited
            .get(&id)
            .is_some_and(|visited_depth| *visited_depth <= depth)
    {
        return Ok(());
    }
    let _nested = ctx.enter_nested("step_pmi_placement_walk")?;
    ctx.insert_btree_map(visited, id, depth, "step_pmi_placement_visited")?;
    if let Some(record) = exchange.records().get(&id) {
        collect_typed_placement_candidates(record, geometry, candidates, ctx)?;
    }
    let Some(record) = exchange.records().get(&id) else {
        return Ok(());
    };
    for partial in ctx.admit_iter(&record.partials[..], "STEP PMI record partial traversal")? {
        for value in ctx.admit_iter(
            partial.parameters.as_slice(),
            "STEP PMI record parameter traversal",
        )? {
            for reference in references(value, ctx) {
                let reference = reference?;
                collect_placement_candidates(
                    reference,
                    exchange,
                    geometry,
                    visited,
                    candidates,
                    depth + 1,
                    ctx,
                )?;
            }
        }
    }
    Ok(())
}

fn targets(
    ids: impl IntoIterator<Item = Result<u64, CodecError>>,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<PmiTarget>, CodecError> {
    let mut seen = BTreeSet::new();
    let mut targets = Vec::new();
    for id in ids {
        let id = id?;
        if seen.contains(&id) {
            continue;
        }
        ctx.insert_btree_set(&mut seen, id, "step_pmi_target_ids")?;

        ctx.reserve_vec(&mut targets, 1, "step_pmi_target_items")?;
        targets.push(PmiTarget::ShapeAspect {
            source_id: super::step_source_id(ctx, id)?,
        });
    }
    Ok(targets)
}

fn pmi_id(id: u64) -> PmiId {
    PmiId::from(ids::presentation(kind!("pmi"), id))
}

fn copy_pmi_target(
    target: &PmiTarget,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<PmiTarget, CodecError> {
    Ok(match target {
        PmiTarget::Body { body } => PmiTarget::Body {
            body: body.try_clone_for_decode(ctx, operation)?,
        },
        PmiTarget::Face { face } => PmiTarget::Face {
            face: face.try_clone_for_decode(ctx, operation)?,
        },
        PmiTarget::Edge { edge } => PmiTarget::Edge {
            edge: edge.try_clone_for_decode(ctx, operation)?,
        },
        PmiTarget::Vertex { vertex } => PmiTarget::Vertex {
            vertex: vertex.try_clone_for_decode(ctx, operation)?,
        },
        PmiTarget::Point { point } => PmiTarget::Point {
            point: point.try_clone_for_decode(ctx, operation)?,
        },
        PmiTarget::Curve { curve } => PmiTarget::Curve {
            curve: curve.try_clone_for_decode(ctx, operation)?,
        },
        PmiTarget::Product { product } => PmiTarget::Product {
            product: product.try_clone_for_decode(ctx, operation)?,
        },
        PmiTarget::Occurrence { occurrence } => PmiTarget::Occurrence {
            occurrence: occurrence.try_clone_for_decode(ctx, operation)?,
        },
        PmiTarget::ShapeAspect { source_id } => {
            let copy = ctx.copy_retained_text(source_id.as_str(), operation)?;
            let source_id = cadmpeg_core::text::NonBlankString::for_decode(
                ctx,
                copy,
                "validate nonblank text",
            )?
            .ok_or_else(|| CodecError::malformed("STEP PMI target has a blank source ID"))?;
            PmiTarget::ShapeAspect { source_id }
        }
    })
}

fn datum_target_form(value: &str, ctx: &DecodeContext<'_>) -> Result<DatumTargetForm, CodecError> {
    let form = ctx.trim_text(value, "STEP datum target form trim")?;
    if ctx.eq_ignore_ascii_case(form, "point", "STEP datum target form case equality")? {
        Ok(DatumTargetForm::Point)
    } else if ctx.eq_ignore_ascii_case(form, "line", "STEP datum target form case equality")? {
        Ok(DatumTargetForm::Line)
    } else if ctx.eq_ignore_ascii_case(form, "rectangle", "STEP datum target form case equality")? {
        Ok(DatumTargetForm::Rectangle)
    } else if ctx.eq_ignore_ascii_case(form, "circle", "STEP datum target form case equality")? {
        Ok(DatumTargetForm::Circle)
    } else if ctx.eq_ignore_ascii_case(
        form,
        "circular curve",
        "STEP datum target form case equality",
    )? {
        Ok(DatumTargetForm::CircularCurve)
    } else {
        Ok(DatumTargetForm::Other(ctx.copy_retained_text(
            value,
            "step_pmi_datum_target_form_copy",
        )?))
    }
}

fn datum_target_identification_parameter<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
) -> Result<Option<&'a Value>, CodecError> {
    Ok(ctx
        .admit_iter(
            &record.partials[..],
            "STEP datum target identification parameter traversal",
        )?
        .find(|partial| partial.name == "DATUM_TARGET")
        .and_then(|partial| partial.parameters.last())
        .or_else(|| record.parameter(4)))
}

fn is_datum_target_name(name: &str) -> bool {
    matches!(name, "DATUM_TARGET" | "PLACED_DATUM_TARGET_FEATURE")
}

fn is_pmi_entity_name(name: &str) -> bool {
    matches!(
        name,
        "SHAPE_ASPECT"
            | "DATUM_FEATURE"
            | "DATUM"
            | "DATUM_SYSTEM"
            | "PLUS_MINUS_TOLERANCE"
            | "DRAUGHTING_MODEL_ITEM_ASSOCIATION"
            | "DIMENSIONAL_CHARACTERISTIC_REPRESENTATION"
            | "DRAUGHTING_MODEL"
            | "ANNOTATION_PLANE"
            | "DRAUGHTING_CALLOUT"
            | "FEATURE_FOR_DATUM_TARGET_RELATIONSHIP"
            | "GEOMETRIC_ITEM_SPECIFIC_USAGE"
    ) || is_dimension_name(name)
        || tolerance_kind(Some(name)).is_some()
        || is_datum_target_name(name)
        || is_presentation_annotation(name)
}

// Simple STEP instances contain only their most-derived entity name. The
// parser does not load an EXPRESS inheritance graph, so keep the supported
// shape_aspect lineage here for leaf names that do not identify that lineage
// by the `_SHAPE_ASPECT` suffix.
const SHAPE_ASPECT_SUBTYPE_NAMES: &[&str] = &[
    "APEX",
    "APPLIED_AREA",
    "ASSEMBLY_BOND_DEFINITION",
    "ASSEMBLY_JOINT",
    "ASSEMBLY_SHAPE_CONSTRAINT",
    "ASSEMBLY_SHAPE_JOINT",
    "BASIC_ROUND_HOLE_OCCURRENCE",
    "BASIC_ROUND_HOLE_OCCURRENCE_IN_ASSEMBLY",
    "BEAD_END",
    "BOSS_TOP",
    "CENTRE_OF_SYMMETRY",
    "CHAMFER",
    "CHAMFER_OFFSET",
    "CIRCULAR_CLOSED_PROFILE",
    "CLOSED_PATH_PROFILE",
    "COMMON_DATUM",
    "COMPONENT_FEATURE",
    "COMPONENT_FEATURE_JOINT",
    "COMPONENT_MATING_CONSTRAINT_CONDITION",
    "COMPONENT_TERMINAL",
    "CONNECTION_ZONE_BASED_ASSEMBLY_JOINT",
    "CONNECTION_ZONE_INTERFACE_PLANE_RELATIONSHIP",
    "CONNECTIVITY_DEFINITION",
    "CONTACTING_FEATURE",
    "CONTACT_FEATURE",
    "COUNTERBORE_HOLE_OCCURRENCE",
    "COUNTERBORE_HOLE_OCCURRENCE_IN_ASSEMBLY",
    "COUNTERDRILL_HOLE_OCCURRENCE",
    "COUNTERDRILL_HOLE_OCCURRENCE_IN_ASSEMBLY",
    "COUNTERSINK_HOLE_OCCURRENCE",
    "COUNTERSINK_HOLE_OCCURRENCE_IN_ASSEMBLY",
    "CROSS_SECTIONAL_ALTERNATIVE_SHAPE_ELEMENT",
    "CROSS_SECTIONAL_GROUP_SHAPE_ELEMENT",
    "CROSS_SECTIONAL_GROUP_SHAPE_ELEMENT_WITH_LACING",
    "CROSS_SECTIONAL_GROUP_SHAPE_ELEMENT_WITH_TUBULAR_COVER",
    "CROSS_SECTIONAL_OCCURRENCE_SHAPE_ELEMENT",
    "CROSS_SECTIONAL_PART_SHAPE_ELEMENT",
    "DATUM",
    "DATUM_FEATURE",
    "DATUM_REFERENCE_COMPARTMENT",
    "DATUM_REFERENCE_ELEMENT",
    "DATUM_SYSTEM",
    "DATUM_SYSTEM_FOR_COMPOSITE_GROUP_ELEMENT",
    "DATUM_TARGET",
    "DEFAULT_MODEL_GEOMETRIC_VIEW",
    "DIMENSIONAL_LOCATION_WITH_DATUM_FEATURE",
    "DIMENSIONAL_SIZE_WITH_DATUM_FEATURE",
    "DIRECTED_ANGLE",
    "DIRECTED_TOLERANCE_ZONE",
    "DIRECTION_FEATURE_TOLERANCE_ZONE",
    "EDGE_ROUND",
    "EXTENSION",
    "FILLET",
    "GENERAL_DATUM_REFERENCE",
    "GEOMETRIC_ALIGNMENT",
    "GEOMETRIC_CONTACT",
    "GEOMETRIC_INTERSECTION",
    "HARNESS_NODE",
    "HARNESS_SEGMENT",
    "HOLE_BOTTOM",
    "INSTANCED_FEATURE",
    "JOGGLE_TERMINATION",
    "LINEAR_PROFILE",
    "MATED_PART_RELATIONSHIP",
    "MODIFIED_PATTERN",
    "NGON_CLOSED_PROFILE",
    "OPEN_PATH_PROFILE",
    "ORIENTED_TOLERANCE_ZONE",
    "PARALLEL_OFFSET",
    "PARTIAL_CIRCULAR_PROFILE",
    "PATH_FEATURE_COMPONENT",
    "PERPENDICULAR_TO",
    "PHYSICAL_COMPONENT_FEATURE",
    "PHYSICAL_COMPONENT_INTERFACE_TERMINAL",
    "PHYSICAL_COMPONENT_TERMINAL",
    "PLACED_DATUM_TARGET_FEATURE",
    "PLACED_FEATURE",
    "POCKET_BOTTOM",
    "PROFILE_FLOOR",
    "RECTANGULAR_CLOSED_PROFILE",
    "RIB_TOP_FLOOR",
    "ROUNDED_U_PROFILE",
    "SHAPE_ASPECT_OCCURRENCE",
    "SLOT_END",
    "SPOTFACE_OCCURRENCE",
    "SPOTFACE_OCCURRENCE_IN_ASSEMBLY",
    "SQUARE_U_PROFILE",
    "TANGENT",
    "TAPER",
    "TEE_PROFILE",
    "TERMINAL_FEATURE",
    "TERMINAL_LOCATION_GROUP",
    "THREAD_RUNOUT",
    "TOLERANCE_ZONE",
    "TOLERANCE_ZONE_WITH_DATUM",
    "TRANSITION_FEATURE",
    "TRANSPORT_FEATURE",
    "TWISTED_CROSS_SECTIONAL_GROUP_SHAPE_ELEMENT",
    "VEE_PROFILE",
];

fn is_shape_aspect_name(name: &str) -> bool {
    name == "SHAPE_ASPECT"
        || name.ends_with("_SHAPE_ASPECT")
        || SHAPE_ASPECT_SUBTYPE_NAMES.contains(&name)
}

fn shape_aspect_parameter<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
    index: usize,
) -> Result<Option<&'a Value>, CodecError> {
    if let Some(partial) = ctx
        .admit_iter(
            &record.partials[..],
            "STEP shape aspect parameter traversal",
        )?
        .find(|partial| partial.name == "SHAPE_ASPECT")
    {
        Ok(partial.parameters.get(index))
    } else {
        Ok(record.parameter(index))
    }
}

fn is_measure_record(ctx: &DecodeContext<'_>, record: &RawRecord) -> Result<bool, CodecError> {
    Ok(ctx
        .admit_iter(
            &record.partials[..],
            "STEP measure record classification traversal",
        )?
        .any(|partial| {
            partial.name == "MEASURE_REPRESENTATION_ITEM"
                || partial.name == "MEASURE_WITH_UNIT"
                || partial.name.ends_with("_MEASURE_WITH_UNIT")
        }))
}

fn is_dimension_name(name: &str) -> bool {
    name == "DIMENSIONAL_SIZE"
        || name.starts_with("DIMENSIONAL_SIZE_")
        || name == "DIMENSIONAL_LOCATION"
        || name.starts_with("DIMENSIONAL_LOCATION_")
        || name == "ANGULAR_SIZE"
        || name.starts_with("ANGULAR_SIZE_")
        || name == "ANGULAR_LOCATION"
        || name.starts_with("ANGULAR_LOCATION_")
        || matches!(name, "DIAMETER_SIZE" | "RADIUS_SIZE")
        || name.ends_with("_SIZE")
        || name.ends_with("_LOCATION")
}

fn dimension_kind(
    name: &str,
    ctx: &DecodeContext<'_>,
) -> Result<Option<DimensionKind>, CodecError> {
    Ok(match name {
        name if name == "DIMENSIONAL_SIZE" || name.starts_with("DIMENSIONAL_SIZE_") => {
            Some(DimensionKind::Size)
        }
        name if name == "DIMENSIONAL_LOCATION" || name.starts_with("DIMENSIONAL_LOCATION_") => {
            Some(DimensionKind::Location)
        }
        name if name == "ANGULAR_SIZE"
            || name.starts_with("ANGULAR_SIZE_")
            || name == "ANGULAR_LOCATION"
            || name.starts_with("ANGULAR_LOCATION_") =>
        {
            Some(DimensionKind::Angular)
        }
        "DIAMETER_SIZE" => Some(DimensionKind::Diameter),
        "RADIUS_SIZE" => Some(DimensionKind::Radius),
        name if name.ends_with("_SIZE") || name.ends_with("_LOCATION") => {
            let mut name = ctx.copy_retained_text(name, "step_pmi_other_dimension_name")?;
            name.make_ascii_lowercase();
            Some(DimensionKind::Other(name))
        }
        _ => None,
    })
}

fn dimension_descriptor<'a>(
    record: &'a RawRecord,
    ctx: &DecodeContext<'_>,
) -> Result<Option<(&'a str, DimensionKind)>, CodecError> {
    for partial in ctx.admit_iter(
        &(record.partials)[..],
        "STEP dimension descriptor traversal",
    )? {
        if let Some(kind) = dimension_kind(partial.name.as_str(), ctx)? {
            return Ok(Some((partial.name.as_str(), kind)));
        }
    }
    Ok(None)
}

fn tolerance_kind(name: Option<&str>) -> Option<GeometricToleranceKind> {
    use GeometricToleranceKind as Kind;
    Some(match name? {
        "STRAIGHTNESS_TOLERANCE" => Kind::Straightness,
        "FLATNESS_TOLERANCE" => Kind::Flatness,
        "ROUNDNESS_TOLERANCE" => Kind::Roundness,
        "CYLINDRICITY_TOLERANCE" => Kind::Cylindricity,
        "COAXIALITY_TOLERANCE" => Kind::Coaxiality,
        "LINE_PROFILE_TOLERANCE" => Kind::LineProfile,
        "SURFACE_PROFILE_TOLERANCE" => Kind::SurfaceProfile,
        "ANGULARITY_TOLERANCE" => Kind::Angularity,
        "PERPENDICULARITY_TOLERANCE" => Kind::Perpendicularity,
        "PARALLELISM_TOLERANCE" => Kind::Parallelism,
        "POSITION_TOLERANCE" => Kind::Position,
        "CONCENTRICITY_TOLERANCE" => Kind::Concentricity,
        "SYMMETRY_TOLERANCE" => Kind::Symmetry,
        "CIRCULAR_RUNOUT_TOLERANCE" => Kind::CircularRunout,
        "TOTAL_RUNOUT_TOLERANCE" => Kind::TotalRunout,
        _ => return None,
    })
}

fn tolerance_modifiers(
    record: &RawRecord,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<String>, CodecError> {
    let mut modifiers = Vec::new();
    if let Some(partial) = ctx
        .admit_iter(&record.partials[..], "STEP tolerance modifiers traversal")?
        .find(|partial| partial.name == "GEOMETRIC_TOLERANCE_WITH_MODIFIERS")
    {
        for value in ctx.admit_iter(
            &(partial.parameters)[..],
            "STEP tolerance modifiers traversal",
        )? {
            modifier_values(value, &mut modifiers, ctx)?;
        }
    }
    Ok(modifiers)
}

fn modifier_values(
    value: &Value,
    output: &mut Vec<String>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let _nested = ctx.enter_nested("step_pmi_modifier_walk")?;
    match value {
        Value::Enumeration(value) => {
            let mut text = ctx.copy_retained_text(value, "step_pmi_modifier_text")?;
            text.make_ascii_lowercase();
            ctx.push_vec(output, text, "step_pmi_modifier_items")?;
        }
        Value::List(values) => {
            for value in
                ctx.admit_iter(values.as_slice(), "STEP modifier values value traversal")?
            {
                modifier_values(value, output, ctx)?;
            }
        }
        Value::Typed(_, value) => modifier_values(value, output, ctx)?,
        _ => {}
    }
    Ok(())
}

fn characteristic_values(
    exchange: &Exchange,
    geometry: &GeometryData,
    losses: &mut Vec<LossNote>,
    graph_limit: usize,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<u64, PmiValue>, CodecError> {
    let mut result = BTreeMap::<u64, PmiValue>::new();
    for (id, record) in exchange.entities(ctx, "DIMENSIONAL_CHARACTERISTIC_REPRESENTATION")? {
        let mut measurements = measure_context(geometry, id, losses, graph_limit);
        let Some(characteristic) = find_record_value(record, ctx, |value| {
            first_matching([value], ctx, |id| {
                Ok({
                    exchange
                        .records()
                        .get(&id)
                        .map(|record| {
                            Ok::<_, CodecError>(
                                ctx.admit_iter(
                                    &record.partials[..],
                                    "STEP characteristic dimension partial traversal",
                                )?
                                .any(|partial| is_dimension_name(&partial.name)),
                            )
                        })
                        .transpose()?
                        .unwrap_or(false)
                })
            })
        })?
        else {
            continue;
        };
        let representation = find_record_value(record, ctx, |value| {
            first_matching([value], ctx, |id| {
                Ok({
                    exchange
                        .records()
                        .get(&id)
                        .map(|record| {
                            Ok::<_, CodecError>(
                                ctx.admit_iter(
                                    &record.partials[..],
                                    "STEP characteristic representation partial traversal",
                                )?
                                .any(|partial| partial.name == "SHAPE_DIMENSION_REPRESENTATION"),
                            )
                        })
                        .transpose()?
                        .unwrap_or(false)
                })
            })
        })?;
        let representation_items =
            if let Some(record) = representation.and_then(|id| exchange.records().get(&id)) {
                ctx.admit_iter(
                    &record.partials[..],
                    "STEP characteristic item partial traversal",
                )?
                .find(|partial| partial.name == "SHAPE_DIMENSION_REPRESENTATION")
                .and_then(|partial| partial.parameters.get(1))
                .and_then(ValueExt::list)
            } else {
                None
            };
        let parameters = representation_items
            .map_or(MeasureParameters::Record(record), MeasureParameters::Items);
        let values = characteristic_measure_values(&parameters, exchange, &mut measurements, ctx)?;
        let mut named_count = 0usize;
        let mut named_first = None;
        for (name, value) in ctx.admit_iter(&values[..], "STEP characteristic values traversal")? {
            if name
                .as_deref()
                .map(|name| {
                    ctx.eq_ignore_ascii_case(
                        name,
                        "nominal value",
                        "STEP characteristic name case equality",
                    )
                })
                .transpose()?
                .unwrap_or(false)
            {
                named_count += 1;
                if named_count == 1 {
                    named_first = Some(*value);
                }
            }
        }
        let selected = if named_count == 1 {
            named_first
        } else if named_count > 1 {
            ctx.push_vec(losses, StepLossCode::DimensionalNominalAmbiguous.note(format!(
                "DIMENSIONAL_CHARACTERISTIC_REPRESENTATION #{id} has {named_count} nominal value measures; the nominal is ambiguous"
                )), "step_pmi_losses")?;
            None
        } else if values.len() == 1 {
            values.first().map(|(_, value)| *value)
        } else {
            if values.len() > 1 {
                ctx.push_vec(losses, StepLossCode::DimensionalUnnamedMeasureAmbiguous.note(format!(
                        "DIMENSIONAL_CHARACTERISTIC_REPRESENTATION #{id} has {} unnamed measure values; the nominal is ambiguous",
                        values.len()
                    )), "step_pmi_losses")?;
            }
            None
        };
        if let Some(selected) = selected {
            ctx.insert_btree_map(
                &mut result,
                characteristic,
                selected,
                "step_pmi_characteristic_values",
            )?;
        }
    }
    Ok(result)
}

enum MeasureParameters<'a> {
    Items(&'a [Value]),
    Record(&'a RawRecord),
}

impl MeasureParameters<'_> {
    fn visit(
        &self,
        ctx: &DecodeContext<'_>,
        mut visitor: impl FnMut(&Value) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        match self {
            Self::Items(items) => {
                for value in ctx.admit_iter(*items, "STEP measure item traversal")? {
                    visitor(value)?;
                }
            }
            Self::Record(record) => {
                for partial in
                    ctx.admit_iter(&record.partials[..], "STEP PMI record partial traversal")?
                {
                    for value in ctx.admit_iter(
                        partial.parameters.as_slice(),
                        "STEP PMI record parameter traversal",
                    )? {
                        visitor(value)?;
                    }
                }
            }
        }
        Ok(())
    }
}

fn characteristic_measure_values(
    parameters: &MeasureParameters<'_>,
    exchange: &Exchange,
    measurements: &mut MeasureContext<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<(Option<String>, PmiValue)>, CodecError> {
    let mut measure_ids = BTreeSet::new();
    parameters.visit(ctx, |parameter| {
        collect_measure_ids(
            parameter,
            exchange,
            &mut BTreeSet::new(),
            0,
            measurements.graph_limit,
            &mut measure_ids,
            ctx,
        )
    })?;
    let mut values = Vec::new();
    for id in measure_ids {
        if let Some(value) = measure(&Value::Reference(id), exchange, measurements, ctx)? {
            let name = exchange
                .records()
                .get(&id)
                .map(|record| measure_item_name(id, record, exchange, measurements.losses, ctx))
                .transpose()?
                .flatten();
            ctx.reserve_vec(&mut values, 1, "step_pmi_measure_values")?;
            values.push((name, value));
        }
    }
    if values.is_empty() {
        parameters.visit(ctx, |parameter| {
            if let Some(value) = measure(parameter, exchange, measurements, ctx)? {
                ctx.reserve_vec(&mut values, 1, "step_pmi_measure_values")?;
                values.push((None, value));
            }
            Ok(())
        })?;
    }
    Ok(values)
}

fn collect_measure_ids(
    value: &Value,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    depth: usize,
    graph_limit: usize,
    measure_ids: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    if depth >= graph_limit {
        return Ok(());
    }
    let _nested = ctx.enter_nested("step_pmi_measure_id_walk")?;
    match value {
        Value::Reference(id) => {
            if active.contains(id) {
                return Ok(());
            }
            ctx.insert_btree_set(active, *id, "step_pmi_measure_active_ids")?;
            if let Some(record) = exchange.records().get(id) {
                if is_measure_record(ctx, record)? {
                    ctx.insert_btree_set(measure_ids, *id, "step_pmi_measure_ids")?;
                } else {
                    for partial in
                        ctx.admit_iter(&record.partials[..], "STEP collect measure ids traversal")?
                    {
                        for parameter in ctx.admit_iter(
                            &(partial.parameters)[..],
                            "STEP collect measure ids traversal",
                        )? {
                            collect_measure_ids(
                                parameter,
                                exchange,
                                active,
                                depth + 1,
                                graph_limit,
                                measure_ids,
                                ctx,
                            )?;
                        }
                    }
                }
            }
            active.remove(id);
        }
        Value::List(values) => {
            for value in ctx.admit_iter(
                values.as_slice(),
                "STEP collect measure ids value traversal",
            )? {
                collect_measure_ids(
                    value,
                    exchange,
                    active,
                    depth + 1,
                    graph_limit,
                    measure_ids,
                    ctx,
                )?;
            }
        }
        Value::Typed(_, value) => {
            collect_measure_ids(
                value,
                exchange,
                active,
                depth + 1,
                graph_limit,
                measure_ids,
                ctx,
            )?;
        }
        _ => {}
    }
    Ok(())
}

fn measure_item_name(
    id: u64,
    record: &RawRecord,
    exchange: &Exchange,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<String>, CodecError> {
    Ok(ctx
        .admit_iter(&record.partials[..], "STEP measure item name traversal")?
        .find(|partial| partial.name == "REPRESENTATION_ITEM")
        .and_then(|partial| partial.parameters.first())
        .map_or_else(
            || {
                Ok::<_, CodecError>(
                    ctx.admit_iter(&record.partials[..], "STEP PMI fallback partial traversal")?
                        .find(|partial| partial.name == "MEASURE_REPRESENTATION_ITEM")
                        .and_then(|partial| partial.parameters.first()),
                )
            },
            |value| Ok(Some(value)),
        )?
        .map(|value| {
            decode_text_charged(
                exchange,
                value,
                losses,
                id,
                "measure item name",
                StepLossCode::MetadataStringInvalid,
                ctx,
            )
        })
        .transpose()?
        .flatten()
        .filter(|name| !name.is_empty()))
}

fn measure_context<'a>(
    geometry: &GeometryData,
    id: u64,
    losses: &'a mut Vec<LossNote>,
    graph_limit: usize,
) -> MeasureContext<'a> {
    MeasureContext {
        length_scale: geometry.units.length([id]).get(),
        angle_scale: geometry.units.angle([id]).get(),
        graph_limit,
        losses,
    }
}

fn first_measure<'a>(
    values: impl IntoIterator<Item = &'a Value>,
    exchange: &Exchange,
    measurements: &mut MeasureContext<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PmiValue>, CodecError> {
    for value in values {
        if let Some(measured) = measure(value, exchange, measurements, ctx)? {
            return Ok(Some(measured));
        }
    }
    Ok(None)
}

fn measure(
    value: &Value,
    exchange: &Exchange,
    measurements: &mut MeasureContext<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PmiValue>, CodecError> {
    measure_inner(value, exchange, &mut BTreeSet::new(), 0, measurements, ctx)
}

fn measure_inner(
    value: &Value,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    depth: usize,
    measurements: &mut MeasureContext<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PmiValue>, CodecError> {
    if depth >= measurements.graph_limit {
        return Ok(None);
    }
    let _nested = ctx.enter_nested("step_pmi_measure_eval_walk")?;
    Ok(match value {
        Value::Integer(value) => cadmpeg_core::convert::f64_from_i64(*value)
            .and_then(|value| PmiValue::new(value, PmiQuantity::Ratio)),
        Value::Real(value) => PmiValue::new(value.get(), PmiQuantity::Ratio),
        Value::Typed(name, value) => match value.number() {
            Some(number) => PmiValue::new(
                if ctx.contains_text(
                    name.as_str(),
                    "LENGTH",
                    "STEP PMI typed length containment",
                )? {
                    number * measurements.length_scale
                } else if ctx.contains_text(
                    name.as_str(),
                    "ANGLE",
                    "STEP PMI typed angle containment",
                )? {
                    number * measurements.angle_scale
                } else {
                    number
                },
                if ctx.contains_text(
                    name.as_str(),
                    "LENGTH",
                    "STEP PMI typed length containment",
                )? {
                    PmiQuantity::Length
                } else if ctx.contains_text(
                    name.as_str(),
                    "ANGLE",
                    "STEP PMI typed angle containment",
                )? {
                    PmiQuantity::Angle
                } else {
                    PmiQuantity::Ratio
                },
            ),
            None => None,
        },
        Value::Reference(id) => {
            if active.contains(id) {
                return Ok(None);
            }
            ctx.insert_btree_set(active, *id, "step_pmi_measure_eval_active")?;
            let Some(record) = exchange.records().get(id) else {
                active.remove(id);
                return Ok(None);
            };
            let mut quantity = None;
            'record_parameters: for partial in
                ctx.admit_iter(&record.partials[..], "STEP measure inner traversal")?
            {
                for parameter in ctx.admit_iter(
                    partial.parameters.as_slice(),
                    "STEP record parameter traversal",
                )? {
                    quantity = measure_quantity(parameter, ctx)?;
                    if quantity.is_some() {
                        break 'record_parameters;
                    }
                }
            }
            let quantity = if let Some(quantity) = quantity {
                quantity
            } else if ctx
                .admit_iter(&record.partials[..], "STEP PMI length classifier traversal")?
                .map(|partial| -> Result<Option<()>, CodecError> {
                    Ok(ctx
                        .contains_text(
                            partial.name.as_str(),
                            "LENGTH",
                            "STEP PMI record length containment",
                        )?
                        .then_some(()))
                })
                .find_map(Result::transpose)
                .transpose()?
                .is_some()
            {
                PmiQuantity::Length
            } else if ctx
                .admit_iter(&record.partials[..], "STEP PMI angle classifier traversal")?
                .map(|partial| -> Result<Option<()>, CodecError> {
                    Ok(ctx
                        .contains_text(
                            partial.name.as_str(),
                            "ANGLE",
                            "STEP PMI record angle containment",
                        )?
                        .then_some(()))
                })
                .find_map(Result::transpose)
                .transpose()?
                .is_some()
            {
                PmiQuantity::Angle
            } else {
                PmiQuantity::Ratio
            };
            let mut unit = None;
            'unit: for partial in
                ctx.admit_iter(&record.partials[..], "STEP PMI unit partial traversal")?
            {
                for candidate in ctx
                    .admit_iter(
                        partial.parameters.as_slice(),
                        "STEP PMI unit reference traversal",
                    )?
                    .filter_map(Value::reference)
                {
                    if let Some(record) = exchange.records().get(&candidate) {
                        if ctx
                            .admit_iter(&record.partials[..], "STEP PMI unit classifier traversal")?
                            .any(|partial| {
                                matches!(partial.name.as_str(), "LENGTH_UNIT" | "PLANE_ANGLE_UNIT")
                            })
                        {
                            unit = Some(candidate);
                            break 'unit;
                        }
                    }
                }
            }
            let scale = match quantity {
                PmiQuantity::Length => {
                    let resolved = match unit {
                        Some(unit) => super::geometry::unit_scale_mm(
                            unit,
                            exchange,
                            &mut BTreeSet::new(),
                            ctx,
                        )?,
                        None => None,
                    };
                    if let Some(scale) = resolved {
                        scale.get()
                    } else {
                        ctx.push_vec(measurements.losses, StepLossCode::PmiLengthUnitUnresolved.note(format!(
                                "PMI length measure #{id} unit scale did not resolve; the document length scale was used"
                            )), "step_pmi_losses")?;
                        measurements.length_scale
                    }
                }
                PmiQuantity::Angle => {
                    let resolved = match unit {
                        Some(unit) => super::geometry::unit_scale_radians(
                            unit,
                            exchange,
                            &mut BTreeSet::new(),
                            ctx,
                        )?,
                        None => None,
                    };
                    if let Some(scale) = resolved {
                        scale.get()
                    } else {
                        ctx.push_vec(measurements.losses, StepLossCode::PmiAngleUnitUnresolved.note(format!(
                                "PMI angle measure #{id} unit scale did not resolve; the document plane-angle scale was used"
                            )), "step_pmi_losses")?;
                        measurements.angle_scale
                    }
                }
                PmiQuantity::Ratio => 1.0,
            };
            let mut result = None;
            'record_parameters: for partial in
                ctx.admit_iter(&record.partials[..], "STEP measure inner traversal")?
            {
                for parameter in ctx.admit_iter(
                    partial.parameters.as_slice(),
                    "STEP record parameter traversal",
                )? {
                    result = ValueExt::typed_number(parameter)
                        .and_then(|number| PmiValue::new(number * scale, quantity));
                    if result.is_none() {
                        result = measure_inner(
                            parameter,
                            exchange,
                            active,
                            depth + 1,
                            measurements,
                            ctx,
                        )?;
                    }
                    if result.is_some() {
                        break 'record_parameters;
                    }
                }
            }
            active.remove(id);
            result
        }
        Value::List(values) => {
            let mut result = None;
            for value in ctx.admit_iter(values.as_slice(), "STEP measure inner value traversal")? {
                result = measure_inner(value, exchange, active, depth + 1, measurements, ctx)?;
                if result.is_some() {
                    break;
                }
            }
            result
        }
        _ => None,
    })
}

fn measure_quantity(
    value: &Value,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PmiQuantity>, CodecError> {
    let _depth = ctx.enter_nested("step_pmi_measure_quantity_walk")?;
    Ok(match value {
        Value::Typed(name, value) => {
            if ctx.contains_text(name.as_str(), "LENGTH", "STEP PMI typed length containment")? {
                Some(PmiQuantity::Length)
            } else if ctx.contains_text(
                name.as_str(),
                "ANGLE",
                "STEP PMI typed angle containment",
            )? {
                Some(PmiQuantity::Angle)
            } else if ctx.contains_text(
                name.as_str(),
                "RATIO",
                "STEP PMI typed ratio containment",
            )? {
                Some(PmiQuantity::Ratio)
            } else {
                measure_quantity(value, ctx)?
            }
        }
        Value::List(values) => {
            let mut quantity = None;
            for value in
                ctx.admit_iter(values.as_slice(), "STEP measure quantity value traversal")?
            {
                quantity = measure_quantity(value, ctx)?;
                if quantity.is_some() {
                    break;
                }
            }
            quantity
        }
        _ => None,
    })
}

#[cfg(test)]
pub(crate) mod tests;
