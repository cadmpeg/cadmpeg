// SPDX-License-Identifier: Apache-2.0
//! STEP semantic product-manufacturing information.

use crate::ids::kind;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::num::NonZeroU32;

use super::reference::{first_matching, references};
use super::{named_parameter, record_values, source_numeric_id, RecordExt, ValueExt};
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

use super::decode_text;
use super::geometry::GeometryData;
use super::topology::TopologyData;
use super::StageOutcome;

mod annotations;

use annotations::{AnnotationDraft, AnnotationIndex, Annotations};

struct MeasureContext<'a> {
    length_scale: f64,
    angle_scale: f64,
    losses: &'a mut Vec<LossNote>,
}

fn collect_pmi_references(
    values: &[Value],
    ctx: &DecodeContext<'_>,
) -> Result<Vec<u64>, CodecError> {
    let mut ids = Vec::new();
    for value in values {
        for id in references(value, ctx) {
            let id = id?;
            ids.push(id);
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
    if !exchange.has_entity_matching(is_pmi_entity_name) {
        return Ok(StageOutcome {
            value: (),
            claims: HashSet::new(),
            losses: Vec::new(),
            notes: Vec::new(),
        });
    }
    let base_aspects = exchange
        .entities_any(&["SHAPE_ASPECT", "DATUM_FEATURE", "DATUM"])
        .map(|(id, _)| id)
        .collect::<std::collections::BTreeSet<_>>();
    let shape_aspects = exchange
        .matching_entity_ids(is_shape_aspect_name)
        .collect::<std::collections::BTreeSet<_>>();
    let mut typed = HashSet::new();
    let mut losses = Vec::new();
    let mut annotations = Annotations::default();
    let hidden_presentation_annotations = hidden_presentation_annotation_ids(exchange, ctx)?;

    let mut presentation_semantics = BTreeMap::<u64, Vec<u64>>::new();
    let characteristic_values = characteristic_values(exchange, geometry, &mut losses, ctx)?;
    for (id, record) in exchange.entities("DATUM") {
        let identification = named_parameter(record, "DATUM", 0)
            .and_then(|value| {
                decode_text(
                    exchange,
                    value,
                    &mut losses,
                    id,
                    "datum identification",
                    StepLossCode::MetadataStringInvalid,
                )
            })
            .unwrap_or_else(|| format!("#{id}"));
        annotations.push(
            ir,
            id,
            AnnotationDraft {
                name: shape_aspect_parameter(record, 0).and_then(|value| {
                    decode_text(
                        exchange,
                        value,
                        &mut losses,
                        id,
                        "datum name",
                        StepLossCode::MetadataStringInvalid,
                    )
                }),
                targets: targets([Ok(id)])?,
                visible: None,
                definition: PmiDefinition::Datum { identification },
            },
        );
        typed.insert(id);
    }

    for id in exchange.matching_entity_ids(is_datum_target_name) {
        let Some(record) = exchange.records().get(&id) else {
            continue;
        };
        let form = shape_aspect_parameter(record, 1)
            .and_then(|value| {
                decode_text(
                    exchange,
                    value,
                    &mut losses,
                    id,
                    "datum target form",
                    StepLossCode::MetadataStringInvalid,
                )
            })
            .unwrap_or_default();
        let identification = datum_target_identification_parameter(record)
            .and_then(|value| {
                decode_text(
                    exchange,
                    value,
                    &mut losses,
                    id,
                    "datum target identification",
                    StepLossCode::MetadataStringInvalid,
                )
            })
            .unwrap_or_else(|| format!("#{id}"));
        annotations.push(
            ir,
            id,
            AnnotationDraft {
                name: shape_aspect_parameter(record, 0).and_then(|value| {
                    decode_text(
                        exchange,
                        value,
                        &mut losses,
                        id,
                        "datum target name",
                        StepLossCode::MetadataStringInvalid,
                    )
                }),
                targets: targets([Ok(id)])?,
                visible: None,
                definition: PmiDefinition::DatumTarget {
                    form: datum_target_form(&form),
                    identification,
                    basis: Vec::new(),
                },
            },
        );
        typed.insert(id);
    }

    for (id, record) in exchange.entities("DATUM_SYSTEM") {
        let constituents = record
            .parameters()
            .iter()
            .rev()
            .find_map(ValueExt::list)
            .unwrap_or_default();
        let mut datum_records = HashSet::new();
        let mut measurements = measure_context(geometry, id, &mut losses);
        let mut datum_references = Vec::new();
        for (index, constituent) in constituents.iter().enumerate() {
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
                datum_references.push(reference);
            }
        }
        let datum_references = match datum_references.try_into() {
            Ok(references) => references,
            Err(error) => {
                losses.push(
                    StepLossCode::PmiDatumSystemInvalid
                        .note(format!("DATUM_SYSTEM #{id} omitted: {error}")),
                );
                continue;
            }
        };
        annotations.push(
            ir,
            id,
            AnnotationDraft {
                name: shape_aspect_parameter(record, 0).and_then(|value| {
                    decode_text(
                        exchange,
                        value,
                        &mut losses,
                        id,
                        "datum system name",
                        StepLossCode::MetadataStringInvalid,
                    )
                }),
                targets: targets(
                    record
                        .parameters()
                        .iter()
                        .flat_map(|value| references(value, ctx))
                        .filter(|id| match id {
                            Ok(id) => base_aspects.contains(id),
                            Err(_) => true,
                        }),
                )?,
                visible: None,
                definition: PmiDefinition::DatumSystem {
                    references: datum_references,
                },
            },
        );
        typed.insert(id);
        typed.extend(datum_records);
    }

    for id in exchange.matching_entity_ids(is_dimension_name) {
        let Some(record) = exchange.records().get(&id) else {
            continue;
        };
        let Some((dimension_name, mut kind)) = dimension_descriptor(record) else {
            continue;
        };
        let mut name = None;
        for value in record
            .partials
            .iter()
            .flat_map(|partial| &partial.parameters)
        {
            if let Some(text) = decode_text(
                exchange,
                value,
                &mut losses,
                id,
                "dimension name",
                StepLossCode::MetadataStringInvalid,
            ) {
                name = Some(text);
                break;
            }
        }
        if matches!(kind, DimensionKind::Size) {
            let category = if dimension_name.starts_with("DIMENSIONAL_SIZE_WITH_DATUM_FEATURE") {
                let mut category = None;
                for value in record
                    .partials
                    .iter()
                    .find(|partial| partial.name == dimension_name)
                    .into_iter()
                    .flat_map(|partial| partial.parameters.iter().rev())
                {
                    if let Some(text) = decode_text(
                        exchange,
                        value,
                        &mut losses,
                        id,
                        "dimension category",
                        StepLossCode::MetadataStringInvalid,
                    ) {
                        category = Some(text);
                        break;
                    }
                }
                category
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
        let nominal = characteristic_values.get(&id).copied();
        let definition = PmiDimension::new(kind, nominal, None)
            .map_err(|error| CodecError::malformed(format_args!("dimension #{id}: {error}")))?;
        let aspect_ids = record
            .partials
            .iter()
            .flat_map(|partial| &partial.parameters)
            .flat_map(|value| references(value, ctx))
            .filter(|reference| match reference {
                Ok(id) => shape_aspects.contains(id),
                Err(_) => true,
            });
        annotations.push(
            ir,
            id,
            AnnotationDraft {
                name,
                targets: targets(aspect_ids)?,
                visible: None,
                definition: PmiDefinition::Dimension(definition),
            },
        );
        typed.insert(id);
    }

    for (id, record) in exchange.entities("PLUS_MINUS_TOLERANCE") {
        let refs = collect_pmi_references(record.parameters(), ctx)?;
        let dimension = refs
            .iter()
            .find_map(|reference| annotations.get(*reference));
        let limits = refs.iter().find_map(|reference| {
            exchange
                .records()
                .get(reference)
                .filter(|candidate| candidate.simple_name() == Some("TOLERANCE_VALUE"))
        });
        let fit = refs
            .iter()
            .find_map(|reference| {
                let record = exchange.records().get(reference)?;
                (record.simple_name() == Some("LIMITS_AND_FITS")).then(
                    || -> Result<_, CodecError> {
                        Ok((
                            *reference,
                            LimitsAndFits {
                                form_variance: record
                                    .parameter(0)
                                    .and_then(|value| {
                                        decode_text(
                                            exchange,
                                            value,
                                            &mut losses,
                                            *reference,
                                            "limits-and-fits form variance",
                                            StepLossCode::MetadataStringInvalid,
                                        )
                                    })
                                    .unwrap_or_default(),
                                zone_variance: record
                                    .parameter(1)
                                    .and_then(|value| {
                                        decode_text(
                                            exchange,
                                            value,
                                            &mut losses,
                                            *reference,
                                            "limits-and-fits zone variance",
                                            StepLossCode::MetadataStringInvalid,
                                        )
                                    })
                                    .unwrap_or_default(),
                                grade: record
                                    .parameter(2)
                                    .and_then(|value| {
                                        decode_text(
                                            exchange,
                                            value,
                                            &mut losses,
                                            *reference,
                                            "limits-and-fits grade",
                                            StepLossCode::MetadataStringInvalid,
                                        )
                                    })
                                    .unwrap_or_default(),
                                source: record
                                    .parameter(3)
                                    .and_then(|value| {
                                        decode_text(
                                            exchange,
                                            value,
                                            &mut losses,
                                            *reference,
                                            "limits-and-fits source",
                                            StepLossCode::MetadataStringInvalid,
                                        )
                                    })
                                    .unwrap_or_default(),
                            },
                        ))
                    },
                )
            })
            .transpose()?;
        if let (Some(index), Some(limits)) = (dimension, limits) {
            let mut measurements = measure_context(geometry, id, &mut losses);
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
                    typed.insert(id);
                    typed.extend(refs);
                } else {
                    losses.push(StepLossCode::DecodeWarning.note(format!(
                        "PLUS_MINUS_TOLERANCE #{id} is an additional tolerance for one dimension"
                    )));
                }
            } else {
                losses.push(StepLossCode::DecodeWarning.note(format!(
                    "PLUS_MINUS_TOLERANCE #{id} does not contain both deviation values"
                )));
            }
        } else if let (Some(index), Some((fit_id, fit))) = (dimension, fit) {
            if set_dimension_tolerance(
                &mut ir.model.pmi[index.get()].definition,
                DimensionTolerance::Fit { fit },
            )
            .map_err(|error| {
                CodecError::malformed(format_args!("PLUS_MINUS_TOLERANCE #{id}: {error}"))
            })? {
                typed.extend([id, fit_id]);
            } else {
                losses.push(StepLossCode::DecodeWarning.note(format!(
                    "PLUS_MINUS_TOLERANCE #{id} is an additional tolerance for one dimension"
                )));
            }
        } else {
            losses.push(StepLossCode::DecodeWarning.note(format!(
                "PLUS_MINUS_TOLERANCE #{id} has no resolvable dimension and limits"
            )));
        }
    }

    for id in exchange.matching_entity_ids(|name| tolerance_kind(Some(name)).is_some()) {
        let Some(record) = exchange.records().get(&id) else {
            continue;
        };
        let Some(tolerance) = record
            .partials
            .iter()
            .find_map(|partial| {
                (partial.name != "GEOMETRIC_TOLERANCE")
                    .then(|| tolerance_kind(Some(&partial.name)))
                    .flatten()
            })
            .or_else(|| {
                record
                    .partials
                    .iter()
                    .find_map(|partial| tolerance_kind(Some(&partial.name)))
            })
        else {
            continue;
        };
        let reference_values = record
            .partials
            .iter()
            .find(|partial| partial.name == "GEOMETRIC_TOLERANCE")
            .map_or(record.parameters(), |partial| partial.parameters.as_slice());
        let refs = collect_pmi_references(reference_values, ctx)?;
        let mut measurements = measure_context(geometry, id, &mut losses);
        let magnitude = first_measure(
            record
                .partials
                .iter()
                .find(|partial| partial.name == "GEOMETRIC_TOLERANCE")
                .into_iter()
                .flat_map(|partial| partial.parameters.iter()),
            exchange,
            &mut measurements,
            ctx,
        )?;
        let magnitude = match magnitude {
            Some(magnitude) => Some(magnitude),
            None => first_measure(
                record
                    .partials
                    .iter()
                    .filter(|partial| partial.name != "GEOMETRIC_TOLERANCE")
                    .flat_map(|partial| partial.parameters.iter()),
                exchange,
                &mut measurements,
                ctx,
            )?,
        };
        let Some(magnitude) = magnitude.and_then(cadmpeg_ir::pmi::PmiMagnitude::new) else {
            let display_name = record
                .partials
                .iter()
                .map(|partial| partial.name.as_str())
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join("+");
            let message = format!("{display_name} #{id} has no numeric magnitude");
            losses.push(StepLossCode::DecodeWarning.note(message));
            continue;
        };
        let defined_unit = record
            .partials
            .iter()
            .find(|partial| partial.name == "GEOMETRIC_TOLERANCE_WITH_DEFINED_UNIT")
            .and_then(|partial| partial.parameters.first())
            .map(|value| measure(value, exchange, &mut measurements, ctx))
            .transpose()?
            .flatten();
        let (defined_area_unit, defined_area_second_unit) = if let Some(partial) = record
            .partials
            .iter()
            .find(|partial| partial.name == "GEOMETRIC_TOLERANCE_WITH_DEFINED_AREA_UNIT")
        {
            let area = partial
                .parameters
                .first()
                .and_then(ValueExt::enumeration)
                .map(str::to_ascii_lowercase);
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
            record
                .partials
                .iter()
                .find(|partial| partial.name == "GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE")
                .into_iter()
                .flat_map(|partial| partial.parameters.iter()),
            ctx,
            |id| {
                annotations.get(id).is_some_and(|index| {
                    matches!(
                        ir.model.pmi[index.get()].definition,
                        PmiDefinition::DatumSystem { .. }
                    )
                })
            },
        )?
        .and_then(|id| {
            annotations
                .get(id)
                .map(|index| &ir.model.pmi[index.get()].id)
        })
        .cloned();
        annotations.push(
            ir,
            id,
            AnnotationDraft {
                name: named_parameter(record, "GEOMETRIC_TOLERANCE", 0)
                    .or_else(|| record.parameter(0))
                    .and_then(|value| {
                        decode_text(
                            exchange,
                            value,
                            &mut losses,
                            id,
                            "geometric tolerance name",
                            StepLossCode::MetadataStringInvalid,
                        )
                    }),
                targets: targets(
                    refs.iter()
                        .copied()
                        .filter(|id| base_aspects.contains(id))
                        .map(Ok),
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
        );
        typed.insert(id);
        typed.extend(refs.iter().copied().filter(|reference| {
            exchange
                .records()
                .get(reference)
                .is_some_and(is_measure_record)
        }));
        for value in record
            .partials
            .iter()
            .flat_map(|partial| partial.parameters.iter())
        {
            for reference in references(value, ctx) {
                let reference = reference?;
                if exchange
                    .records()
                    .get(&reference)
                    .is_some_and(is_measure_record)
                {
                    typed.insert(reference);
                }
            }
        }
    }

    for (id, record) in exchange.entities("DRAUGHTING_MODEL_ITEM_ASSOCIATION") {
        let Some(definition) = named_parameter(record, "DRAUGHTING_MODEL_ITEM_ASSOCIATION", 2)
            .and_then(ValueExt::reference)
        else {
            continue;
        };
        if annotations.get(definition).is_some() {
            if let Some(items) = named_parameter(record, "DRAUGHTING_MODEL_ITEM_ASSOCIATION", 4) {
                for item in references(items, ctx) {
                    let item = item?;
                    presentation_semantics
                        .entry(item)
                        .or_default()
                        .push(definition);
                }
            }
            typed.insert(id);
        }
    }

    for id in exchange.matching_entity_ids(is_presentation_annotation) {
        let Some(record) = exchange.records().get(&id) else {
            continue;
        };
        let Some(name) = presentation_annotation_name(record) else {
            continue;
        };
        let mut text_records = BTreeSet::new();
        let text = find_annotation_text(
            id,
            exchange,
            &mut BTreeSet::new(),
            &mut text_records,
            &mut losses,
            ctx,
        )?;
        // Placement identity is the carrier key; the transform value cannot
        // make two source carriers one semantic carrier.
        let mut placement_candidates = BTreeMap::new();
        let mut placement_visited = BTreeMap::new();
        for parameter in record_values(record) {
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
        let placement = match placement_candidates.len() {
            0 => None,
            1 => placement_candidates.values().next().copied(),
            count => {
                losses.push(StepLossCode::PresentationAnnotationPlacementAmbiguous.note(
                    format!(
                        "presentation annotation #{id} has {count} reachable placement carriers with no unique placement"
                    ),
                ));
                None
            }
        };
        let mut semantics = Vec::new();
        for parameter in record_values(record) {
            for reference in references(parameter, ctx) {
                let reference = reference?;
                if annotations.get(reference).is_some() {
                    semantics.push(pmi_id(reference));
                }
            }
        }
        for semantic in presentation_semantics.get(&id).into_iter().flatten() {
            semantics.push(pmi_id(*semantic));
        }
        annotations.push(
            ir,
            id,
            AnnotationDraft {
                name: named_parameter(record, name, 0)
                    .or_else(|| named_parameter(record, "REPRESENTATION_ITEM", 0))
                    .or_else(|| record.parameter(0))
                    .and_then(|value| {
                        decode_text(
                            exchange,
                            value,
                            &mut losses,
                            id,
                            "presentation annotation name",
                            StepLossCode::MetadataStringInvalid,
                        )
                    }),
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
        );
        typed.insert(id);
        typed.extend(text_records);
    }
    for (id, _) in
        exchange.entities_any(&["DRAUGHTING_MODEL", "ANNOTATION_PLANE", "DRAUGHTING_CALLOUT"])
    {
        typed.insert(id);
    }

    resolve_feature_for_datum_target_relationships(exchange, &annotations, ir, &mut typed, ctx)?;
    let points_by_source = point_sources(ir);
    let curves_by_source = curve_sources(ir);
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

    let targeted_aspects = ir
        .model
        .pmi
        .iter()
        .flat_map(|annotation| &annotation.targets)
        .filter_map(|target| match target {
            PmiTarget::ShapeAspect { source_id } => {
                source_id.as_str().strip_prefix('#')?.parse().ok()
            }
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    typed.extend(shape_aspects.intersection(&targeted_aspects).copied());
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
    typed: &mut HashSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    for (id, record) in exchange.entities("DIMENSIONAL_CHARACTERISTIC_REPRESENTATION") {
        let Some(_) = first_matching(record_values(record), ctx, |reference| {
            annotations.get(reference).is_some()
        })?
        else {
            continue;
        };
        typed.insert(id);
        for parameter in record_values(record) {
            for representation_id in references(parameter, ctx) {
                let representation_id = representation_id?;
                let Some(representation) = exchange.records().get(&representation_id) else {
                    continue;
                };
                if !representation
                    .partials
                    .iter()
                    .any(|partial| partial.name == "SHAPE_DIMENSION_REPRESENTATION")
                {
                    continue;
                }
                typed.insert(representation_id);
                for parameter in record_values(representation) {
                    for reference in references(parameter, ctx) {
                        let reference = reference?;
                        if exchange
                            .records()
                            .get(&reference)
                            .is_some_and(is_measure_record)
                        {
                            typed.insert(reference);
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
    typed: &mut HashSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    for (id, record) in exchange.entities("FEATURE_FOR_DATUM_TARGET_RELATIONSHIP") {
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
                source_id: super::step_source_id(relating),
            },
        );
        typed.extend([id, relating]);
    }
    Ok(())
}

fn resolve_geometric_item_usages(
    exchange: &Exchange,
    topology: &TopologyData,
    geometry_sources: GeometrySources<'_>,
    (shape_aspects, annotations): (&BTreeSet<u64>, &Annotations),
    ir: &mut CadIr,
    typed: &mut HashSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let mut aspect_annotations = BTreeMap::<u64, BTreeSet<AnnotationIndex>>::new();
    for (&annotation_id, record) in exchange.records() {
        let Some(annotation_index) = annotations.get(annotation_id) else {
            continue;
        };
        if shape_aspects.contains(&annotation_id) {
            aspect_annotations
                .entry(annotation_id)
                .or_default()
                .insert(annotation_index);
        }
        for parameter in record_values(record) {
            for reference in references(parameter, ctx) {
                let reference = reference?;
                if shape_aspects.contains(&reference) {
                    aspect_annotations
                        .entry(reference)
                        .or_default()
                        .insert(annotation_index);
                }
            }
        }
    }

    let mut relationship_aspects = BTreeMap::<u64, BTreeSet<u64>>::new();
    for record in exchange.records().values() {
        let Some((relating, related)) = relationship_endpoints(record, ctx)? else {
            continue;
        };
        relationship_aspects
            .entry(relating)
            .or_default()
            .insert(related);
        relationship_aspects
            .entry(related)
            .or_default()
            .insert(relating);
    }

    for (&id, record) in exchange.records() {
        let Some(partial) = record
            .partials
            .iter()
            .find(|partial| partial.name == "GEOMETRIC_ITEM_SPECIFIC_USAGE")
        else {
            continue;
        };
        let Some(definition) = first_matching(partial.parameters.get(2), ctx, |_| true)? else {
            continue;
        };
        let Some(identified_item) = first_matching(partial.parameters.get(4), ctx, |_| true)?
        else {
            continue;
        };
        let mut annotation_indices = BTreeSet::new();
        for &index in aspect_annotations.get(&definition).into_iter().flatten() {
            annotation_indices.insert(index);
        }
        if let Some(aspects) = relationship_aspects.get(&definition) {
            for aspect in aspects {
                for &index in aspect_annotations.get(aspect).into_iter().flatten() {
                    annotation_indices.insert(index);
                }
            }
        }
        if annotation_indices.is_empty() {
            continue;
        }
        let targets = topology_targets(identified_item, topology, geometry_sources);
        if targets.is_empty() {
            continue;
        }
        for annotation_index in annotation_indices {
            let annotation = &mut ir.model.pmi[annotation_index.get()];
            for target in &targets {
                push_target(&mut annotation.targets, copy_pmi_target(target)?);
            }
        }
        typed.insert(id);
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
) -> std::vec::Vec<cadmpeg_ir::PmiTarget> {
    let mut targets = Vec::new();
    for body in topology.body_by_root.get(&id).into_iter().flatten() {
        let body = body.clone();
        push_target(&mut targets, PmiTarget::Body { body });
    }
    for face in topology.faces_by_source.get(&id).into_iter().flatten() {
        let face = face.clone();
        push_target(&mut targets, PmiTarget::Face { face });
    }
    for edge in topology.edges_by_source.get(&id).into_iter().flatten() {
        let edge = edge.clone();
        push_target(&mut targets, PmiTarget::Edge { edge });
    }
    for vertex in topology.vertices_by_source.get(&id).into_iter().flatten() {
        let vertex = vertex.clone();
        push_target(&mut targets, PmiTarget::Vertex { vertex });
    }
    for point in geometry_sources.points.get(&id).into_iter().flatten() {
        let point = point.clone();
        push_target(&mut targets, PmiTarget::Point { point });
    }
    for curve in geometry_sources.curves.get(&id).into_iter().flatten() {
        let curve = curve.clone();
        push_target(&mut targets, PmiTarget::Curve { curve });
    }
    targets
}

fn push_target(targets: &mut Vec<PmiTarget>, target: PmiTarget) {
    if !targets.contains(&target) {
        targets.push(target);
    }
}

fn relationship_endpoints(
    record: &RawRecord,
    ctx: &DecodeContext<'_>,
) -> Result<Option<(u64, u64)>, CodecError> {
    let Some(parameters) = record.partials.iter().find_map(|partial| {
        matches!(
            partial.name.as_str(),
            "SHAPE_ASPECT_RELATIONSHIP" | "FEATURE_FOR_DATUM_TARGET_RELATIONSHIP"
        )
        .then_some(partial.parameters.as_slice())
    }) else {
        return Ok(None);
    };
    let Some(relating) = first_matching(parameters.get(2), ctx, |_| true)? else {
        return Ok(None);
    };
    let Some(related) = first_matching(parameters.get(3), ctx, |_| true)? else {
        return Ok(None);
    };
    Ok(Some((relating, related)))
}

fn point_sources(
    ir: &CadIr,
) -> std::collections::BTreeMap<u64, std::vec::Vec<cadmpeg_ir::ids::PointId>> {
    let mut points = BTreeMap::<u64, Vec<_>>::new();
    for point in &ir.model.points {
        let Some(source) = source_numeric_id(point.id.as_str(), "point") else {
            continue;
        };
        let id = point.id.clone();
        points.entry(source).or_default().push(id);
    }
    points
}

fn curve_sources(
    ir: &CadIr,
) -> std::collections::BTreeMap<u64, std::vec::Vec<cadmpeg_ir::ids::CurveId>> {
    let mut curves = BTreeMap::<u64, Vec<_>>::new();
    for curve in &ir.model.curves {
        let Some(source) = source_numeric_id(curve.id.as_str(), "curve") else {
            continue;
        };
        let id = curve.id.clone();
        curves.entry(source).or_default().push(id);
    }
    curves
}

fn datum_references_for_compartment(
    value: &Value,
    precedence: NonZeroU32,
    exchange: &Exchange,
    annotations: &Annotations,
    typed: &mut HashSet<u64>,
    measurements: &mut MeasureContext<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<DatumReference>, CodecError> {
    let Some(compartment_id) = value.reference() else {
        return Ok(Vec::new());
    };
    let Some(compartment) = exchange.records().get(&compartment_id) else {
        return Ok(Vec::new());
    };
    if compartment.partial("DATUM_REFERENCE_COMPARTMENT").is_none()
        && compartment.partial("DATUM_REFERENCE_ELEMENT").is_none()
    {
        return Ok(Vec::new());
    }
    typed.insert(compartment_id);
    let mut compartment_modifiers = Vec::new();
    for modifier in datum_modifiers(compartment)
        .and_then(ValueExt::list)
        .into_iter()
        .flatten()
    {
        if let Some(text) = modifier_text(modifier, exchange, typed, measurements, ctx)? {
            compartment_modifiers.push(text);
        }
    }
    let base = datum_base(compartment);
    let mut output = Vec::new();
    if is_common_datum_list(base) {
        let Some(Value::Typed(_, members)) = base else {
            return Ok(output);
        };
        let members = members.list().unwrap_or_default();
        let common_group = (members.iter().filter_map(ValueExt::reference).count() >= 2)
            .then_some(precedence.get());
        for element_id in members.iter().filter_map(ValueExt::reference) {
            let Some(element) = exchange.records().get(&element_id) else {
                continue;
            };
            if element.partial("DATUM_REFERENCE_ELEMENT").is_none() {
                continue;
            }
            let Some(datum) = datum_base(element).and_then(ValueExt::reference) else {
                continue;
            };
            if annotations.get(datum).is_none() {
                continue;
            }
            let mut modifiers = compartment_modifiers
                .iter()
                .map(std::borrow::ToOwned::to_owned)
                .collect::<Vec<_>>();
            for modifier in datum_modifiers(element)
                .and_then(ValueExt::list)
                .into_iter()
                .flatten()
            {
                if let Some(text) = modifier_text(modifier, exchange, typed, measurements, ctx)? {
                    modifiers.push(text);
                }
            }
            typed.extend([element_id, datum]);
            output.push(DatumReference {
                datum: pmi_id(datum),
                precedence,
                common_group,
                modifiers,
            });
        }
        return Ok(output);
    }
    if let Some(base) = base {
        visit_datum_ids(base, ctx, &mut |datum| {
            if annotations.get(datum).is_none() {
                return Ok(());
            }
            typed.insert(datum);
            output.push(DatumReference {
                datum: pmi_id(datum),
                precedence,
                common_group: None,
                modifiers: compartment_modifiers.clone(),
            });
            Ok(())
        })?;
    }
    Ok(output)
}

fn datum_base(record: &RawRecord) -> Option<&Value> {
    record
        .partials
        .iter()
        .find(|partial| partial.name == "GENERAL_DATUM_REFERENCE")
        .and_then(|partial| partial.parameters.first())
        .or_else(|| record.parameter(4))
}

fn datum_modifiers(record: &RawRecord) -> Option<&Value> {
    record
        .partials
        .iter()
        .find(|partial| partial.name == "GENERAL_DATUM_REFERENCE")
        .and_then(|partial| partial.parameters.get(1))
        .or_else(|| record.parameter(5))
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
            for value in values {
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
    typed: &mut HashSet<u64>,
    measurements: &mut MeasureContext<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<String>, CodecError> {
    let _nested = ctx.enter_nested("step_pmi_datum_modifier_walk")?;
    match value {
        Value::Enumeration(value) => {
            let mut text = value.to_owned();
            text.make_ascii_lowercase();
            Ok(Some(text))
        }
        Value::Typed(_, value) => modifier_text(value, exchange, typed, measurements, ctx),
        Value::Reference(id) => {
            let Some(record) = exchange.records().get(id) else {
                return Ok(None);
            };
            let Some(parameters) = record
                .partials
                .iter()
                .find(|partial| partial.name == "DATUM_REFERENCE_MODIFIER_WITH_VALUE")
            else {
                return Ok(None);
            };
            let parameters = parameters.parameters.as_slice();
            typed.insert(*id);
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
            typed.insert(measure_id);
            let mut text = format!("{kind}:{value}");
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

fn presentation_annotation_name(record: &RawRecord) -> Option<&str> {
    record.partials.iter().find_map(|partial| {
        is_presentation_annotation(&partial.name).then_some(partial.name.as_str())
    })
}

pub(super) fn is_supported_invisibility_target(record: &RawRecord) -> bool {
    presentation_annotation_name(record).is_some()
}

fn hidden_presentation_annotation_ids(
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeSet<u64>, CodecError> {
    let mut hidden = BTreeSet::new();
    for record in exchange.records().values() {
        let Some(items) = record
            .partials
            .iter()
            .find(|partial| partial.name == "INVISIBILITY")
            .and_then(|partial| partial.parameters.first())
        else {
            continue;
        };
        for target in references(items, ctx) {
            let target = target?;
            if exchange
                .records()
                .get(&target)
                .is_some_and(is_supported_invisibility_target)
            {
                hidden.insert(target);
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
    let has_annotation_text = record.partials.iter().any(|partial| {
        partial.name == "ANNOTATION_TEXT"
            || partial.name == "ANNOTATION_TEXT_CHARACTER"
            || partial.name.starts_with("ANNOTATION_TEXT_WITH_")
    });
    for partial in &record.partials {
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
        for reference in partial
            .parameters
            .iter()
            .flat_map(|value| references(value, ctx))
        {
            let reference = reference?;
            if let Some(&(origin, z_axis, x_axis)) = geometry.placements.get(&reference) {
                if let Some(transform) =
                    super::geometry::placement_transform((origin, z_axis, x_axis))
                {
                    candidates.insert(reference, transform);
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
    ctx: &DecodeContext<'_>,
) -> Result<Option<String>, CodecError> {
    let mut candidates = BTreeMap::new();
    collect_annotation_text(id, exchange, visited, &mut candidates, losses, ctx)?;
    let Some((text_id, text)) = candidates.pop_first() else {
        return Ok(None);
    };
    if candidates.is_empty() {
        used.insert(text_id);
        Ok(Some(text))
    } else {
        let count = candidates.len() + 1;
        losses.push(StepLossCode::PresentationAnnotationTextUnordered.note(format!(
                    "presentation annotation #{id} has {count} reachable text carriers with no ordered composition"
                )));
        Ok(None)
    }
}

fn collect_annotation_text(
    id: u64,
    exchange: &Exchange,
    visited: &mut BTreeSet<u64>,
    candidates: &mut BTreeMap<u64, String>,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    if visited.contains(&id) {
        return Ok(());
    }
    let _depth_guard = ctx.enter_nested("step_pmi_annotation_text_walk")?;
    visited.insert(id);
    let Some(record) = exchange.records().get(&id) else {
        return Ok(());
    };
    if let Some(value) = named_parameter(record, "TEXT_LITERAL", 0)
        .or_else(|| named_parameter(record, "TEXT_LITERAL_WITH_ASSOCIATED_CURVES", 0))
    {
        if let Some(text) = decode_text(
            exchange,
            value,
            losses,
            id,
            "PMI annotation text",
            StepLossCode::MetadataStringInvalid,
        ) {
            candidates.insert(id, text);
        }
    }
    for reference in record_values(record).flat_map(|value| references(value, ctx)) {
        let reference = reference?;
        collect_annotation_text(reference, exchange, visited, candidates, losses, ctx)?;
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
    // Retain the shortest depth so the traversal is independent of aggregate
    // member order when a graph has alternate paths.
    if visited
        .get(&id)
        .is_some_and(|visited_depth| *visited_depth <= depth)
    {
        return Ok(());
    }
    let _nested = ctx.enter_nested("step_pmi_placement_walk")?;
    visited.insert(id, depth);
    if let Some(record) = exchange.records().get(&id) {
        collect_typed_placement_candidates(record, geometry, candidates, ctx)?;
    }
    let Some(record) = exchange.records().get(&id) else {
        return Ok(());
    };
    for reference in record_values(record).flat_map(|value| references(value, ctx)) {
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
    Ok(())
}

fn targets(
    ids: impl IntoIterator<Item = Result<u64, CodecError>>,
) -> Result<Vec<PmiTarget>, CodecError> {
    let mut seen = BTreeSet::new();
    let mut targets = Vec::new();
    for id in ids {
        let id = id?;
        if seen.contains(&id) {
            continue;
        }
        seen.insert(id);

        targets.push(PmiTarget::ShapeAspect {
            source_id: super::step_source_id(id),
        });
    }
    Ok(targets)
}

fn pmi_id(id: u64) -> PmiId {
    PmiId::from(ids::presentation(kind!("pmi"), id))
}

fn copy_pmi_target(target: &PmiTarget) -> Result<PmiTarget, CodecError> {
    Ok(match target {
        PmiTarget::Body { body } => PmiTarget::Body { body: body.clone() },
        PmiTarget::Face { face } => PmiTarget::Face { face: face.clone() },
        PmiTarget::Edge { edge } => PmiTarget::Edge { edge: edge.clone() },
        PmiTarget::Vertex { vertex } => PmiTarget::Vertex {
            vertex: vertex.clone(),
        },
        PmiTarget::Point { point } => PmiTarget::Point {
            point: point.clone(),
        },
        PmiTarget::Curve { curve } => PmiTarget::Curve {
            curve: curve.clone(),
        },
        PmiTarget::Product { product } => PmiTarget::Product {
            product: product.clone(),
        },
        PmiTarget::Occurrence { occurrence } => PmiTarget::Occurrence {
            occurrence: occurrence.clone(),
        },
        PmiTarget::ShapeAspect { source_id } => {
            let copy = source_id.as_str().to_owned();
            let source_id = cadmpeg_core::text::NonBlankString::new(copy)
                .ok_or_else(|| CodecError::malformed("STEP PMI target has a blank source ID"))?;
            PmiTarget::ShapeAspect { source_id }
        }
    })
}

fn datum_target_form(value: &str) -> cadmpeg_ir::DatumTargetForm {
    let form = value.trim();
    if form.eq_ignore_ascii_case("point") {
        DatumTargetForm::Point
    } else if form.eq_ignore_ascii_case("line") {
        DatumTargetForm::Line
    } else if form.eq_ignore_ascii_case("rectangle") {
        DatumTargetForm::Rectangle
    } else if form.eq_ignore_ascii_case("circle") {
        DatumTargetForm::Circle
    } else if form.eq_ignore_ascii_case("circular curve") {
        DatumTargetForm::CircularCurve
    } else {
        DatumTargetForm::Other(value.to_owned())
    }
}

fn datum_target_identification_parameter(record: &RawRecord) -> Option<&Value> {
    record
        .partials
        .iter()
        .find(|partial| partial.name == "DATUM_TARGET")
        .and_then(|partial| partial.parameters.last())
        .or_else(|| record.parameter(4))
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

fn shape_aspect_parameter(record: &RawRecord, index: usize) -> Option<&Value> {
    if let Some(partial) = record
        .partials
        .iter()
        .find(|partial| partial.name == "SHAPE_ASPECT")
    {
        partial.parameters.get(index)
    } else {
        record.parameter(index)
    }
}

fn is_measure_record(record: &RawRecord) -> bool {
    record.partials.iter().any(|partial| {
        partial.name == "MEASURE_REPRESENTATION_ITEM"
            || partial.name == "MEASURE_WITH_UNIT"
            || partial.name.ends_with("_MEASURE_WITH_UNIT")
    })
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

fn dimension_kind(name: &str) -> Option<cadmpeg_ir::DimensionKind> {
    match name {
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
            let mut name = name.to_owned();
            name.make_ascii_lowercase();
            Some(DimensionKind::Other(name))
        }
        _ => None,
    }
}

fn dimension_descriptor(record: &RawRecord) -> Option<(&str, cadmpeg_ir::DimensionKind)> {
    for partial in &record.partials {
        if let Some(kind) = dimension_kind(partial.name.as_str()) {
            return Some((partial.name.as_str(), kind));
        }
    }
    None
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
    if let Some(partial) = record
        .partials
        .iter()
        .find(|partial| partial.name == "GEOMETRIC_TOLERANCE_WITH_MODIFIERS")
    {
        for value in &partial.parameters {
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
            let mut text = value.to_owned();
            text.make_ascii_lowercase();
            output.push(text);
        }
        Value::List(values) => {
            for value in values {
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
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<u64, PmiValue>, CodecError> {
    let mut result = BTreeMap::<u64, PmiValue>::new();
    for (id, record) in exchange.entities("DIMENSIONAL_CHARACTERISTIC_REPRESENTATION") {
        let mut measurements = measure_context(geometry, id, losses);
        let Some(characteristic) = first_matching(record_values(record), ctx, |id| {
            exchange.records().get(&id).is_some_and(|record| {
                record
                    .partials
                    .iter()
                    .any(|partial| is_dimension_name(&partial.name))
            })
        })?
        else {
            continue;
        };
        let representation = first_matching(record_values(record), ctx, |id| {
            exchange.records().get(&id).is_some_and(|record| {
                record
                    .partials
                    .iter()
                    .any(|partial| partial.name == "SHAPE_DIMENSION_REPRESENTATION")
            })
        })?;
        let representation_items = representation
            .and_then(|id| exchange.records().get(&id))
            .and_then(|record| {
                record
                    .partials
                    .iter()
                    .find(|partial| partial.name == "SHAPE_DIMENSION_REPRESENTATION")
                    .and_then(|partial| partial.parameters.get(1))
                    .and_then(ValueExt::list)
            });
        let parameters = representation_items
            .map_or(MeasureParameters::Record(record), MeasureParameters::Items);
        let values = characteristic_measure_values(&parameters, exchange, &mut measurements, ctx)?;
        let mut named_count = 0usize;
        let mut named_first = None;
        for (name, value) in &values {
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
        let selected = if named_count == 1 {
            named_first
        } else if named_count > 1 {
            losses.push(StepLossCode::DimensionalNominalAmbiguous.note(format!(
                "DIMENSIONAL_CHARACTERISTIC_REPRESENTATION #{id} has {named_count} nominal value measures; the nominal is ambiguous"
                )));
            None
        } else if values.len() == 1 {
            values.first().map(|(_, value)| *value)
        } else {
            if values.len() > 1 {
                losses.push(StepLossCode::DimensionalUnnamedMeasureAmbiguous.note(format!(
                        "DIMENSIONAL_CHARACTERISTIC_REPRESENTATION #{id} has {} unnamed measure values; the nominal is ambiguous",
                        values.len()
                    )));
            }
            None
        };
        if let Some(selected) = selected {
            result.insert(characteristic, selected);
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
        mut visitor: impl FnMut(&Value) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        match self {
            Self::Items(items) => {
                for value in *items {
                    visitor(value)?;
                }
            }
            Self::Record(record) => {
                for value in record_values(record) {
                    visitor(value)?;
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
    parameters.visit(|parameter| {
        collect_measure_ids(
            parameter,
            exchange,
            &mut BTreeSet::new(),
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
                .and_then(|record| measure_item_name(id, record, exchange, measurements.losses));

            values.push((name, value));
        }
    }
    if values.is_empty() {
        parameters.visit(|parameter| {
            if let Some(value) = measure(parameter, exchange, measurements, ctx)? {
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
    measure_ids: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let _nested = ctx.enter_nested("step_pmi_measure_id_walk")?;
    match value {
        Value::Reference(id) => {
            if active.contains(id) {
                return Ok(());
            }
            active.insert(*id);
            if let Some(record) = exchange.records().get(id) {
                if is_measure_record(record) {
                    measure_ids.insert(*id);
                } else {
                    for partial in &record.partials {
                        for parameter in &partial.parameters {
                            collect_measure_ids(parameter, exchange, active, measure_ids, ctx)?;
                        }
                    }
                }
            }
            active.remove(id);
        }
        Value::List(values) => {
            for value in values {
                collect_measure_ids(value, exchange, active, measure_ids, ctx)?;
            }
        }
        Value::Typed(_, value) => {
            collect_measure_ids(value, exchange, active, measure_ids, ctx)?;
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
) -> Option<String> {
    record
        .partials
        .iter()
        .find(|partial| partial.name == "REPRESENTATION_ITEM")
        .and_then(|partial| partial.parameters.first())
        .or_else(|| {
            record
                .partials
                .iter()
                .find(|partial| partial.name == "MEASURE_REPRESENTATION_ITEM")
                .and_then(|partial| partial.parameters.first())
        })
        .and_then(|value| {
            decode_text(
                exchange,
                value,
                losses,
                id,
                "measure item name",
                StepLossCode::MetadataStringInvalid,
            )
        })
        .filter(|name| !name.is_empty())
}

fn measure_context<'a>(
    geometry: &GeometryData,
    id: u64,
    losses: &'a mut Vec<LossNote>,
) -> MeasureContext<'a> {
    MeasureContext {
        length_scale: geometry.units.length([id]).get(),
        angle_scale: geometry.units.angle([id]).get(),
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
    measure_inner(value, exchange, &mut BTreeSet::new(), measurements, ctx)
}

fn measure_inner(
    value: &Value,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    measurements: &mut MeasureContext<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PmiValue>, CodecError> {
    let _nested = ctx.enter_nested("step_pmi_measure_eval_walk")?;
    Ok(match value {
        Value::Integer(value) => cadmpeg_core::convert::f64_from_i64(*value)
            .and_then(|value| PmiValue::new(value, PmiQuantity::Ratio)),
        Value::Real(value) => PmiValue::new(value.get(), PmiQuantity::Ratio),
        Value::Typed(name, value) => value.number().and_then(|number| {
            PmiValue::new(
                if name.contains("LENGTH") {
                    number * measurements.length_scale
                } else if name.contains("ANGLE") {
                    number * measurements.angle_scale
                } else {
                    number
                },
                if name.contains("LENGTH") {
                    PmiQuantity::Length
                } else if name.contains("ANGLE") {
                    PmiQuantity::Angle
                } else {
                    PmiQuantity::Ratio
                },
            )
        }),
        Value::Reference(id) => {
            if active.contains(id) {
                return Ok(None);
            }
            active.insert(*id);
            let Some(record) = exchange.records().get(id) else {
                active.remove(id);
                return Ok(None);
            };
            let mut quantity = None;
            for parameter in record
                .partials
                .iter()
                .flat_map(|partial| &partial.parameters)
            {
                quantity = measure_quantity(parameter, ctx)?;
                if quantity.is_some() {
                    break;
                }
            }
            let quantity = quantity.unwrap_or_else(|| {
                if record
                    .partials
                    .iter()
                    .any(|partial| partial.name.contains("LENGTH"))
                {
                    PmiQuantity::Length
                } else if record
                    .partials
                    .iter()
                    .any(|partial| partial.name.contains("ANGLE"))
                {
                    PmiQuantity::Angle
                } else {
                    PmiQuantity::Ratio
                }
            });
            let unit = record
                .partials
                .iter()
                .flat_map(|partial| &partial.parameters)
                .filter_map(Value::reference)
                .find(|unit| {
                    exchange.records().get(unit).is_some_and(|record| {
                        record.partials.iter().any(|partial| {
                            matches!(partial.name.as_str(), "LENGTH_UNIT" | "PLANE_ANGLE_UNIT")
                        })
                    })
                });
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
                        measurements.losses.push(StepLossCode::PmiLengthUnitUnresolved.note(format!(
                                "PMI length measure #{id} unit scale did not resolve; the document length scale was used"
                            )));
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
                        measurements.losses.push(StepLossCode::PmiAngleUnitUnresolved.note(format!(
                                "PMI angle measure #{id} unit scale did not resolve; the document plane-angle scale was used"
                            )));
                        measurements.angle_scale
                    }
                }
                PmiQuantity::Ratio => 1.0,
            };
            let mut result = None;
            for parameter in record
                .partials
                .iter()
                .flat_map(|partial| &partial.parameters)
            {
                result = ValueExt::typed_number(parameter)
                    .and_then(|number| PmiValue::new(number * scale, quantity));
                if result.is_none() {
                    result = measure_inner(parameter, exchange, active, measurements, ctx)?;
                }
                if result.is_some() {
                    break;
                }
            }
            active.remove(id);
            result
        }
        Value::List(values) => {
            let mut result = None;
            for value in values {
                result = measure_inner(value, exchange, active, measurements, ctx)?;
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
            if name.contains("LENGTH") {
                Some(PmiQuantity::Length)
            } else if name.contains("ANGLE") {
                Some(PmiQuantity::Angle)
            } else if name.contains("RATIO") {
                Some(PmiQuantity::Ratio)
            } else {
                measure_quantity(value, ctx)?
            }
        }
        Value::List(values) => {
            let mut quantity = None;
            for value in values {
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
