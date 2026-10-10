// SPDX-License-Identifier: Apache-2.0
//! STEP semantic product-manufacturing information.

use crate::ids::kind;
use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU32;

use super::reference::{first_matching, references};
use super::{find_record_value, named_parameter, source_numeric_id, RecordExt, ValueExt};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
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

use super::geometry::GeometryData;
use super::topology::TopologyData;
use super::StageOutcome;
use super::{decode_output_text, decode_text_scoped};

mod annotations;
mod discovery;

use discovery::{AnnotationDiscoveryIndex, PlacementSelection};

use annotations::{AnnotationDraft, AnnotationIndex, Annotations};

struct MeasureContext<'a, 'ctx> {
    length_scale: f64,
    angle_scale: f64,
    graph_limit: usize,
    losses: (
        &'a mut Vec<LossNote>,
        &'a std::cell::RefCell<ScopedReservation<'ctx>>,
    ),
}

fn collect_pmi_references(
    values: &[Value],
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<Vec<u64>, CodecError> {
    let mut ids = Vec::new();
    ctx.charge_work(0, "STEP collect pmi references traversal")?;
    let mut pmi_source = values.iter();
    for _ in 0..pmi_source.len() {
        let value = ctx
            .next_charged(&mut pmi_source, "STEP collect pmi references traversal")?
            .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
        for id in references(value, ctx) {
            let id = id?;
            ctx.push_vec(&mut ids, id, operation)?;
        }
    }
    Ok(ids)
}

pub(super) fn decode<'ctx>(
    exchange: &Exchange,
    geometry: &GeometryData,
    topology: &TopologyData,
    ir: &mut CadIr,
    ctx: &'ctx DecodeContext<'_>,
) -> Result<
    StageOutcome<(
        cadmpeg_core::decode::ScopedReservation<'ctx>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    )>,
    CodecError,
> {
    let slot_storage = std::cell::RefCell::new(ctx.reserve_scoped(0, "STEP stage report buffers")?);
    let mut claim_storage = ctx.reserve_scoped(0, "STEP stage claim storage")?;
    let mut scratch_storage = ctx.reserve_scoped(0, "STEP decode scratch")?;
    if !exchange.has_entity_matching(ctx, is_pmi_entity_name)? {
        return Ok(StageOutcome {
            value: (claim_storage, slot_storage.into_inner()),
            claims: BTreeSet::new(),
            losses: Vec::new(),
            notes: Vec::new(),
        });
    }
    let mut base_aspects = BTreeSet::new();
    for entity in exchange.entities_any(ctx, &["SHAPE_ASPECT", "DATUM_FEATURE", "DATUM"])? {
        let (id, _) = entity?;
        scratch_storage.with_storage(|| {
            ctx.insert_btree_set(&mut base_aspects, id, "step_pmi_base_aspects")
        })?;
    }
    let mut shape_aspects = BTreeSet::new();
    for id in exchange.matching_entity_ids(ctx, is_shape_aspect_name)? {
        let id = id?;
        scratch_storage.with_storage(|| {
            ctx.insert_btree_set(&mut shape_aspects, id, "step_pmi_shape_aspects")
        })?;
    }
    let mut typed = BTreeSet::new();
    let mut losses = Vec::new();
    let mut annotations = Annotations::new(ctx)?;
    let (hidden_presentation_annotations_buffer, _hidden_storage) = ctx
        .with_scoped_storage("STEP hidden annotation index scratch", || {
            hidden_presentation_annotation_ids(exchange, ctx)
        })?;
    let hidden_presentation_annotations = hidden_presentation_annotations_buffer;

    let mut presentation_semantics = BTreeMap::<u64, Vec<u64>>::new();
    let graph_limit = super::record_graph_limit(ctx);
    let characteristic_values = characteristic_values(
        exchange,
        geometry,
        (&mut losses, &slot_storage),
        graph_limit,
        &mut scratch_storage,
        ctx,
    )?;
    for indexed_entity in exchange.entities(ctx, "DATUM")? {
        let (id, record) = indexed_entity?;
        let identification = record
            .partial(ctx, "DATUM")?
            .and_then(|partial| partial.parameters.first())
            .map(|value| {
                decode_output_text(
                    exchange,
                    value,
                    (&mut losses, &slot_storage),
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
            ir,
            id,
            AnnotationDraft {
                name: shape_aspect_parameter(ctx, record, 0)?
                    .map(|value| {
                        decode_output_text(
                            exchange,
                            value,
                            (&mut losses, &slot_storage),
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
        claim_storage
            .with_storage(|| ctx.insert_btree_set(&mut typed, id, "step_pmi_typed_claims"))?;
    }

    for entity in exchange.matching_entity_ids(ctx, is_datum_target_name)? {
        let id = entity?;
        let Some(record) = ctx.get_btree_map(exchange.records(), &id, "STEP pmi record get")?
        else {
            continue;
        };
        let mut form_storage = ctx.reserve_scoped(0, "STEP datum target form scratch")?;
        let form = shape_aspect_parameter(ctx, record, 1)?
            .map(|value| {
                decode_text_scoped(
                    exchange,
                    value,
                    (&mut losses, &slot_storage),
                    id,
                    ("datum target form", StepLossCode::MetadataStringInvalid),
                    ctx,
                    &mut form_storage,
                )
            })
            .transpose()?
            .flatten()
            .unwrap_or_default();
        let identification = datum_target_identification_parameter(ctx, record)?
            .map(|value| {
                decode_output_text(
                    exchange,
                    value,
                    (&mut losses, &slot_storage),
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
            ir,
            id,
            AnnotationDraft {
                name: shape_aspect_parameter(ctx, record, 0)?
                    .map(|value| {
                        decode_output_text(
                            exchange,
                            value,
                            (&mut losses, &slot_storage),
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
        claim_storage
            .with_storage(|| ctx.insert_btree_set(&mut typed, id, "step_pmi_typed_claims"))?;
    }

    for indexed_entity in exchange.entities(ctx, "DATUM_SYSTEM")? {
        let (id, record) = indexed_entity?;
        let constituents = ctx
            .find_map(
                record.parameters().iter().rev(),
                |value| Ok(ValueExt::list(value)),
                "STEP decode traversal",
            )?
            .unwrap_or_default();
        let mut datum_record_storage = ctx.reserve_scoped(0, "STEP datum claim candidates")?;
        let mut datum_records = BTreeSet::new();
        let mut measurements =
            measure_context(geometry, id, (&mut losses, &slot_storage), graph_limit, ctx)?;
        let mut reference_storage =
            ctx.reserve_scoped(0, "STEP datum reference candidate scratch")?;
        let mut datum_references = Vec::new();
        ctx.charge_work(0, "STEP decode traversal")?;
        let mut pmi_source = constituents.iter().enumerate();
        for _ in 0..pmi_source.len() {
            let (index, constituent) = ctx
                .next_charged(&mut pmi_source, "STEP decode traversal")?
                .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
            let Some(precedence) = u32::try_from(index + 1).ok().and_then(NonZeroU32::new) else {
                continue;
            };
            datum_references_for_compartment(
                (constituent, precedence),
                exchange,
                &annotations,
                (&mut datum_records, &mut datum_record_storage),
                &mut measurements,
                (&mut datum_references, &mut reference_storage),
                ctx,
            )?;
        }
        let datum_references = match datum_references.try_into() {
            Ok(references) => references,
            Err(error) => {
                ctx.push_scoped_vec(
                    &mut slot_storage.borrow_mut(),
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
            ir,
            id,
            AnnotationDraft {
                name: shape_aspect_parameter(ctx, record, 0)?
                    .map(|value| {
                        decode_output_text(
                            exchange,
                            value,
                            (&mut losses, &slot_storage),
                            id,
                            "datum system name",
                            StepLossCode::MetadataStringInvalid,
                            ctx,
                        )
                    })
                    .transpose()?
                    .flatten(),
                targets: {
                    let mut storage = ctx.reserve_scoped(0, "STEP target identity scratch")?;
                    let mut seen = BTreeSet::new();
                    let mut targets = Vec::new();
                    let mut parameters = record.parameters().iter();
                    ctx.charge_work(0, "STEP datum target parameter traversal")?;
                    for _ in 0..parameters.len() {
                        let value = ctx
                            .next_charged(&mut parameters, "STEP datum target parameter traversal")?
                            .ok_or_else(|| {
                                CodecError::malformed("STEP PMI traversal source ended early")
                            })?;
                        for id in references(value, ctx) {
                            let id = id?;
                            if ctx.contains_btree_set(
                                &base_aspects,
                                &id,
                                "STEP pmi base_aspects contains",
                            )? {
                                push_shape_aspect_target(
                                    id,
                                    &mut seen,
                                    &mut storage,
                                    &mut targets,
                                    ctx,
                                )?;
                            }
                        }
                    }
                    targets
                },
                visible: None,
                definition: PmiDefinition::DatumSystem {
                    references: datum_references,
                },
            },
        )?;
        reference_storage.commit()?;
        claim_storage
            .with_storage(|| ctx.insert_btree_set(&mut typed, id, "step_pmi_typed_claims"))?;
        claim_storage.with_storage(|| {
            super::claim_records(ctx, &mut typed, datum_records, "step_pmi_typed_claims")
        })?;
    }

    for entity in exchange.matching_entity_ids(ctx, is_dimension_name)? {
        let id = entity?;
        let Some(record) = ctx.get_btree_map(exchange.records(), &id, "STEP pmi record get")?
        else {
            continue;
        };
        let Some((dimension_name, mut kind)) = dimension_descriptor(record, ctx)? else {
            continue;
        };
        let name = find_record_value(record, ctx, |value| {
            decode_output_text(
                exchange,
                value,
                (&mut losses, &slot_storage),
                id,
                "dimension name",
                StepLossCode::MetadataStringInvalid,
                ctx,
            )
        })?;
        if matches!(kind, DimensionKind::Size) {
            let mut category_storage = ctx.reserve_scoped(0, "STEP dimension category scratch")?;
            let category = if dimension_name.starts_with("DIMENSIONAL_SIZE_WITH_DATUM_FEATURE") {
                if let Some(partial) = ctx.find_map(
                    &record.partials[..],
                    |partial| {
                        Ok(ctx
                            .equal(
                                partial.name.as_str(),
                                dimension_name,
                                "STEP PMI dimension name equality",
                            )?
                            .then_some(partial))
                    },
                    "STEP PMI dimension partial search",
                )? {
                    ctx.find_map(
                        partial.parameters.iter().rev(),
                        |value| {
                            decode_text_scoped(
                                exchange,
                                value,
                                (&mut losses, &slot_storage),
                                id,
                                ("dimension category", StepLossCode::MetadataStringInvalid),
                                ctx,
                                &mut category_storage,
                            )
                        },
                        "STEP dimension category parameter search",
                    )?
                } else {
                    None
                }
            } else {
                None
            };
            let category = category.as_deref().or(name.as_deref());
            kind = if category.is_some_and(|value| value.eq_ignore_ascii_case("diameter")) {
                DimensionKind::Diameter
            } else if category.is_some_and(|value| value.eq_ignore_ascii_case("radius")) {
                DimensionKind::Radius
            } else {
                kind
            };
        }
        let nominal = ctx
            .get_btree_map(
                &characteristic_values,
                &id,
                "STEP pmi characteristic_values get",
            )?
            .copied();
        let definition = PmiDimension::new(kind, nominal, None)
            .map_err(|error| CodecError::malformed(format_args!("dimension #{id}: {error}")))?;
        let mut aspect_targets = Vec::new();
        let mut aspect_storage = ctx.reserve_scoped(0, "STEP dimension aspect scratch")?;
        let mut aspect_ids = BTreeSet::new();
        ctx.charge_work(0, "STEP dimension aspect partial traversal")?;
        let mut pmi_source = record.partials[..].iter();
        for _ in 0..pmi_source.len() {
            let partial = ctx
                .next_charged(&mut pmi_source, "STEP dimension aspect partial traversal")?
                .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
            ctx.charge_work(0, "STEP dimension aspect parameter traversal")?;
            let mut pmi_source = partial.parameters.as_slice().iter();
            for _ in 0..pmi_source.len() {
                let value = ctx
                    .next_charged(&mut pmi_source, "STEP dimension aspect parameter traversal")?
                    .ok_or_else(|| {
                        CodecError::malformed("STEP PMI traversal source ended early")
                    })?;
                for reference in references(value, ctx) {
                    let id = reference?;
                    if ctx.contains_btree_set(
                        &shape_aspects,
                        &id,
                        "STEP pmi shape_aspects contains",
                    )? && !ctx.contains_btree_set(
                        &aspect_ids,
                        &id,
                        "STEP pmi aspect_ids contains",
                    )? {
                        aspect_storage.with_storage(|| {
                            ctx.insert_btree_set(&mut aspect_ids, id, "step_pmi_target_ids")
                        })?;
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
            ir,
            id,
            AnnotationDraft {
                name,
                targets: aspect_targets,
                visible: None,
                definition: PmiDefinition::Dimension(definition),
            },
        )?;
        claim_storage
            .with_storage(|| ctx.insert_btree_set(&mut typed, id, "step_pmi_typed_claims"))?;
    }

    for indexed_entity in exchange.entities(ctx, "PLUS_MINUS_TOLERANCE")? {
        let (id, record) = indexed_entity?;
        let (refs_buffer, _reference_storage) = ctx
            .with_scoped_storage("STEP PMI reference scratch", || {
                collect_pmi_references(record.parameters(), ctx, "step_pmi_plus_minus_references")
            })?;
        let refs = refs_buffer;
        let dimension = ctx.find_map(
            &refs[..],
            |reference| annotations.get(*reference),
            "STEP decode traversal",
        )?;
        let limits = ctx.find_map(
            &refs[..],
            |reference| {
                Ok(ctx
                    .get_btree_map(exchange.records(), reference, "STEP pmi record get")?
                    .filter(|candidate| candidate.simple_name() == Some("TOLERANCE_VALUE")))
            },
            "STEP decode traversal",
        )?;
        let mut fit_storage = ctx.reserve_scoped(0, "STEP limits-and-fits candidate scratch")?;
        let fit = ctx.find_map(
            &refs[..],
            |reference| {
                let Some(record) =
                    ctx.get_btree_map(exchange.records(), reference, "STEP pmi record get")?
                else {
                    return Ok(None);
                };
                if record.simple_name() != Some("LIMITS_AND_FITS") {
                    return Ok(None);
                }
                Ok(Some((
                    *reference,
                    LimitsAndFits {
                        form_variance: record
                            .parameter(0)
                            .map(|value| {
                                decode_text_scoped(
                                    exchange,
                                    value,
                                    (&mut losses, &slot_storage),
                                    *reference,
                                    (
                                        "limits-and-fits form variance",
                                        StepLossCode::MetadataStringInvalid,
                                    ),
                                    ctx,
                                    &mut fit_storage,
                                )
                            })
                            .transpose()?
                            .flatten()
                            .unwrap_or_default(),
                        zone_variance: record
                            .parameter(1)
                            .map(|value| {
                                decode_text_scoped(
                                    exchange,
                                    value,
                                    (&mut losses, &slot_storage),
                                    *reference,
                                    (
                                        "limits-and-fits zone variance",
                                        StepLossCode::MetadataStringInvalid,
                                    ),
                                    ctx,
                                    &mut fit_storage,
                                )
                            })
                            .transpose()?
                            .flatten()
                            .unwrap_or_default(),
                        grade: record
                            .parameter(2)
                            .map(|value| {
                                decode_text_scoped(
                                    exchange,
                                    value,
                                    (&mut losses, &slot_storage),
                                    *reference,
                                    ("limits-and-fits grade", StepLossCode::MetadataStringInvalid),
                                    ctx,
                                    &mut fit_storage,
                                )
                            })
                            .transpose()?
                            .flatten()
                            .unwrap_or_default(),
                        source: record
                            .parameter(3)
                            .map(|value| {
                                decode_text_scoped(
                                    exchange,
                                    value,
                                    (&mut losses, &slot_storage),
                                    *reference,
                                    (
                                        "limits-and-fits source",
                                        StepLossCode::MetadataStringInvalid,
                                    ),
                                    ctx,
                                    &mut fit_storage,
                                )
                            })
                            .transpose()?
                            .flatten()
                            .unwrap_or_default(),
                    },
                )))
            },
            "STEP decode traversal",
        )?;
        if let (Some(index), Some(limits)) = (dimension, limits) {
            let mut measurements =
                measure_context(geometry, id, (&mut losses, &slot_storage), graph_limit, ctx)?;
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
                    claim_storage.with_storage(|| {
                        ctx.insert_btree_set(&mut typed, id, "step_pmi_typed_claims")
                    })?;
                    claim_storage.with_storage(|| {
                        super::claim_records(ctx, &mut typed, refs, "step_pmi_typed_claims")
                    })?;
                } else {
                    ctx.push_scoped_vec(
                        &mut slot_storage.borrow_mut(),
                        &mut losses,
                        StepLossCode::DecodeWarning.note(format!(
                        "PLUS_MINUS_TOLERANCE #{id} is an additional tolerance for one dimension"
                    )),
                        "step_pmi_losses",
                    )?;
                }
            } else {
                ctx.push_scoped_vec(
                    &mut slot_storage.borrow_mut(),
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
                fit_storage.commit()?;
                for claim in [id, fit_id] {
                    claim_storage.with_storage(|| {
                        ctx.insert_btree_set(&mut typed, claim, "step_pmi_typed_claims")
                    })?;
                }
            } else {
                ctx.push_scoped_vec(
                    &mut slot_storage.borrow_mut(),
                    &mut losses,
                    StepLossCode::DecodeWarning.note(format!(
                        "PLUS_MINUS_TOLERANCE #{id} is an additional tolerance for one dimension"
                    )),
                    "step_pmi_losses",
                )?;
            }
        } else {
            ctx.push_scoped_vec(
                &mut slot_storage.borrow_mut(),
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
        let Some(record) = ctx.get_btree_map(exchange.records(), &id, "STEP pmi record get")?
        else {
            continue;
        };
        let Some(tolerance) = ctx
            .find_map(
                &record.partials[..],
                |partial| {
                    Ok((partial.name != "GEOMETRIC_TOLERANCE")
                        .then(|| tolerance_kind(Some(&partial.name)))
                        .flatten())
                },
                "STEP decode traversal",
            )?
            .map_or_else(
                || {
                    ctx.find_map(
                        &record.partials[..],
                        |partial| Ok(tolerance_kind(Some(&partial.name))),
                        "STEP PMI fallback partial traversal",
                    )
                },
                |value| Ok(Some(value)),
            )?
        else {
            continue;
        };
        let reference_values = record
            .partial(ctx, "GEOMETRIC_TOLERANCE")?
            .map_or(record.parameters(), |partial| partial.parameters.as_slice());
        let (refs_buffer, _reference_storage) =
            ctx.with_scoped_storage("STEP PMI reference scratch", || {
                collect_pmi_references(
                    reference_values,
                    ctx,
                    "step_pmi_geometric_tolerance_references",
                )
            })?;
        let refs = refs_buffer;
        let mut measurements =
            measure_context(geometry, id, (&mut losses, &slot_storage), graph_limit, ctx)?;
        let magnitude = first_measure(
            record
                .partial(ctx, "GEOMETRIC_TOLERANCE")?
                .map(|partial| partial.parameters.as_slice())
                .unwrap_or_default(),
            exchange,
            &mut measurements,
            ctx,
        )?;
        let magnitude = match magnitude {
            Some(magnitude) => Some(magnitude),
            None => ctx.find_map(
                &record.partials[..],
                |partial| {
                    if partial.name == "GEOMETRIC_TOLERANCE" {
                        return Ok(None);
                    }
                    first_measure(
                        partial.parameters.as_slice(),
                        exchange,
                        &mut measurements,
                        ctx,
                    )
                },
                "STEP tolerance fallback partial traversal",
            )?,
        };
        let Some(magnitude) = magnitude.and_then(cadmpeg_ir::pmi::PmiMagnitude::new) else {
            let (display_name_buffer, _display_storage) = ctx
                .with_scoped_storage("STEP tolerance type text scratch", || {
                    super::record_type_text(record, ctx, "step_record_display_name")
                })?;
            let display_name = display_name_buffer;
            let message = ctx.format_retained(
                format_args!("{display_name} #{id} has no numeric magnitude"),
                "step_pmi_invalid_tolerance_text",
            )?;
            ctx.push_scoped_vec(
                &mut slot_storage.borrow_mut(),
                &mut losses,
                StepLossCode::DecodeWarning.note(message),
                "step_pmi_losses",
            )?;
            continue;
        };
        let defined_unit = record
            .partial(ctx, "GEOMETRIC_TOLERANCE_WITH_DEFINED_UNIT")?
            .and_then(|partial| partial.parameters.first())
            .map(|value| measure(value, exchange, &mut measurements, ctx))
            .transpose()?
            .flatten();
        let (defined_area_unit, defined_area_second_unit) = if let Some(partial) =
            record.partial(ctx, "GEOMETRIC_TOLERANCE_WITH_DEFINED_AREA_UNIT")?
        {
            let area = partial
                .parameters
                .first()
                .and_then(ValueExt::enumeration)
                .map(|name| {
                    let mut name =
                        ctx.copy_retained_text(name, "step_pmi_defined_area_unit_text")?;
                    ctx.make_ascii_lowercase(&mut name, "STEP PMI text case conversion")?;
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
        let datum_values = record
            .partial(ctx, "GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE")?
            .map(|partial| partial.parameters.as_slice())
            .unwrap_or_default();
        let datum_system = ctx
            .find_map(
                datum_values,
                |value| {
                    first_matching([value], ctx, |id| {
                        Ok(annotations.get(id)?.is_some_and(|index| {
                            matches!(
                                ir.model.pmi[index.get()].definition,
                                PmiDefinition::DatumSystem { .. }
                            )
                        }))
                    })
                },
                "STEP tolerance datum parameter traversal",
            )?
            .map(|id| annotations.get(id))
            .transpose()?
            .flatten()
            .map(|index| &ir.model.pmi[index.get()].id)
            .map(|id| id.try_clone_for_decode(ctx, "step_pmi_datum_system_identity_copy"))
            .transpose()?;
        annotations.push(
            ir,
            id,
            AnnotationDraft {
                name: record
                    .partial(ctx, "GEOMETRIC_TOLERANCE")?
                    .and_then(|partial| partial.parameters.first())
                    .or_else(|| record.parameter(0))
                    .map(|value| {
                        decode_output_text(
                            exchange,
                            value,
                            (&mut losses, &slot_storage),
                            id,
                            "geometric tolerance name",
                            StepLossCode::MetadataStringInvalid,
                            ctx,
                        )
                    })
                    .transpose()?
                    .flatten(),
                targets: {
                    let mut storage = ctx.reserve_scoped(0, "STEP target identity scratch")?;
                    let mut seen = BTreeSet::new();
                    let mut targets = Vec::new();
                    let mut references = refs.iter();
                    ctx.charge_work(0, "STEP tolerance target reference traversal")?;
                    for _ in 0..references.len() {
                        let &id = ctx
                            .next_charged(
                                &mut references,
                                "STEP tolerance target reference traversal",
                            )?
                            .ok_or_else(|| {
                                CodecError::malformed("STEP PMI traversal source ended early")
                            })?;
                        if ctx.contains_btree_set(
                            &base_aspects,
                            &id,
                            "STEP pmi base_aspects contains",
                        )? {
                            push_shape_aspect_target(
                                id,
                                &mut seen,
                                &mut storage,
                                &mut targets,
                                ctx,
                            )?;
                        }
                    }
                    targets
                },
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
        claim_storage
            .with_storage(|| ctx.insert_btree_set(&mut typed, id, "step_pmi_typed_claims"))?;
        ctx.charge_work(0, "STEP tolerance measure reference traversal")?;
        let mut pmi_source = refs.as_slice().iter().copied();
        for _ in 0..pmi_source.len() {
            let reference = ctx
                .next_charged(
                    &mut pmi_source,
                    "STEP tolerance measure reference traversal",
                )?
                .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
            if let Some(record) =
                ctx.get_btree_map(exchange.records(), &reference, "STEP pmi record get")?
            {
                if is_measure_record(ctx, record)? {
                    claim_storage.with_storage(|| {
                        ctx.insert_btree_set(&mut typed, reference, "step_pmi_typed_claims")
                    })?;
                }
            }
        }
        ctx.charge_work(0, "STEP decode traversal")?;
        let mut pmi_source = record.partials[..].iter();
        for _ in 0..pmi_source.len() {
            let partial = ctx
                .next_charged(&mut pmi_source, "STEP decode traversal")?
                .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
            ctx.charge_work(0, "STEP record parameter traversal")?;
            let mut pmi_source = partial.parameters.as_slice().iter();
            for _ in 0..pmi_source.len() {
                let value = ctx
                    .next_charged(&mut pmi_source, "STEP record parameter traversal")?
                    .ok_or_else(|| {
                        CodecError::malformed("STEP PMI traversal source ended early")
                    })?;
                for reference in references(value, ctx) {
                    let reference = reference?;
                    if let Some(record) =
                        ctx.get_btree_map(exchange.records(), &reference, "STEP pmi record get")?
                    {
                        if is_measure_record(ctx, record)? {
                            claim_storage.with_storage(|| {
                                ctx.insert_btree_set(&mut typed, reference, "step_pmi_typed_claims")
                            })?;
                        }
                    }
                }
            }
        }
    }

    for indexed_entity in exchange.entities(ctx, "DRAUGHTING_MODEL_ITEM_ASSOCIATION")? {
        let (id, record) = indexed_entity?;
        let Some(definition) = record
            .partial(ctx, "DRAUGHTING_MODEL_ITEM_ASSOCIATION")?
            .and_then(|partial| partial.parameters.get(2))
            .and_then(ValueExt::reference)
        else {
            continue;
        };
        if annotations.get(definition)?.is_some() {
            if let Some(items) = record
                .partial(ctx, "DRAUGHTING_MODEL_ITEM_ASSOCIATION")?
                .and_then(|partial| partial.parameters.get(4))
            {
                for item in references(items, ctx) {
                    let item = item?;
                    scratch_storage.with_storage(|| {
                        ctx.push_btree_group(
                            &mut presentation_semantics,
                            item,
                            definition,
                            "step_pmi_presentation_semantic_groups",
                            "step_pmi_presentation_semantic_members",
                        )
                    })?;
                }
            }
            claim_storage
                .with_storage(|| ctx.insert_btree_set(&mut typed, id, "step_pmi_typed_claims"))?;
        }
    }

    let mut discovery = AnnotationDiscoveryIndex::new(ctx)?;
    for entity in exchange.matching_entity_ids(ctx, is_presentation_annotation)? {
        let id = entity?;
        let Some(record) = ctx.get_btree_map(exchange.records(), &id, "STEP pmi record get")?
        else {
            continue;
        };
        let Some(name) = presentation_annotation_name(ctx, record)? else {
            continue;
        };
        let mut text_record_storage = ctx.reserve_scoped(0, "STEP annotation claim candidates")?;
        let mut text_records = BTreeSet::new();
        let text = discovery.text(
            id,
            exchange,
            geometry,
            (&mut text_records, &mut text_record_storage),
            (&mut losses, &slot_storage),
        )?;
        // Placement identity is the carrier key; the transform value cannot
        // make two source carriers one semantic carrier.
        let placement = match discovery.placement(record, exchange, geometry)? {
            PlacementSelection::Absent => None,
            PlacementSelection::Unique(placement) => Some(placement),
            PlacementSelection::Ambiguous(count) => {
                ctx.push_scoped_vec(&mut slot_storage.borrow_mut(), &mut losses, StepLossCode::PresentationAnnotationPlacementAmbiguous.note(
                    format!(
                        "presentation annotation #{id} has {count} reachable placement carriers with no unique placement"
                    ),
                ), "step_pmi_losses")?;
                None
            }
        };
        let mut semantics = Vec::new();
        ctx.charge_work(0, "STEP PMI record partial traversal")?;
        let mut pmi_source = record.partials[..].iter();
        for _ in 0..pmi_source.len() {
            let partial = ctx
                .next_charged(&mut pmi_source, "STEP PMI record partial traversal")?
                .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
            ctx.charge_work(0, "STEP PMI record parameter traversal")?;
            let mut pmi_source = partial.parameters.as_slice().iter();
            for _ in 0..pmi_source.len() {
                let parameter = ctx
                    .next_charged(&mut pmi_source, "STEP PMI record parameter traversal")?
                    .ok_or_else(|| {
                        CodecError::malformed("STEP PMI traversal source ended early")
                    })?;
                for reference in references(parameter, ctx) {
                    let reference = reference?;
                    if annotations.get(reference)?.is_some() {
                        ctx.push_vec(
                            &mut semantics,
                            pmi_id(reference),
                            "step_pmi_presentation_semantics",
                        )?;
                    }
                }
            }
        }
        if let Some(items) = ctx.get_btree_map(
            &presentation_semantics,
            &id,
            "STEP pmi presentation_semantics get",
        )? {
            ctx.charge_work(0, "STEP optional collection traversal")?;
            let mut pmi_source = items.iter();
            for _ in 0..pmi_source.len() {
                let semantic = ctx
                    .next_charged(&mut pmi_source, "STEP optional collection traversal")?
                    .ok_or_else(|| {
                        CodecError::malformed("STEP PMI traversal source ended early")
                    })?;
                ctx.push_vec(
                    &mut semantics,
                    pmi_id(*semantic),
                    "step_pmi_presentation_semantics",
                )?;
            }
        }
        annotations.push(
            ir,
            id,
            AnnotationDraft {
                name: named_parameter(ctx, record, name, 0)?
                    .map_or_else(
                        || -> Result<_, CodecError> {
                            Ok(record
                                .partial(ctx, "REPRESENTATION_ITEM")?
                                .and_then(|partial| partial.parameters.first()))
                        },
                        |value| Ok(Some(value)),
                    )?
                    .or_else(|| record.parameter(0))
                    .map(|value| {
                        decode_output_text(
                            exchange,
                            value,
                            (&mut losses, &slot_storage),
                            id,
                            "presentation annotation name",
                            StepLossCode::MetadataStringInvalid,
                            ctx,
                        )
                    })
                    .transpose()?
                    .flatten(),
                targets: Vec::new(),
                visible: ctx
                    .contains_btree_set(
                        &hidden_presentation_annotations,
                        &id,
                        "STEP pmi hidden_presentation_annotations contains",
                    )?
                    .then_some(false),
                definition: PmiDefinition::Presentation {
                    text,
                    placement,
                    semantics,
                },
            },
        )?;
        claim_storage
            .with_storage(|| ctx.insert_btree_set(&mut typed, id, "step_pmi_typed_claims"))?;
        claim_storage.with_storage(|| {
            super::claim_records(ctx, &mut typed, text_records, "step_pmi_typed_claims")
        })?;
    }
    for entity in exchange.entities_any(
        ctx,
        &["DRAUGHTING_MODEL", "ANNOTATION_PLANE", "DRAUGHTING_CALLOUT"],
    )? {
        let (id, _) = entity?;
        claim_storage
            .with_storage(|| ctx.insert_btree_set(&mut typed, id, "step_pmi_typed_claims"))?;
    }

    resolve_feature_for_datum_target_relationships(
        exchange,
        &annotations,
        ir,
        (&mut typed, &mut claim_storage),
        ctx,
    )?;
    let (points_by_source_buffer, _point_storage) =
        ctx.with_scoped_storage("STEP PMI point source scratch", || point_sources(ir, ctx))?;
    let points_by_source = points_by_source_buffer;
    let (curves_by_source_buffer, _curve_storage) =
        ctx.with_scoped_storage("STEP PMI curve source scratch", || curve_sources(ir, ctx))?;
    let curves_by_source = curves_by_source_buffer;
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
        (&mut typed, &mut claim_storage),
        ctx,
    )?;

    let mut targeted_aspects = BTreeSet::new();
    ctx.charge_work(0, "STEP targeted annotation traversal")?;
    let mut pmi_source = ir.model.pmi[..].iter();
    for _ in 0..pmi_source.len() {
        let annotation = ctx
            .next_charged(&mut pmi_source, "STEP targeted annotation traversal")?
            .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
        ctx.charge_work(0, "STEP annotation target traversal")?;
        let mut pmi_source = annotation.targets.iter();
        for _ in 0..pmi_source.len() {
            let target = ctx
                .next_charged(&mut pmi_source, "STEP annotation target traversal")?
                .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
            let id = match target {
                PmiTarget::ShapeAspect { source_id } => {
                    match source_id.as_str().strip_prefix('#') {
                        Some(number) => ctx
                            .parse_text::<u64>(number, "STEP PMI targeted aspect number parse")?
                            .ok(),
                        None => None,
                    }
                }
                _ => None,
            };
            if let Some(id) = id {
                scratch_storage.with_storage(|| {
                    ctx.insert_btree_set(&mut targeted_aspects, id, "step_pmi_targeted_aspects")
                })?;
            }
        }
    }
    {
        let mut visited_items = targeted_aspects.iter();
        ctx.charge_work(0, "step_pmi_typed_claims")?;
        for _ in 0..visited_items.len() {
            let &id = ctx
                .next_charged(&mut visited_items, "step_pmi_typed_claims")?
                .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
            if ctx.contains_btree_set(&shape_aspects, &id, "step_pmi_typed_claims")? {
                claim_storage.with_storage(|| {
                    ctx.insert_btree_set(&mut typed, id, "step_pmi_typed_claims")
                })?;
            }
        }
    }
    mark_characteristic_representations(
        exchange,
        &annotations,
        (&mut typed, &mut claim_storage),
        ctx,
    )?;
    Ok(StageOutcome {
        value: (claim_storage, slot_storage.into_inner()),
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
    (typed, claim_storage): (&mut BTreeSet<u64>, &mut ScopedReservation<'_>),
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let mut visited_storage = ctx.reserve_scoped(0, "STEP characteristic claim index scratch")?;
    let mut visited = BTreeSet::new();
    for indexed_entity in exchange.entities(ctx, "DIMENSIONAL_CHARACTERISTIC_REPRESENTATION")? {
        let (id, record) = indexed_entity?;
        let Some(_) = find_record_value(record, ctx, |value| {
            first_matching([value], ctx, |reference| {
                Ok(annotations.get(reference)?.is_some())
            })
        })?
        else {
            continue;
        };
        claim_storage.with_storage(|| ctx.insert_btree_set(typed, id, "step_pmi_typed_claims"))?;
        ctx.charge_work(0, "STEP PMI record partial traversal")?;
        let mut pmi_source = record.partials[..].iter();
        for _ in 0..pmi_source.len() {
            let partial = ctx
                .next_charged(&mut pmi_source, "STEP PMI record partial traversal")?
                .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
            ctx.charge_work(0, "STEP PMI record parameter traversal")?;
            let mut pmi_source = partial.parameters.as_slice().iter();
            for _ in 0..pmi_source.len() {
                let parameter = ctx
                    .next_charged(&mut pmi_source, "STEP PMI record parameter traversal")?
                    .ok_or_else(|| {
                        CodecError::malformed("STEP PMI traversal source ended early")
                    })?;
                for representation_id in references(parameter, ctx) {
                    let representation_id = representation_id?;
                    if !visited_storage.with_storage(|| {
                        ctx.insert_btree_set(
                            &mut visited,
                            representation_id,
                            "step_characteristic_claim_visited",
                        )
                    })? {
                        continue;
                    }
                    let Some(representation) = ctx.get_btree_map(
                        exchange.records(),
                        &representation_id,
                        "STEP pmi record get",
                    )?
                    else {
                        continue;
                    };
                    if representation
                        .partial(ctx, "SHAPE_DIMENSION_REPRESENTATION")?
                        .is_none()
                    {
                        continue;
                    }
                    claim_storage.with_storage(|| {
                        ctx.insert_btree_set(typed, representation_id, "step_pmi_typed_claims")
                    })?;
                    ctx.charge_work(0, "STEP PMI record partial traversal")?;
                    let mut pmi_source = representation.partials[..].iter();
                    for _ in 0..pmi_source.len() {
                        let partial = ctx
                            .next_charged(&mut pmi_source, "STEP PMI record partial traversal")?
                            .ok_or_else(|| {
                                CodecError::malformed("STEP PMI traversal source ended early")
                            })?;
                        ctx.charge_work(0, "STEP PMI record parameter traversal")?;
                        let mut pmi_source = partial.parameters.as_slice().iter();
                        for _ in 0..pmi_source.len() {
                            let parameter = ctx
                                .next_charged(
                                    &mut pmi_source,
                                    "STEP PMI record parameter traversal",
                                )?
                                .ok_or_else(|| {
                                    CodecError::malformed("STEP PMI traversal source ended early")
                                })?;
                            for reference in references(parameter, ctx) {
                                let reference = reference?;
                                if let Some(record) = ctx.get_btree_map(
                                    exchange.records(),
                                    &reference,
                                    "STEP pmi record get",
                                )? {
                                    if is_measure_record(ctx, record)? {
                                        claim_storage.with_storage(|| {
                                            ctx.insert_btree_set(
                                                typed,
                                                reference,
                                                "step_pmi_typed_claims",
                                            )
                                        })?;
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
    (typed, claim_storage): (&mut BTreeSet<u64>, &mut ScopedReservation<'_>),
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let mut target_storage = ctx.reserve_scoped(0, "STEP PMI target indices")?;
    let mut target_indices = BTreeMap::<usize, TargetIndex>::new();

    for indexed_entity in exchange.entities(ctx, "FEATURE_FOR_DATUM_TARGET_RELATIONSHIP")? {
        let (id, record) = indexed_entity?;
        let Some((relating, related)) = relationship_endpoints(record, ctx)? else {
            continue;
        };
        let Some(annotation_index) = annotations.get(related)? else {
            continue;
        };
        let annotation = &mut ir.model.pmi[annotation_index.get()];
        let PmiDefinition::DatumTarget { basis, .. } = &mut annotation.definition else {
            continue;
        };
        let (source_id_buffer, _source_storage) = ctx
            .with_scoped_storage("STEP datum basis source", || {
                super::step_source_id(ctx, relating)
            })?;
        let source_id = source_id_buffer;
        let seen = target_index(
            &mut target_indices,
            &mut target_storage,
            annotation_index.get(),
            basis,
            ctx,
        )?;
        push_target(
            (seen, &mut target_storage),
            basis,
            (8, source_id.as_str()),
            || {
                Ok(PmiTarget::ShapeAspect {
                    source_id: source_id
                        .try_clone_for_decode(ctx, "step_pmi_datum_basis_identity")?,
                })
            },
            ctx,
            "step_pmi_datum_basis_targets",
        )?;
        for claim in [id, relating] {
            claim_storage
                .with_storage(|| ctx.insert_btree_set(typed, claim, "step_pmi_typed_claims"))?;
        }
    }
    Ok(())
}

fn resolve_geometric_item_usages(
    exchange: &Exchange,
    topology: &TopologyData,
    geometry_sources: GeometrySources<'_>,
    (shape_aspects, annotations): (&BTreeSet<u64>, &Annotations),
    ir: &mut CadIr,
    (typed, claim_storage): (&mut BTreeSet<u64>, &mut ScopedReservation<'_>),
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let mut scratch_storage =
        ctx.reserve_scoped(0, "STEP resolve_geometric_item_usages scratch")?;
    let mut target_storage = ctx.reserve_scoped(0, "STEP PMI target indices")?;
    let mut target_indices = BTreeMap::<usize, TargetIndex>::new();

    let mut aspect_annotations = BTreeMap::<u64, BTreeSet<AnnotationIndex>>::new();
    {
        let mut visited_items = exchange.records().iter();
        ctx.charge_work(0, "STEP resolve geometric item usages traversal")?;
        for _ in 0..visited_items.len() {
            let (&annotation_id, record) = ctx
                .next_charged(
                    &mut visited_items,
                    "STEP resolve geometric item usages traversal",
                )?
                .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
            let Some(annotation_index) = annotations.get(annotation_id)? else {
                continue;
            };
            if ctx.contains_btree_set(
                shape_aspects,
                &annotation_id,
                "STEP pmi shape_aspects contains",
            )? {
                scratch_storage.with_storage(|| {
                    ctx.insert_btree_group_set(
                        &mut aspect_annotations,
                        annotation_id,
                        annotation_index,
                        "step_pmi_aspect_annotation_groups",
                        "step_pmi_aspect_annotation_members",
                    )
                })?;
            }
            ctx.charge_work(0, "STEP PMI record partial traversal")?;
            let mut pmi_source = record.partials[..].iter();
            for _ in 0..pmi_source.len() {
                let partial = ctx
                    .next_charged(&mut pmi_source, "STEP PMI record partial traversal")?
                    .ok_or_else(|| {
                        CodecError::malformed("STEP PMI traversal source ended early")
                    })?;
                ctx.charge_work(0, "STEP PMI record parameter traversal")?;
                let mut pmi_source = partial.parameters.as_slice().iter();
                for _ in 0..pmi_source.len() {
                    let parameter = ctx
                        .next_charged(&mut pmi_source, "STEP PMI record parameter traversal")?
                        .ok_or_else(|| {
                            CodecError::malformed("STEP PMI traversal source ended early")
                        })?;
                    for reference in references(parameter, ctx) {
                        let reference = reference?;
                        if ctx.contains_btree_set(
                            shape_aspects,
                            &reference,
                            "STEP pmi shape_aspects contains",
                        )? {
                            scratch_storage.with_storage(|| {
                                ctx.insert_btree_group_set(
                                    &mut aspect_annotations,
                                    reference,
                                    annotation_index,
                                    "step_pmi_aspect_annotation_groups",
                                    "step_pmi_aspect_annotation_members",
                                )
                            })?;
                        }
                    }
                }
            }
        }
    }

    let mut relationship_aspects = BTreeMap::<u64, BTreeSet<u64>>::new();
    {
        let mut visited_items = exchange.records().iter();
        ctx.charge_work(0, "STEP resolve geometric item usages map traversal")?;
        for _ in 0..visited_items.len() {
            let (_, record) = ctx
                .next_charged(
                    &mut visited_items,
                    "STEP resolve geometric item usages map traversal",
                )?
                .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
            let Some((relating, related)) = relationship_endpoints(record, ctx)? else {
                continue;
            };
            scratch_storage.with_storage(|| {
                ctx.insert_btree_group_set(
                    &mut relationship_aspects,
                    relating,
                    related,
                    "step_pmi_relationship_aspect_groups",
                    "step_pmi_relationship_aspect_members",
                )
            })?;
            scratch_storage.with_storage(|| {
                ctx.insert_btree_group_set(
                    &mut relationship_aspects,
                    related,
                    relating,
                    "step_pmi_relationship_aspect_groups",
                    "step_pmi_relationship_aspect_members",
                )
            })?;
        }
    }

    {
        let mut visited_items = exchange.records().iter();
        ctx.charge_work(0, "STEP resolve geometric item usages traversal")?;
        for _ in 0..visited_items.len() {
            let (&id, record) = ctx
                .next_charged(
                    &mut visited_items,
                    "STEP resolve geometric item usages traversal",
                )?
                .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
            let Some(partial) = record.partial(ctx, "GEOMETRIC_ITEM_SPECIFIC_USAGE")? else {
                continue;
            };
            let Some(definition) = first_matching(partial.parameters.get(2), ctx, |_| Ok(true))?
            else {
                continue;
            };
            let Some(identified_item) =
                first_matching(partial.parameters.get(4), ctx, |_| Ok(true))?
            else {
                continue;
            };
            let mut annotation_storage = ctx.reserve_scoped(0, "STEP usage annotation scratch")?;
            let mut annotation_indices = BTreeSet::new();
            if let Some(items) = ctx.get_btree_map(
                &aspect_annotations,
                &definition,
                "STEP pmi aspect_annotations get",
            )? {
                {
                    let mut visited_items = items.iter();
                    ctx.charge_work(0, "STEP optional collection traversal")?;
                    for _ in 0..visited_items.len() {
                        let &index = ctx
                            .next_charged(&mut visited_items, "STEP optional collection traversal")?
                            .ok_or_else(|| {
                                CodecError::malformed("STEP PMI traversal source ended early")
                            })?;
                        annotation_storage.with_storage(|| {
                            ctx.insert_btree_set(
                                &mut annotation_indices,
                                index,
                                "step_pmi_usage_annotation_indices",
                            )
                        })?;
                    }
                }
            }
            if let Some(aspects) = ctx.get_btree_map(
                &relationship_aspects,
                &definition,
                "STEP pmi relationship_aspects get",
            )? {
                {
                    let mut visited_items = aspects.iter();
                    ctx.charge_work(0, "STEP pmi aspects traversal")?;
                    for _ in 0..visited_items.len() {
                        let aspect = ctx
                            .next_charged(&mut visited_items, "STEP pmi aspects traversal")?
                            .ok_or_else(|| {
                                CodecError::malformed("STEP PMI traversal source ended early")
                            })?;
                        if let Some(items) = ctx.get_btree_map(
                            &aspect_annotations,
                            aspect,
                            "STEP pmi aspect_annotations get",
                        )? {
                            {
                                let mut visited_items = items.iter();
                                ctx.charge_work(0, "STEP optional collection traversal")?;
                                for _ in 0..visited_items.len() {
                                    let &index = ctx
                                        .next_charged(
                                            &mut visited_items,
                                            "STEP optional collection traversal",
                                        )?
                                        .ok_or_else(|| {
                                            CodecError::malformed(
                                                "STEP PMI traversal source ended early",
                                            )
                                        })?;
                                    annotation_storage.with_storage(|| {
                                        ctx.insert_btree_set(
                                            &mut annotation_indices,
                                            index,
                                            "step_pmi_usage_annotation_indices",
                                        )
                                    })?;
                                }
                            }
                        }
                    }
                }
            }
            if annotation_indices.is_empty() {
                continue;
            }
            let (targets_buffer, _target_storage) = ctx
                .with_scoped_storage("STEP geometric usage target scratch", || {
                    topology_targets(identified_item, topology, geometry_sources, ctx)
                })?;
            let targets = targets_buffer;
            if targets.is_empty() {
                continue;
            }
            {
                let mut visited_items = annotation_indices.into_iter();
                ctx.charge_work(0, "STEP pmi annotation_indices traversal")?;
                for _ in 0..visited_items.len() {
                    let annotation_index = ctx
                        .next_charged(&mut visited_items, "STEP pmi annotation_indices traversal")?
                        .ok_or_else(|| {
                            CodecError::malformed("STEP PMI traversal source ended early")
                        })?;
                    let annotation = &mut ir.model.pmi[annotation_index.get()];
                    ctx.charge_work(0, "STEP resolve geometric item usages traversal")?;
                    let mut pmi_source = targets[..].iter();
                    for _ in 0..pmi_source.len() {
                        let target = ctx
                            .next_charged(
                                &mut pmi_source,
                                "STEP resolve geometric item usages traversal",
                            )?
                            .ok_or_else(|| {
                                CodecError::malformed("STEP PMI traversal source ended early")
                            })?;
                        let seen = target_index(
                            &mut target_indices,
                            &mut target_storage,
                            annotation_index.get(),
                            &annotation.targets,
                            ctx,
                        )?;
                        push_target(
                            (seen, &mut target_storage),
                            &mut annotation.targets,
                            target_key(target),
                            || copy_pmi_target(target, ctx, "step_pmi_geometric_usage_identity"),
                            ctx,
                            "step_pmi_geometric_usage_targets",
                        )?;
                    }
                }
            }
            drop(annotation_storage);
            claim_storage
                .with_storage(|| ctx.insert_btree_set(typed, id, "step_pmi_typed_claims"))?;
        }
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
    let mut scratch = ctx.reserve_scoped(0, "STEP PMI topology target index")?;
    let mut seen: TargetIndex = std::array::from_fn(|_| BTreeSet::new());
    if let Some(items) = ctx.get_btree_map(
        &topology.body_by_root,
        &id,
        "STEP pmi topology.body_by_root get",
    )? {
        ctx.charge_work(0, "STEP optional collection traversal")?;
        let mut pmi_source = items.iter();
        for _ in 0..pmi_source.len() {
            let body = ctx
                .next_charged(&mut pmi_source, "STEP optional collection traversal")?
                .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
            push_target(
                (&mut seen, &mut scratch),
                &mut targets,
                (0, body.as_str()),
                || {
                    Ok(PmiTarget::Body {
                        body: body.try_clone_for_decode(ctx, "step_pmi_topology_identity")?,
                    })
                },
                ctx,
                "step_pmi_topology_targets",
            )?;
        }
    }
    if let Some(items) = ctx.get_btree_map(
        &topology.faces_by_source,
        &id,
        "STEP pmi topology.faces_by_source get",
    )? {
        ctx.charge_work(0, "STEP optional collection traversal")?;
        let mut pmi_source = items.iter();
        for _ in 0..pmi_source.len() {
            let face = ctx
                .next_charged(&mut pmi_source, "STEP optional collection traversal")?
                .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
            push_target(
                (&mut seen, &mut scratch),
                &mut targets,
                (1, face.as_str()),
                || {
                    Ok(PmiTarget::Face {
                        face: face.try_clone_for_decode(ctx, "step_pmi_topology_identity")?,
                    })
                },
                ctx,
                "step_pmi_topology_targets",
            )?;
        }
    }
    if let Some(items) = ctx.get_btree_map(
        &topology.edges_by_source,
        &id,
        "STEP pmi topology.edges_by_source get",
    )? {
        ctx.charge_work(0, "STEP optional collection traversal")?;
        let mut pmi_source = items.iter();
        for _ in 0..pmi_source.len() {
            let edge = ctx
                .next_charged(&mut pmi_source, "STEP optional collection traversal")?
                .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
            push_target(
                (&mut seen, &mut scratch),
                &mut targets,
                (2, edge.as_str()),
                || {
                    Ok(PmiTarget::Edge {
                        edge: edge.try_clone_for_decode(ctx, "step_pmi_topology_identity")?,
                    })
                },
                ctx,
                "step_pmi_topology_targets",
            )?;
        }
    }
    if let Some(items) = ctx.get_btree_map(
        &topology.vertices_by_source,
        &id,
        "STEP pmi topology.vertices_by_source get",
    )? {
        ctx.charge_work(0, "STEP optional collection traversal")?;
        let mut pmi_source = items.iter();
        for _ in 0..pmi_source.len() {
            let vertex = ctx
                .next_charged(&mut pmi_source, "STEP optional collection traversal")?
                .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
            push_target(
                (&mut seen, &mut scratch),
                &mut targets,
                (3, vertex.as_str()),
                || {
                    Ok(PmiTarget::Vertex {
                        vertex: vertex.try_clone_for_decode(ctx, "step_pmi_topology_identity")?,
                    })
                },
                ctx,
                "step_pmi_topology_targets",
            )?;
        }
    }
    if let Some(items) = ctx.get_btree_map(
        geometry_sources.points,
        &id,
        "STEP pmi geometry_sources.points get",
    )? {
        ctx.charge_work(0, "STEP optional collection traversal")?;
        let mut pmi_source = items.iter();
        for _ in 0..pmi_source.len() {
            let point = ctx
                .next_charged(&mut pmi_source, "STEP optional collection traversal")?
                .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
            push_target(
                (&mut seen, &mut scratch),
                &mut targets,
                (4, point.as_str()),
                || {
                    Ok(PmiTarget::Point {
                        point: point.try_clone_for_decode(ctx, "step_pmi_topology_identity")?,
                    })
                },
                ctx,
                "step_pmi_topology_targets",
            )?;
        }
    }
    if let Some(items) = ctx.get_btree_map(
        geometry_sources.curves,
        &id,
        "STEP pmi geometry_sources.curves get",
    )? {
        ctx.charge_work(0, "STEP optional collection traversal")?;
        let mut pmi_source = items.iter();
        for _ in 0..pmi_source.len() {
            let curve = ctx
                .next_charged(&mut pmi_source, "STEP optional collection traversal")?
                .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
            push_target(
                (&mut seen, &mut scratch),
                &mut targets,
                (5, curve.as_str()),
                || {
                    Ok(PmiTarget::Curve {
                        curve: curve.try_clone_for_decode(ctx, "step_pmi_topology_identity")?,
                    })
                },
                ctx,
                "step_pmi_topology_targets",
            )?;
        }
    }
    Ok(targets)
}

fn target_key(target: &PmiTarget) -> (u8, &str) {
    match target {
        PmiTarget::Body { body } => (0, body.as_str()),
        PmiTarget::Face { face } => (1, face.as_str()),
        PmiTarget::Edge { edge } => (2, edge.as_str()),
        PmiTarget::Vertex { vertex } => (3, vertex.as_str()),
        PmiTarget::Point { point } => (4, point.as_str()),
        PmiTarget::Curve { curve } => (5, curve.as_str()),
        PmiTarget::Product { product } => (6, product.as_str()),
        PmiTarget::Occurrence { occurrence } => (7, occurrence.as_str()),
        PmiTarget::ShapeAspect { source_id } => (8, source_id.as_str()),
    }
}

// Each lane corresponds to one PmiTarget variant; identities compare within that lane.
type TargetIndex = [BTreeSet<String>; 9];

fn target_index<'a>(
    indices: &'a mut BTreeMap<usize, TargetIndex>,
    storage: &mut ScopedReservation<'_>,
    index: usize,
    targets: &[PmiTarget],
    ctx: &DecodeContext<'_>,
) -> Result<&'a mut TargetIndex, CodecError> {
    match storage
        .with_storage(|| ctx.entry_btree_map(indices, index, "STEP PMI target index groups"))?
    {
        std::collections::btree_map::Entry::Occupied(entry) => Ok(entry.into_mut()),
        std::collections::btree_map::Entry::Vacant(entry) => {
            let mut keys: TargetIndex = std::array::from_fn(|_| BTreeSet::new());
            ctx.charge_work(0, "STEP PMI existing target traversal")?;
            let mut pmi_source = targets.iter();
            for _ in 0..pmi_source.len() {
                let target = ctx
                    .next_charged(&mut pmi_source, "STEP PMI existing target traversal")?
                    .ok_or_else(|| {
                        CodecError::malformed("STEP PMI traversal source ended early")
                    })?;
                let (lane, identity) = target_key(target);
                storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut keys[usize::from(lane)],
                        ctx.copy_retained_text(identity, "STEP PMI target index identity")?,
                        "STEP PMI target index members",
                    )
                })?;
            }
            Ok(entry.insert(keys))
        }
    }
}

fn push_target(
    (seen, storage): (&mut TargetIndex, &mut ScopedReservation<'_>),
    targets: &mut Vec<PmiTarget>,
    (lane, identity): (u8, &str),
    make_target: impl FnOnce() -> Result<PmiTarget, CodecError>,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<(), CodecError> {
    let seen = &mut seen[usize::from(lane)];
    if !ctx.contains_btree_set(seen, identity, "STEP PMI target identity membership")? {
        ctx.reserve_vec(targets, 1, operation)?;
        let target = make_target()?;
        storage.with_storage(|| {
            let key = ctx.copy_retained_text(identity, "STEP PMI target index identity")?;
            ctx.insert_btree_set(seen, key, "STEP PMI target identity index")
        })?;
        targets.push(target);
    }
    Ok(())
}

fn relationship_endpoints(
    record: &RawRecord,
    ctx: &DecodeContext<'_>,
) -> Result<Option<(u64, u64)>, CodecError> {
    let Some(parameters) = ctx.find_map(
        &(record.partials)[..],
        |partial| {
            Ok(matches!(
                partial.name.as_str(),
                "SHAPE_ASPECT_RELATIONSHIP" | "FEATURE_FOR_DATUM_TARGET_RELATIONSHIP"
            )
            .then_some(partial.parameters.as_slice()))
        },
        "STEP relationship endpoints traversal",
    )?
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
    ctx.charge_work(0, "STEP point sources traversal")?;
    let mut pmi_source = ir.model.points[..].iter();
    for _ in 0..pmi_source.len() {
        let point = ctx
            .next_charged(&mut pmi_source, "STEP point sources traversal")?
            .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
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
    ctx.charge_work(0, "STEP curve sources traversal")?;
    let mut pmi_source = ir.model.curves[..].iter();
    for _ in 0..pmi_source.len() {
        let curve = ctx
            .next_charged(&mut pmi_source, "STEP curve sources traversal")?
            .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
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
    (value, precedence): (&Value, NonZeroU32),
    exchange: &Exchange,
    annotations: &Annotations,
    (typed, claim_storage): (&mut BTreeSet<u64>, &mut ScopedReservation<'_>),
    measurements: &mut MeasureContext<'_, '_>,
    (output, output_storage): (&mut Vec<DatumReference>, &mut ScopedReservation<'_>),
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let Some(compartment_id) = value.reference() else {
        return Ok(());
    };
    let Some(compartment) =
        ctx.get_btree_map(exchange.records(), &compartment_id, "STEP pmi record get")?
    else {
        return Ok(());
    };
    if compartment
        .partial(ctx, "DATUM_REFERENCE_COMPARTMENT")?
        .is_none()
        && compartment
            .partial(ctx, "DATUM_REFERENCE_ELEMENT")?
            .is_none()
    {
        return Ok(());
    }
    claim_storage
        .with_storage(|| ctx.insert_btree_set(typed, compartment_id, "step_pmi_typed_claims"))?;
    let (compartment_modifiers_buffer, mut modifier_storage) =
        ctx.temporary_vec(0, "step_pmi_datum_modifier_items")?;
    let mut compartment_modifiers = compartment_modifiers_buffer;
    ctx.charge_work(0, "STEP datum modifier traversal")?;
    let mut pmi_source = datum_modifiers(ctx, compartment)?
        .and_then(ValueExt::list)
        .unwrap_or_default()
        .iter();
    for _ in 0..pmi_source.len() {
        let modifier = ctx
            .next_charged(&mut pmi_source, "STEP datum modifier traversal")?
            .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
        if let Some(text) = modifier_text(
            modifier,
            exchange,
            (typed, claim_storage),
            measurements,
            &mut modifier_storage,
            ctx,
        )? {
            ctx.push_scoped_vec(
                &mut modifier_storage,
                &mut compartment_modifiers,
                text,
                "step_pmi_datum_modifier_items",
            )?;
        }
    }
    let base = datum_base(ctx, compartment)?;
    if is_common_datum_list(base) {
        let Some(Value::Typed(_, members)) = base else {
            return Ok(());
        };
        let members = members.list().unwrap_or_default();
        let mut member_count = 0;
        let common_group = ctx
            .any_by(
                members,
                |member| {
                    member_count += usize::from(member.reference().is_some());
                    Ok(member_count >= 2)
                },
                "STEP datum common group member search",
            )?
            .then_some(precedence.get());
        ctx.charge_work(0, "STEP datum references for compartment traversal")?;
        let mut members = members.iter();
        for _ in 0..members.len() {
            let member = ctx
                .next_charged(
                    &mut members,
                    "STEP datum references for compartment traversal",
                )?
                .ok_or_else(|| CodecError::malformed("STEP datum member source ended early"))?;
            let Some(element_id) = member.reference() else {
                continue;
            };
            let Some(element) =
                ctx.get_btree_map(exchange.records(), &element_id, "STEP pmi record get")?
            else {
                continue;
            };
            if element.partial(ctx, "DATUM_REFERENCE_ELEMENT")?.is_none() {
                continue;
            }
            let Some(datum) = datum_base(ctx, element)?.and_then(ValueExt::reference) else {
                continue;
            };
            if annotations.get(datum)?.is_none() {
                continue;
            }
            let mut modifiers = output_storage.with_storage(|| {
                ctx.try_collect_vec(
                    compartment_modifiers
                        .iter()
                        .map(|value| ctx.copy_retained_text(value, "step_pmi_datum_modifier_copy")),
                    "step_pmi_datum_modifier_items",
                )
            })?;
            ctx.charge_work(0, "STEP datum modifier traversal")?;
            let mut pmi_source = datum_modifiers(ctx, element)?
                .and_then(ValueExt::list)
                .unwrap_or_default()
                .iter();
            for _ in 0..pmi_source.len() {
                let modifier = ctx
                    .next_charged(&mut pmi_source, "STEP datum modifier traversal")?
                    .ok_or_else(|| {
                        CodecError::malformed("STEP PMI traversal source ended early")
                    })?;
                if let Some(text) = modifier_text(
                    modifier,
                    exchange,
                    (typed, claim_storage),
                    measurements,
                    output_storage,
                    ctx,
                )? {
                    ctx.push_scoped_vec(
                        output_storage,
                        &mut modifiers,
                        text,
                        "step_pmi_datum_modifier_items",
                    )?;
                }
            }
            for claim in [element_id, datum] {
                claim_storage
                    .with_storage(|| ctx.insert_btree_set(typed, claim, "step_pmi_typed_claims"))?;
            }
            ctx.push_scoped_vec(
                output_storage,
                output,
                DatumReference {
                    datum: pmi_id(datum),
                    precedence,
                    common_group,
                    modifiers,
                },
                "step_pmi_datum_reference_items",
            )?;
        }
        return Ok(());
    }
    if let Some(base) = base {
        visit_datum_ids(base, ctx, &mut |datum| {
            if annotations.get(datum)?.is_none() {
                return Ok(());
            }
            claim_storage
                .with_storage(|| ctx.insert_btree_set(typed, datum, "step_pmi_typed_claims"))?;
            let modifiers = output_storage.with_storage(|| {
                ctx.try_collect_vec(
                    compartment_modifiers
                        .iter()
                        .map(|value| ctx.copy_retained_text(value, "step_pmi_datum_modifier_copy")),
                    "step_pmi_datum_modifier_items",
                )
            })?;
            ctx.push_scoped_vec(
                output_storage,
                output,
                DatumReference {
                    datum: pmi_id(datum),
                    precedence,
                    common_group: None,
                    modifiers,
                },
                "step_pmi_datum_reference_items",
            )
        })?;
    }
    Ok(())
}

fn datum_base<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
) -> Result<Option<&'a Value>, CodecError> {
    Ok(record
        .partial(ctx, "GENERAL_DATUM_REFERENCE")?
        .and_then(|partial| partial.parameters.first())
        .or_else(|| record.parameter(4)))
}

fn datum_modifiers<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
) -> Result<Option<&'a Value>, CodecError> {
    Ok(record
        .partial(ctx, "GENERAL_DATUM_REFERENCE")?
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
            ctx.charge_work(0, "STEP visit datum ids value traversal")?;
            let mut pmi_source = values.as_slice().iter();
            for _ in 0..pmi_source.len() {
                let value = ctx
                    .next_charged(&mut pmi_source, "STEP visit datum ids value traversal")?
                    .ok_or_else(|| {
                        CodecError::malformed("STEP PMI traversal source ended early")
                    })?;
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
    (typed, claim_storage): (&mut BTreeSet<u64>, &mut ScopedReservation<'_>),
    measurements: &mut MeasureContext<'_, '_>,
    storage: &mut ScopedReservation<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<String>, CodecError> {
    let _nested = ctx.enter_nested("step_pmi_datum_modifier_walk")?;
    match value {
        Value::Enumeration(value) => {
            let mut text = storage
                .with_storage(|| ctx.copy_retained_text(value, "step_pmi_datum_modifier_text"))?;
            ctx.make_ascii_lowercase(&mut text, "STEP PMI text case conversion")?;
            Ok(Some(text))
        }
        Value::Typed(_, value) => {
            ctx.charge_work(1, "STEP typed datum modifier descent")?;
            modifier_text(
                value,
                exchange,
                (typed, claim_storage),
                measurements,
                storage,
                ctx,
            )
        }
        Value::Reference(id) => {
            let Some(record) = ctx.get_btree_map(exchange.records(), id, "STEP pmi record get")?
            else {
                return Ok(None);
            };
            let Some(parameters) = record.partial(ctx, "DATUM_REFERENCE_MODIFIER_WITH_VALUE")?
            else {
                return Ok(None);
            };
            let parameters = parameters.parameters.as_slice();
            claim_storage
                .with_storage(|| ctx.insert_btree_set(typed, *id, "step_pmi_typed_claims"))?;
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
            claim_storage.with_storage(|| {
                ctx.insert_btree_set(typed, measure_id, "step_pmi_typed_claims")
            })?;
            let mut text = storage.with_storage(|| {
                ctx.format_retained(
                    format_args!("{kind}:{value}"),
                    "step_pmi_datum_modifier_value_text",
                )
            })?;
            ctx.make_ascii_lowercase(&mut text, "STEP PMI text case conversion")?;
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
    ctx.find_map(
        &record.partials[..],
        |partial| Ok(is_presentation_annotation(&partial.name).then_some(partial.name.as_str())),
        "STEP presentation annotation name traversal",
    )
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
    {
        let mut visited_items = exchange.records().iter();
        ctx.charge_work(0, "STEP hidden presentation annotation ids map traversal")?;
        for _ in 0..visited_items.len() {
            let (_, record) = ctx
                .next_charged(
                    &mut visited_items,
                    "STEP hidden presentation annotation ids map traversal",
                )?
                .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
            let Some(items) = record
                .partial(ctx, "INVISIBILITY")?
                .and_then(|partial| partial.parameters.first())
            else {
                continue;
            };
            for target in references(items, ctx) {
                let target = target?;
                if let Some(record) =
                    ctx.get_btree_map(exchange.records(), &target, "STEP pmi record get")?
                {
                    if is_supported_invisibility_target(ctx, record)? {
                        ctx.insert_btree_set(
                            &mut hidden,
                            target,
                            "step_pmi_hidden_annotation_ids",
                        )?;
                    }
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
    let has_annotation_text = ctx.any_by(
        &(record.partials)[..],
        |partial| {
            Ok(partial.name == "ANNOTATION_TEXT"
                || partial.name == "ANNOTATION_TEXT_CHARACTER"
                || partial.name.starts_with("ANNOTATION_TEXT_WITH_"))
        },
        "STEP collect typed placement candidates traversal",
    )?;
    ctx.charge_work(0, "STEP collect typed placement candidates traversal")?;
    let mut pmi_source = record.partials[..].iter();
    for _ in 0..pmi_source.len() {
        let partial = ctx
            .next_charged(
                &mut pmi_source,
                "STEP collect typed placement candidates traversal",
            )?
            .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
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
        ctx.charge_work(0, "STEP collect typed placement candidates traversal")?;
        let mut parameters = partial.parameters.iter();
        for _ in 0..parameters.len() {
            let value = ctx
                .next_charged(
                    &mut parameters,
                    "STEP collect typed placement candidates traversal",
                )?
                .ok_or_else(|| {
                    CodecError::malformed("STEP placement parameter source ended early")
                })?;
            for reference in references(value, ctx) {
                let reference = reference?;
                if let Some(&(origin, z_axis, x_axis)) = ctx.get_btree_map(
                    &geometry.placements,
                    &reference,
                    "STEP pmi geometry.placements get",
                )? {
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
    }
    Ok(())
}

fn find_annotation_text(
    id: u64,
    exchange: &Exchange,
    visited: &mut BTreeSet<u64>,
    (used, claim_storage): (&mut BTreeSet<u64>, &mut ScopedReservation<'_>),
    (losses, slot_storage): (
        &mut Vec<LossNote>,
        &std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'_>>,
    ),
    depth: usize,
    ctx: &DecodeContext<'_>,
) -> Result<Option<String>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "STEP annotation text scratch")?;
    let mut candidates = BTreeMap::new();
    collect_annotation_text(
        id,
        exchange,
        visited,
        (&mut candidates, &mut storage),
        (losses, slot_storage),
        depth,
        ctx,
    )?;
    match candidates.len() {
        0 => Ok(None),
        1 => {
            let (&text_id, text) = candidates.first_key_value().expect("one text candidate");
            claim_storage.with_storage(|| {
                ctx.insert_btree_set(used, text_id, "step_pmi_annotation_text_used")
            })?;
            Ok(Some(ctx.copy_retained_text(text, "step_string_text")?))
        }
        count => {
            ctx.push_scoped_vec(&mut slot_storage.borrow_mut(), losses, StepLossCode::PresentationAnnotationTextUnordered.note(format!(
                "presentation annotation #{id} has {count} reachable text carriers with no ordered composition"
            )), "step_pmi_losses")?;
            Ok(None)
        }
    }
}

fn collect_annotation_text(
    id: u64,
    exchange: &Exchange,
    visited: &mut BTreeSet<u64>,
    (candidates, storage): (&mut BTreeMap<u64, String>, &mut ScopedReservation<'_>),
    (losses, slot_storage): (
        &mut Vec<LossNote>,
        &std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'_>>,
    ),
    depth: usize,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    if depth >= 256 || ctx.contains_btree_set(visited, &id, "STEP pmi visited contains")? {
        return Ok(());
    }
    let _depth_guard = ctx.enter_nested("step_pmi_annotation_text_walk")?;
    storage
        .with_storage(|| ctx.insert_btree_set(visited, id, "step_pmi_annotation_text_visited"))?;
    let Some(record) = ctx.get_btree_map(exchange.records(), &id, "STEP pmi record get")? else {
        return Ok(());
    };
    if let Some(value) = record
        .partial(ctx, "TEXT_LITERAL")?
        .and_then(|partial| partial.parameters.first())
        .map_or_else(
            || -> Result<_, CodecError> {
                Ok(record
                    .partial(ctx, "TEXT_LITERAL_WITH_ASSOCIATED_CURVES")?
                    .and_then(|partial| partial.parameters.first()))
            },
            |value| Ok(Some(value)),
        )?
    {
        if let Some(text) = decode_text_scoped(
            exchange,
            value,
            (losses, slot_storage),
            id,
            ("PMI annotation text", StepLossCode::MetadataStringInvalid),
            ctx,
            storage,
        )? {
            storage.with_storage(|| {
                ctx.insert_btree_map(candidates, id, text, "step_pmi_annotation_text_candidates")
            })?;
        }
    }
    ctx.charge_work(0, "STEP PMI record partial traversal")?;
    let mut pmi_source = record.partials[..].iter();
    for _ in 0..pmi_source.len() {
        let partial = ctx
            .next_charged(&mut pmi_source, "STEP PMI record partial traversal")?
            .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
        ctx.charge_work(0, "STEP PMI record parameter traversal")?;
        let mut pmi_source = partial.parameters.as_slice().iter();
        for _ in 0..pmi_source.len() {
            let value = ctx
                .next_charged(&mut pmi_source, "STEP PMI record parameter traversal")?
                .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
            for reference in references(value, ctx) {
                let reference = reference?;
                collect_annotation_text(
                    reference,
                    exchange,
                    visited,
                    (candidates, storage),
                    (losses, slot_storage),
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
        || ctx
            .get_btree_map(visited, &id, "STEP pmi visited get")?
            .is_some_and(|visited_depth| *visited_depth <= depth)
    {
        return Ok(());
    }
    let _nested = ctx.enter_nested("step_pmi_placement_walk")?;
    ctx.insert_btree_map(visited, id, depth, "step_pmi_placement_visited")?;
    let Some(record) = ctx.get_btree_map(exchange.records(), &id, "STEP pmi record get")? else {
        return Ok(());
    };
    collect_typed_placement_candidates(record, geometry, candidates, ctx)?;
    ctx.charge_work(0, "STEP PMI record partial traversal")?;
    let mut pmi_source = record.partials[..].iter();
    for _ in 0..pmi_source.len() {
        let partial = ctx
            .next_charged(&mut pmi_source, "STEP PMI record partial traversal")?
            .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
        ctx.charge_work(0, "STEP PMI record parameter traversal")?;
        let mut pmi_source = partial.parameters.as_slice().iter();
        for _ in 0..pmi_source.len() {
            let value = ctx
                .next_charged(&mut pmi_source, "STEP PMI record parameter traversal")?
                .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
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
    let mut storage = ctx.reserve_scoped(0, "STEP target identity scratch")?;
    let mut seen = BTreeSet::new();
    let mut targets = Vec::new();
    for id in ids {
        push_shape_aspect_target(id?, &mut seen, &mut storage, &mut targets, ctx)?;
    }
    Ok(targets)
}

fn push_shape_aspect_target(
    id: u64,
    seen: &mut BTreeSet<u64>,
    storage: &mut ScopedReservation<'_>,
    targets: &mut Vec<PmiTarget>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    if ctx.contains_btree_set(seen, &id, "STEP pmi seen contains")? {
        return Ok(());
    }
    storage.with_storage(|| ctx.insert_btree_set(seen, id, "step_pmi_target_ids"))?;
    ctx.reserve_vec(targets, 1, "step_pmi_target_items")?;
    targets.push(PmiTarget::ShapeAspect {
        source_id: super::step_source_id(ctx, id)?,
    });
    Ok(())
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
    let form = value.trim();
    if form.eq_ignore_ascii_case("point") {
        Ok(DatumTargetForm::Point)
    } else if form.eq_ignore_ascii_case("line") {
        Ok(DatumTargetForm::Line)
    } else if form.eq_ignore_ascii_case("rectangle") {
        Ok(DatumTargetForm::Rectangle)
    } else if form.eq_ignore_ascii_case("circle") {
        Ok(DatumTargetForm::Circle)
    } else if form.eq_ignore_ascii_case("circular curve") {
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
    Ok(record
        .partial(ctx, "DATUM_TARGET")?
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
    if let Some(partial) = record.partial(ctx, "SHAPE_ASPECT")? {
        Ok(partial.parameters.get(index))
    } else {
        Ok(record.parameter(index))
    }
}

fn is_measure_record(ctx: &DecodeContext<'_>, record: &RawRecord) -> Result<bool, CodecError> {
    ctx.any_by(
        &record.partials[..],
        |partial| {
            Ok(partial.name == "MEASURE_REPRESENTATION_ITEM"
                || partial.name == "MEASURE_WITH_UNIT"
                || partial.name.ends_with("_MEASURE_WITH_UNIT"))
        },
        "STEP measure record classification traversal",
    )
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
            ctx.make_ascii_lowercase(&mut name, "STEP PMI text case conversion")?;
            Some(DimensionKind::Other(name))
        }
        _ => None,
    })
}

fn dimension_descriptor<'a>(
    record: &'a RawRecord,
    ctx: &DecodeContext<'_>,
) -> Result<Option<(&'a str, DimensionKind)>, CodecError> {
    ctx.find_map(
        &record.partials[..],
        |partial| {
            Ok(dimension_kind(partial.name.as_str(), ctx)?
                .map(|kind| (partial.name.as_str(), kind)))
        },
        "STEP dimension descriptor traversal",
    )
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
    if let Some(partial) = record.partial(ctx, "GEOMETRIC_TOLERANCE_WITH_MODIFIERS")? {
        ctx.charge_work(0, "STEP tolerance modifiers traversal")?;
        let mut pmi_source = partial.parameters[..].iter();
        for _ in 0..pmi_source.len() {
            let value = ctx
                .next_charged(&mut pmi_source, "STEP tolerance modifiers traversal")?
                .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
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
            ctx.make_ascii_lowercase(&mut text, "STEP PMI text case conversion")?;
            ctx.push_vec(output, text, "step_pmi_modifier_items")?;
        }
        Value::List(values) => {
            ctx.charge_work(0, "STEP modifier values value traversal")?;
            let mut pmi_source = values.as_slice().iter();
            for _ in 0..pmi_source.len() {
                let value = ctx
                    .next_charged(&mut pmi_source, "STEP modifier values value traversal")?
                    .ok_or_else(|| {
                        CodecError::malformed("STEP PMI traversal source ended early")
                    })?;
                modifier_values(value, output, ctx)?;
            }
        }
        Value::Typed(_, value) => {
            ctx.charge_work(1, "STEP typed modifier descent")?;
            modifier_values(value, output, ctx)?;
        }
        _ => {}
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum NominalSelection {
    Absent,
    Unique(PmiValue),
    NamedAmbiguous(usize),
    UnnamedAmbiguous(usize),
}

struct CharacteristicAnalysis {
    selection: NominalSelection,
    losses: Vec<LossNote>,
}

fn characteristic_values(
    exchange: &Exchange,
    geometry: &GeometryData,
    (losses, slot_storage): (
        &mut Vec<LossNote>,
        &std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'_>>,
    ),
    graph_limit: usize,
    storage: &mut ScopedReservation<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<u64, PmiValue>, CodecError> {
    let mut result = BTreeMap::<u64, PmiValue>::new();
    let mut analyses = BTreeMap::new();
    let mut analysis_storage = ctx.reserve_scoped(0, "STEP characteristic analysis scratch")?;
    for indexed_entity in exchange.entities(ctx, "DIMENSIONAL_CHARACTERISTIC_REPRESENTATION")? {
        let (id, record) = indexed_entity?;
        let measurements = measure_context(geometry, id, (losses, slot_storage), graph_limit, ctx)?;
        let Some(characteristic) = find_record_value(record, ctx, |value| {
            first_matching([value], ctx, |id| {
                Ok({
                    ctx.get_btree_map(exchange.records(), &id, "STEP pmi record get")?
                        .map(|record| {
                            ctx.any_by(
                                &record.partials[..],
                                |partial| Ok(is_dimension_name(&partial.name)),
                                "STEP characteristic dimension partial traversal",
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
                    ctx.get_btree_map(exchange.records(), &id, "STEP pmi record get")?
                        .map(|record| {
                            Ok::<_, CodecError>(
                                record
                                    .partial(ctx, "SHAPE_DIMENSION_REPRESENTATION")?
                                    .is_some(),
                            )
                        })
                        .transpose()?
                        .unwrap_or(false)
                })
            })
        })?;
        let representation_items = if let Some(record) = representation
            .map(|id| ctx.get_btree_map(exchange.records(), &id, "STEP pmi record get"))
            .transpose()?
            .flatten()
        {
            record
                .partial(ctx, "SHAPE_DIMENSION_REPRESENTATION")?
                .and_then(|partial| partial.parameters.get(1))
                .and_then(ValueExt::list)
        } else {
            None
        };
        let parameters = representation_items
            .map_or(MeasureParameters::Record(record), MeasureParameters::Items);
        let key = (
            (
                representation_items.and(representation).unwrap_or(id),
                representation_items.is_some(),
            ),
            measurements.length_scale.to_bits(),
            measurements.angle_scale.to_bits(),
            graph_limit,
        );
        if !ctx.contains_key_btree_map(&analyses, &key, "STEP characteristic analysis lookup")? {
            let mut value_storage = ctx.reserve_scoped(0, "STEP characteristic value scratch")?;
            let cached_reports = std::cell::RefCell::new(
                ctx.reserve_scoped(0, "STEP characteristic cached reports")?,
            );
            let mut cached_losses = Vec::new();
            let mut cached_measurements = MeasureContext {
                length_scale: measurements.length_scale,
                angle_scale: measurements.angle_scale,
                graph_limit,
                losses: (&mut cached_losses, &cached_reports),
            };
            let values = analysis_storage.with_storage(|| {
                characteristic_measure_values(
                    &parameters,
                    exchange,
                    &mut cached_measurements,
                    &mut value_storage,
                    ctx,
                )
            })?;
            let mut named_count = 0usize;
            let mut named_first = None;
            for (name, value) in ctx.admit_iter(&values, "STEP characteristic values traversal")? {
                if name
                    .as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case("nominal value"))
                {
                    named_count += 1;
                    if named_count == 1 {
                        named_first = Some(*value);
                    }
                }
            }
            let selection = if named_count == 1 {
                NominalSelection::Unique(named_first.expect("one named nominal"))
            } else if named_count > 1 {
                NominalSelection::NamedAmbiguous(named_count)
            } else {
                match values.as_slice() {
                    [] => NominalSelection::Absent,
                    [(_, value)] => NominalSelection::Unique(*value),
                    values => NominalSelection::UnnamedAmbiguous(values.len()),
                }
            };
            let analysis = CharacteristicAnalysis {
                selection,
                losses: cached_losses,
            };
            drop(values);
            drop(value_storage);
            analysis_storage.absorb(&mut cached_reports.into_inner())?;
            analysis_storage.with_storage(|| {
                ctx.insert_btree_map(&mut analyses, key, analysis, "STEP characteristic analyses")
            })?;
        }
        let analysis = ctx
            .get_btree_map(&analyses, &key, "STEP characteristic analysis lookup")?
            .ok_or_else(|| CodecError::malformed("STEP characteristic analysis is missing"))?;
        for loss in ctx.admit_iter(&analysis.losses, "STEP characteristic diagnostic replay")? {
            let loss = loss.try_clone_for_decode(ctx, "STEP characteristic cached loss copy")?;
            ctx.push_scoped_vec(
                &mut slot_storage.borrow_mut(),
                losses,
                loss,
                "step_pmi_losses",
            )?;
        }
        let selected = match analysis.selection {
            NominalSelection::Absent => None,
            NominalSelection::Unique(value) => Some(value),
            NominalSelection::NamedAmbiguous(count) => {
                ctx.push_scoped_vec(&mut slot_storage.borrow_mut(), losses, StepLossCode::DimensionalNominalAmbiguous.note(format!(
                    "DIMENSIONAL_CHARACTERISTIC_REPRESENTATION #{id} has {count} nominal value measures; the nominal is ambiguous"
                )), "step_pmi_losses")?;
                None
            }
            NominalSelection::UnnamedAmbiguous(count) => {
                ctx.push_scoped_vec(&mut slot_storage.borrow_mut(), losses, StepLossCode::DimensionalUnnamedMeasureAmbiguous.note(format!(
                    "DIMENSIONAL_CHARACTERISTIC_REPRESENTATION #{id} has {count} unnamed measure values; the nominal is ambiguous"
                )), "step_pmi_losses")?;
                None
            }
        };
        if let Some(selected) = selected {
            storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut result,
                    characteristic,
                    selected,
                    "step_pmi_characteristic_values",
                )
            })?;
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
                ctx.charge_work(0, "STEP measure item traversal")?;
                let mut pmi_source = items.iter();
                for _ in 0..pmi_source.len() {
                    let value = ctx
                        .next_charged(&mut pmi_source, "STEP measure item traversal")?
                        .ok_or_else(|| {
                            CodecError::malformed("STEP PMI traversal source ended early")
                        })?;
                    visitor(value)?;
                }
            }
            Self::Record(record) => {
                ctx.charge_work(0, "STEP PMI record partial traversal")?;
                let mut pmi_source = record.partials[..].iter();
                for _ in 0..pmi_source.len() {
                    let partial = ctx
                        .next_charged(&mut pmi_source, "STEP PMI record partial traversal")?
                        .ok_or_else(|| {
                            CodecError::malformed("STEP PMI traversal source ended early")
                        })?;
                    ctx.charge_work(0, "STEP PMI record parameter traversal")?;
                    let mut pmi_source = partial.parameters.as_slice().iter();
                    for _ in 0..pmi_source.len() {
                        let value = ctx
                            .next_charged(&mut pmi_source, "STEP PMI record parameter traversal")?
                            .ok_or_else(|| {
                                CodecError::malformed("STEP PMI traversal source ended early")
                            })?;
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
    measurements: &mut MeasureContext<'_, '_>,
    storage: &mut ScopedReservation<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<(Option<String>, PmiValue)>, CodecError> {
    let mut measure_ids = BTreeSet::new();
    let mut visited = BTreeMap::new();
    parameters.visit(ctx, |parameter| {
        storage.with_storage(|| {
            collect_measure_ids(
                parameter,
                exchange,
                &mut visited,
                0,
                measurements.graph_limit,
                &mut measure_ids,
                ctx,
            )
        })
    })?;
    let mut values = Vec::new();
    {
        let mut visited_items = measure_ids.into_iter();
        ctx.charge_work(0, "STEP pmi measure_ids traversal")?;
        for _ in 0..visited_items.len() {
            let id = ctx
                .next_charged(&mut visited_items, "STEP pmi measure_ids traversal")?
                .ok_or_else(|| CodecError::malformed("STEP PMI traversal source ended early"))?;
            if let Some(value) = measure(&Value::Reference(id), exchange, measurements, ctx)? {
                let name = ctx
                    .get_btree_map(exchange.records(), &id, "STEP pmi record get")?
                    .map(|record| {
                        measure_item_name(
                            id,
                            record,
                            exchange,
                            (measurements.losses.0, measurements.losses.1),
                            (ctx, storage),
                        )
                    })
                    .transpose()?
                    .flatten();
                storage
                    .with_storage(|| ctx.reserve_vec(&mut values, 1, "step_pmi_measure_values"))?;
                values.push((name, value));
            }
        }
    }
    if values.is_empty() {
        parameters.visit(ctx, |parameter| {
            if let Some(value) = measure(parameter, exchange, measurements, ctx)? {
                storage
                    .with_storage(|| ctx.reserve_vec(&mut values, 1, "step_pmi_measure_values"))?;
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
    visited: &mut BTreeMap<u64, usize>,
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
            if ctx
                .get_btree_map(visited, id, "STEP measure visited depth lookup")?
                .is_some_and(|prior| *prior <= depth)
            {
                return Ok(());
            }
            ctx.insert_btree_map(visited, *id, depth, "step_pmi_measure_visited_ids")?;
            if let Some(record) =
                ctx.get_btree_map(exchange.records(), id, "STEP pmi record get")?
            {
                if is_measure_record(ctx, record)? {
                    ctx.insert_btree_set(measure_ids, *id, "step_pmi_measure_ids")?;
                } else {
                    ctx.charge_work(0, "STEP collect measure ids traversal")?;
                    let mut pmi_source = record.partials[..].iter();
                    for _ in 0..pmi_source.len() {
                        let partial = ctx
                            .next_charged(&mut pmi_source, "STEP collect measure ids traversal")?
                            .ok_or_else(|| {
                                CodecError::malformed("STEP PMI traversal source ended early")
                            })?;
                        ctx.charge_work(0, "STEP collect measure ids traversal")?;
                        let mut pmi_source = partial.parameters[..].iter();
                        for _ in 0..pmi_source.len() {
                            let parameter = ctx
                                .next_charged(
                                    &mut pmi_source,
                                    "STEP collect measure ids traversal",
                                )?
                                .ok_or_else(|| {
                                    CodecError::malformed("STEP PMI traversal source ended early")
                                })?;
                            collect_measure_ids(
                                parameter,
                                exchange,
                                visited,
                                depth + 1,
                                graph_limit,
                                measure_ids,
                                ctx,
                            )?;
                        }
                    }
                }
            }
        }
        Value::List(values) => {
            ctx.charge_work(0, "STEP collect measure ids value traversal")?;
            let mut pmi_source = values.as_slice().iter();
            for _ in 0..pmi_source.len() {
                let value = ctx
                    .next_charged(&mut pmi_source, "STEP collect measure ids value traversal")?
                    .ok_or_else(|| {
                        CodecError::malformed("STEP PMI traversal source ended early")
                    })?;
                collect_measure_ids(
                    value,
                    exchange,
                    visited,
                    depth + 1,
                    graph_limit,
                    measure_ids,
                    ctx,
                )?;
            }
        }
        Value::Typed(_, value) => {
            ctx.charge_work(1, "STEP typed measure ID descent")?;
            collect_measure_ids(
                value,
                exchange,
                visited,
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
    (losses, slot_storage): (
        &mut Vec<LossNote>,
        &std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'_>>,
    ),
    (ctx, storage): (&DecodeContext<'_>, &mut ScopedReservation<'_>),
) -> Result<Option<String>, CodecError> {
    Ok(record
        .partial(ctx, "REPRESENTATION_ITEM")?
        .and_then(|partial| partial.parameters.first())
        .map_or_else(
            || {
                Ok::<_, CodecError>(
                    record
                        .partial(ctx, "MEASURE_REPRESENTATION_ITEM")?
                        .and_then(|partial| partial.parameters.first()),
                )
            },
            |value| Ok(Some(value)),
        )?
        .map(|value| {
            decode_text_scoped(
                exchange,
                value,
                (losses, slot_storage),
                id,
                ("measure item name", StepLossCode::MetadataStringInvalid),
                ctx,
                storage,
            )
        })
        .transpose()?
        .flatten()
        .filter(|name| !name.is_empty()))
}

fn measure_context<'a, 'ctx>(
    geometry: &GeometryData,
    id: u64,
    losses: (
        &'a mut Vec<LossNote>,
        &'a std::cell::RefCell<ScopedReservation<'ctx>>,
    ),
    graph_limit: usize,
    ctx: &DecodeContext<'_>,
) -> Result<MeasureContext<'a, 'ctx>, CodecError> {
    Ok(MeasureContext {
        length_scale: geometry.units.length([id], ctx)?.get(),
        angle_scale: geometry.units.angle([id], ctx)?.get(),
        graph_limit,
        losses,
    })
}

fn first_measure(
    values: &[Value],
    exchange: &Exchange,
    measurements: &mut MeasureContext<'_, '_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PmiValue>, CodecError> {
    ctx.find_map(
        values,
        |value| measure(value, exchange, measurements, ctx),
        "STEP first measure traversal",
    )
}

fn measure(
    value: &Value,
    exchange: &Exchange,
    measurements: &mut MeasureContext<'_, '_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PmiValue>, CodecError> {
    let mut walk = MeasureWalk::new(ctx)?;
    measure_inner(value, exchange, &mut walk, 0, measurements, ctx)
}

// Record walks have at most 256 active value depths.
const MEASURE_DEPTH_WORDS: usize = super::MAX_RECORD_GRAPH_DEPTH / 64;

#[derive(Clone, Copy)]
struct MeasureFailure {
    blocked: [u64; MEASURE_DEPTH_WORDS],
    // This frame identifies the entire active prefix on which a failure depends.
    anchor: Option<(usize, u64)>,
}

struct MeasureWalk<'ctx> {
    active: BTreeMap<u64, usize>,
    complete: BTreeMap<(u64, usize), MeasureFailure>,
    frames: [u64; super::MAX_RECORD_GRAPH_DEPTH],
    next_frame: u64,
    blocked: [u64; MEASURE_DEPTH_WORDS],
    storage: ScopedReservation<'ctx>,
}

impl<'ctx> MeasureWalk<'ctx> {
    fn new(ctx: &'ctx DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            active: BTreeMap::new(),
            complete: BTreeMap::new(),
            frames: [0; super::MAX_RECORD_GRAPH_DEPTH],
            next_frame: 0,
            blocked: [0; MEASURE_DEPTH_WORDS],
            storage: ctx.reserve_scoped(0, "STEP measure completion scratch")?,
        })
    }

    fn merge_blocked(&mut self, blocked: [u64; MEASURE_DEPTH_WORDS]) {
        for (target, source) in self.blocked.iter_mut().zip(blocked) {
            *target |= source;
        }
    }
}

fn measure_inner(
    value: &Value,
    exchange: &Exchange,
    walk: &mut MeasureWalk<'_>,
    depth: usize,
    measurements: &mut MeasureContext<'_, '_>,
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
        Value::Typed(name, value) => {
            if let Some(number) = value.number() {
                let (quantity, scale) = if ctx
                    .position_by(
                        name.as_bytes().windows(b"LENGTH".len()),
                        |window| Ok(window == &b"LENGTH"[..]),
                        "STEP PMI typed length containment",
                    )?
                    .is_some()
                {
                    (PmiQuantity::Length, measurements.length_scale)
                } else if ctx
                    .position_by(
                        name.as_bytes().windows(b"ANGLE".len()),
                        |window| Ok(window == &b"ANGLE"[..]),
                        "STEP PMI typed angle containment",
                    )?
                    .is_some()
                {
                    (PmiQuantity::Angle, measurements.angle_scale)
                } else {
                    (PmiQuantity::Ratio, 1.0)
                };
                PmiValue::new(number * scale, quantity)
            } else {
                None
            }
        }
        Value::Reference(id) => {
            if let Some(&active_depth) =
                ctx.get_btree_map(&walk.active, id, "STEP pmi active contains")?
            {
                walk.blocked[active_depth / 64] |= 1 << (active_depth % 64);
                return Ok(None);
            }
            if let Some(result) = ctx.get_btree_map(
                &walk.complete,
                &(*id, depth),
                "STEP measure completion lookup",
            )? {
                if result
                    .anchor
                    .is_none_or(|(depth, frame)| walk.frames[depth] == frame)
                {
                    let result = *result;
                    walk.merge_blocked(result.blocked);
                    return Ok(None);
                }
            }
            let loss_start = measurements.losses.0.len();
            let prior_blocked = std::mem::take(&mut walk.blocked);
            // Every frame creation follows admitted keyed work, so the session
            // work limit bounds this monotonically increasing identity.
            walk.next_frame += 1;
            walk.frames[depth] = walk.next_frame;
            let (_inserted, active_storage) =
                ctx.with_scoped_storage("STEP active key scratch", || {
                    ctx.insert_btree_map(
                        &mut walk.active,
                        *id,
                        depth,
                        "step_pmi_measure_eval_active",
                    )
                })?;
            let Some(record) = ctx.get_btree_map(exchange.records(), id, "STEP pmi record get")?
            else {
                ctx.remove_btree_map(&mut walk.active, id, "STEP pmi active remove")?;
                walk.frames[depth] = 0;
                walk.merge_blocked(prior_blocked);
                return Ok(None);
            };
            let quantity =
                find_record_value(record, ctx, |parameter| measure_quantity(parameter, ctx))?;
            let quantity = if let Some(quantity) = quantity {
                quantity
            } else if ctx
                .find_map(
                    &record.partials[..],
                    |partial| -> Result<Option<()>, CodecError> {
                        Ok(ctx
                            .position_by(
                                partial.name.as_bytes().windows(b"LENGTH".len()),
                                |window| Ok(window == &b"LENGTH"[..]),
                                "STEP PMI record length containment",
                            )?
                            .is_some()
                            .then_some(()))
                    },
                    "STEP PMI length classifier traversal",
                )?
                .is_some()
            {
                PmiQuantity::Length
            } else if ctx
                .find_map(
                    &record.partials[..],
                    |partial| -> Result<Option<()>, CodecError> {
                        Ok(ctx
                            .position_by(
                                partial.name.as_bytes().windows(b"ANGLE".len()),
                                |window| Ok(window == &b"ANGLE"[..]),
                                "STEP PMI record angle containment",
                            )?
                            .is_some()
                            .then_some(()))
                    },
                    "STEP PMI angle classifier traversal",
                )?
                .is_some()
            {
                PmiQuantity::Angle
            } else {
                PmiQuantity::Ratio
            };
            let unit = find_record_value(record, ctx, |parameter| {
                let Some(candidate) = parameter.reference() else {
                    return Ok(None);
                };
                let Some(unit) = ctx.get_btree_map(
                    exchange.records(),
                    &candidate,
                    "STEP PMI measure unit lookup",
                )?
                else {
                    return Ok(None);
                };
                Ok((unit.partial(ctx, "LENGTH_UNIT")?.is_some()
                    || unit.partial(ctx, "PLANE_ANGLE_UNIT")?.is_some())
                .then_some(candidate))
            })?;
            let scale = match quantity {
                PmiQuantity::Length => {
                    let resolved = match unit {
                        Some(unit) => {
                            let (scale, _unit_storage) = ctx.with_scoped_storage(
                                "STEP measure unit resolver scratch",
                                || {
                                    super::geometry::unit_scale_mm(
                                        unit,
                                        exchange,
                                        &mut BTreeSet::new(),
                                        ctx,
                                    )
                                },
                            )?;
                            scale
                        }
                        None => None,
                    };
                    if let Some(scale) = resolved {
                        scale.get()
                    } else {
                        ctx.push_scoped_vec(&mut measurements.losses.1.borrow_mut(), measurements.losses.0, StepLossCode::PmiLengthUnitUnresolved.note(format!(
                                "PMI length measure #{id} unit scale did not resolve; the document length scale was used"
                            )), "step_pmi_losses")?;
                        measurements.length_scale
                    }
                }
                PmiQuantity::Angle => {
                    let resolved = match unit {
                        Some(unit) => {
                            let (scale, _unit_storage) = ctx.with_scoped_storage(
                                "STEP measure unit resolver scratch",
                                || {
                                    super::geometry::unit_scale_radians(
                                        unit,
                                        exchange,
                                        &mut BTreeSet::new(),
                                        ctx,
                                    )
                                },
                            )?;
                            scale
                        }
                        None => None,
                    };
                    if let Some(scale) = resolved {
                        scale.get()
                    } else {
                        ctx.push_scoped_vec(&mut measurements.losses.1.borrow_mut(), measurements.losses.0, StepLossCode::PmiAngleUnitUnresolved.note(format!(
                                "PMI angle measure #{id} unit scale did not resolve; the document plane-angle scale was used"
                            )), "step_pmi_losses")?;
                        measurements.angle_scale
                    }
                }
                PmiQuantity::Ratio => 1.0,
            };
            let result = find_record_value(record, ctx, |parameter| {
                let mut number = parameter;
                while let Value::Typed(_, inner) = number {
                    ctx.charge_work(1, "STEP PMI typed numeric step")?;
                    number = inner;
                }
                if let Some(value) = number
                    .number()
                    .and_then(|number| PmiValue::new(number * scale, quantity))
                {
                    Ok(Some(value))
                } else {
                    measure_inner(parameter, exchange, walk, depth + 1, measurements, ctx)
                }
            })?;
            ctx.remove_btree_map(&mut walk.active, id, "STEP pmi active remove")?;
            drop(active_storage);
            walk.frames[depth] = 0;
            if result.is_none() {
                // A fully explored failed record resolves cuts back to itself.
                // Cuts to outer frames still constrain reuse of its failure.
                walk.blocked[depth / 64] &= !(1 << (depth % 64));
            }
            if measurements.losses.0.len() == loss_start && result.is_none() {
                let anchor = walk
                    .blocked
                    .iter()
                    .enumerate()
                    .rev()
                    .find_map(|(word, bits)| {
                        if *bits == 0 {
                            None
                        } else {
                            let bit =
                                63 - cadmpeg_core::decode::index_from_u32(bits.leading_zeros());
                            let depth = word * 64 + bit;
                            Some((depth, walk.frames[depth]))
                        }
                    });
                walk.storage.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut walk.complete,
                        (*id, depth),
                        MeasureFailure {
                            blocked: walk.blocked,
                            anchor,
                        },
                        "step_measure_complete",
                    )
                })?;
            }
            walk.merge_blocked(prior_blocked);
            result
        }
        Value::List(values) => ctx.find_map(
            values.as_slice(),
            |value| measure_inner(value, exchange, walk, depth + 1, measurements, ctx),
            "STEP measure inner value traversal",
        )?,
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
            if ctx
                .position_by(
                    name.as_bytes().windows(b"LENGTH".len()),
                    |window| Ok(window == &b"LENGTH"[..]),
                    "STEP PMI typed length containment",
                )?
                .is_some()
            {
                Some(PmiQuantity::Length)
            } else if ctx
                .position_by(
                    name.as_bytes().windows(b"ANGLE".len()),
                    |window| Ok(window == &b"ANGLE"[..]),
                    "STEP PMI typed angle containment",
                )?
                .is_some()
            {
                Some(PmiQuantity::Angle)
            } else if ctx
                .position_by(
                    name.as_bytes().windows(b"RATIO".len()),
                    |window| Ok(window == &b"RATIO"[..]),
                    "STEP PMI typed ratio containment",
                )?
                .is_some()
            {
                Some(PmiQuantity::Ratio)
            } else {
                measure_quantity(value, ctx)?
            }
        }
        Value::List(values) => ctx.find_map(
            values.as_slice(),
            |value| measure_quantity(value, ctx),
            "STEP measure quantity value traversal",
        )?,
        _ => None,
    })
}

#[cfg(test)]
pub(crate) mod tests;
