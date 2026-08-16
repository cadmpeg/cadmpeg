// SPDX-License-Identifier: Apache-2.0
//! STEP semantic product-manufacturing information.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::PmiId;
use cadmpeg_ir::pmi::{
    DatumReference, DatumTargetForm, DimensionKind, GeometricToleranceKind, LimitsAndFits,
    PmiAnnotation, PmiDefinition, PmiQuantity, PmiTarget, PmiValue,
};
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::transform::Transform;

use crate::ids::StepIdentity;
use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

use super::decode_text;
use super::geometry::GeometryData;
use super::topology::TopologyData;
use super::StageOutcome;

struct MeasureContext<'a> {
    length_scale: f64,
    angle_scale: f64,
    graph_limit: usize,
    losses: &'a mut Vec<LossNote>,
}

pub(super) fn decode(
    exchange: &Exchange,
    geometry: &GeometryData,
    topology: &TopologyData,
    ir: &mut CadIr,
    ctx: Option<&DecodeContext<'_>>,
) -> StageOutcome<()> {
    if !exchange.has_entity_matching(is_pmi_entity_name) {
        return StageOutcome {
            value: (),
            claims: HashSet::new(),
            warnings: Vec::new(),
            losses: Vec::new(),
            notes: Vec::new(),
        };
    }
    let base_aspects = exchange
        .entities_any(&["SHAPE_ASPECT", "DATUM_FEATURE", "DATUM"])
        .map(|(id, _)| id)
        .collect::<BTreeSet<_>>();
    let shape_aspects = exchange
        .matching_entity_ids(is_shape_aspect_name)
        .into_iter()
        .collect::<BTreeSet<_>>();
    let mut typed = HashSet::new();
    let mut warnings = Vec::new();
    let mut losses = Vec::new();
    let mut annotations = BTreeMap::<u64, usize>::new();
    let hidden_presentation_annotations = hidden_presentation_annotation_ids(exchange);

    let mut presentation_semantics = BTreeMap::<u64, Vec<u64>>::new();
    let graph_limit = super::record_graph_limit(ctx);
    let characteristic_values = characteristic_values(exchange, geometry, &mut losses, graph_limit);
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
        push_annotation(
            ir,
            &mut annotations,
            id,
            shape_aspect_parameter(record, 0).and_then(|value| {
                decode_text(
                    exchange,
                    value,
                    &mut losses,
                    id,
                    "datum name",
                    StepLossCode::MetadataStringInvalid,
                )
            }),
            targets([id]),
            None,
            PmiDefinition::Datum { identification },
        );
        typed.insert(id);
    }

    for id in exchange.matching_entity_ids(is_datum_target_name) {
        let Some(record) = exchange.records.get(&id) else {
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
        push_annotation(
            ir,
            &mut annotations,
            id,
            shape_aspect_parameter(record, 0).and_then(|value| {
                decode_text(
                    exchange,
                    value,
                    &mut losses,
                    id,
                    "datum target name",
                    StepLossCode::MetadataStringInvalid,
                )
            }),
            targets([id]),
            None,
            PmiDefinition::DatumTarget {
                form: datum_target_form(&form),
                identification,
                basis: Vec::new(),
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
        let mut measurements = measure_context(geometry, id, &mut losses, graph_limit);
        let datum_references = constituents
            .iter()
            .enumerate()
            .filter_map(|(index, constituent)| {
                let precedence = u32::try_from(index + 1).ok()?;
                Some(datum_references(
                    constituent,
                    precedence,
                    exchange,
                    &annotations,
                    &mut datum_records,
                    &mut measurements,
                ))
            })
            .flatten()
            .collect::<Vec<_>>();
        push_annotation(
            ir,
            &mut annotations,
            id,
            shape_aspect_parameter(record, 0).and_then(|value| {
                decode_text(
                    exchange,
                    value,
                    &mut losses,
                    id,
                    "datum system name",
                    StepLossCode::MetadataStringInvalid,
                )
            }),
            targets(
                record
                    .parameters()
                    .iter()
                    .flat_map(references)
                    .filter(|id| base_aspects.contains(id)),
            ),
            None,
            PmiDefinition::DatumSystem {
                references: datum_references,
            },
        );
        typed.insert(id);
        typed.extend(datum_records);
    }

    for id in exchange.matching_entity_ids(|name| dimension_kind(Some(name)).is_some()) {
        let Some(record) = exchange.records.get(&id) else {
            continue;
        };
        let Some((dimension_name, mut kind)) = dimension_descriptor(record) else {
            continue;
        };
        let name = record
            .partials
            .iter()
            .flat_map(|partial| &partial.parameters)
            .find_map(|value| {
                decode_text(
                    exchange,
                    value,
                    &mut losses,
                    id,
                    "dimension name",
                    StepLossCode::MetadataStringInvalid,
                )
            });
        if matches!(kind, DimensionKind::Size) {
            let category = if dimension_name.starts_with("DIMENSIONAL_SIZE_WITH_DATUM_FEATURE") {
                record
                    .partials
                    .iter()
                    .find(|partial| partial.name == dimension_name)
                    .into_iter()
                    .flat_map(|partial| partial.parameters.iter().rev())
                    .find_map(|value| {
                        decode_text(
                            exchange,
                            value,
                            &mut losses,
                            id,
                            "dimension category",
                            StepLossCode::MetadataStringInvalid,
                        )
                    })
            } else {
                name.clone()
            };
            kind = match category.as_deref().map(str::to_ascii_lowercase).as_deref() {
                Some("diameter") => DimensionKind::Diameter,
                Some("radius") => DimensionKind::Radius,
                _ => kind,
            };
        }
        let nominal = characteristic_values.get(&id).copied();
        let aspect_ids = record
            .partials
            .iter()
            .flat_map(|partial| &partial.parameters)
            .flat_map(references)
            .filter(|reference| shape_aspects.contains(reference));
        push_annotation(
            ir,
            &mut annotations,
            id,
            name,
            targets(aspect_ids),
            None,
            PmiDefinition::Dimension {
                dimension: kind,
                nominal,
                lower_deviation: None,
                upper_deviation: None,
                limits_and_fits: None,
            },
        );
        typed.insert(id);
    }

    for (id, record) in exchange.entities("PLUS_MINUS_TOLERANCE") {
        let refs = record
            .parameters()
            .iter()
            .flat_map(references)
            .collect::<Vec<_>>();
        let dimension = refs
            .iter()
            .find_map(|reference| annotations.get(reference).copied());
        let limits = refs.iter().find_map(|reference| {
            exchange
                .records
                .get(reference)
                .filter(|candidate| candidate.simple_name() == Some("TOLERANCE_VALUE"))
        });
        let fit = refs.iter().find_map(|reference| {
            let record = exchange.records.get(reference)?;
            (record.simple_name() == Some("LIMITS_AND_FITS")).then(|| {
                (
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
                )
            })
        });
        if let (Some(index), Some(limits)) = (dimension, limits) {
            let mut measurements = measure_context(geometry, id, &mut losses, graph_limit);
            let lower = limits
                .parameters()
                .first()
                .and_then(|value| measure(value, exchange, &mut measurements));
            let upper = limits
                .parameters()
                .get(1)
                .and_then(|value| measure(value, exchange, &mut measurements));
            if let PmiDefinition::Dimension {
                lower_deviation,
                upper_deviation,
                ..
            } = &mut ir.model.pmi[index].definition
            {
                *lower_deviation = lower;
                *upper_deviation = upper;
            }
            typed.insert(id);
            typed.extend(refs);
        } else if let (Some(index), Some((fit_id, fit))) = (dimension, fit) {
            if let PmiDefinition::Dimension {
                limits_and_fits, ..
            } = &mut ir.model.pmi[index].definition
            {
                *limits_and_fits = Some(fit);
            }
            typed.extend([id, fit_id]);
        } else {
            warnings.push(format!(
                "PLUS_MINUS_TOLERANCE #{id} has no resolvable dimension and limits"
            ));
        }
    }

    for id in exchange.matching_entity_ids(|name| tolerance_kind(Some(name)).is_some()) {
        let Some(record) = exchange.records.get(&id) else {
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
        let refs = record
            .partials
            .iter()
            .find(|partial| partial.name == "GEOMETRIC_TOLERANCE")
            .map_or_else(
                || {
                    record
                        .parameters()
                        .iter()
                        .flat_map(references)
                        .collect::<Vec<_>>()
                },
                |partial| {
                    partial
                        .parameters
                        .iter()
                        .flat_map(references)
                        .collect::<Vec<_>>()
                },
            );
        let mut measurements = measure_context(geometry, id, &mut losses, graph_limit);
        let magnitude = record
            .partials
            .iter()
            .find(|partial| partial.name == "GEOMETRIC_TOLERANCE")
            .into_iter()
            .flat_map(|partial| partial.parameters.iter())
            .find_map(|value| measure(value, exchange, &mut measurements))
            .or_else(|| {
                record
                    .partials
                    .iter()
                    .filter(|partial| partial.name != "GEOMETRIC_TOLERANCE")
                    .flat_map(|partial| partial.parameters.iter())
                    .find_map(|value| measure(value, exchange, &mut measurements))
            });
        let Some(magnitude) = magnitude else {
            warnings.push(format!(
                "{} #{id} has no numeric magnitude",
                record.display_name()
            ));
            continue;
        };
        let defined_unit = record
            .partials
            .iter()
            .find(|partial| partial.name == "GEOMETRIC_TOLERANCE_WITH_DEFINED_UNIT")
            .and_then(|partial| partial.parameters.first())
            .and_then(|value| measure(value, exchange, &mut measurements));
        let (defined_area_unit, defined_area_second_unit) = record
            .partials
            .iter()
            .find(|partial| partial.name == "GEOMETRIC_TOLERANCE_WITH_DEFINED_AREA_UNIT")
            .map_or((None, None), |partial| {
                (
                    partial
                        .parameters
                        .first()
                        .and_then(ValueExt::enumeration)
                        .map(str::to_ascii_lowercase),
                    partial
                        .parameters
                        .get(1)
                        .and_then(|value| measure(value, exchange, &mut measurements)),
                )
            });
        // A complex tolerance keeps its base targets in GEOMETRIC_TOLERANCE,
        // while GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE carries the datum
        // system as a separate aggregate.
        let datum_system = record
            .partials
            .iter()
            .find(|partial| partial.name == "GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE")
            .into_iter()
            .flat_map(|partial| partial.parameters.iter())
            .flat_map(references)
            .find_map(|id| {
                let annotation = &ir.model.pmi[*annotations.get(&id)?];
                matches!(annotation.definition, PmiDefinition::DatumSystem { .. })
                    .then(|| annotation.id.clone())
            });
        push_annotation(
            ir,
            &mut annotations,
            id,
            named_parameter(record, "GEOMETRIC_TOLERANCE", 0)
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
            targets(refs.iter().copied().filter(|id| base_aspects.contains(id))),
            None,
            PmiDefinition::GeometricTolerance {
                tolerance,
                magnitude,
                defined_unit,
                defined_area_unit,
                defined_area_second_unit,
                datum_system,
                modifiers: tolerance_modifiers(record),
            },
        );
        typed.insert(id);
        typed.extend(refs.iter().copied().filter(|reference| {
            exchange
                .records
                .get(reference)
                .is_some_and(is_measure_record)
        }));
        typed.extend(
            record
                .partials
                .iter()
                .flat_map(|partial| partial.parameters.iter())
                .flat_map(references)
                .filter(|reference| {
                    exchange
                        .records
                        .get(reference)
                        .is_some_and(is_measure_record)
                }),
        );
    }

    for (id, record) in exchange.entities("DRAUGHTING_MODEL_ITEM_ASSOCIATION") {
        let Some(definition) = named_parameter(record, "DRAUGHTING_MODEL_ITEM_ASSOCIATION", 2)
            .and_then(ValueExt::reference)
        else {
            continue;
        };
        if annotations.contains_key(&definition) {
            for item in named_parameter(record, "DRAUGHTING_MODEL_ITEM_ASSOCIATION", 4)
                .into_iter()
                .flat_map(references)
            {
                presentation_semantics
                    .entry(item)
                    .or_default()
                    .push(definition);
            }
            typed.insert(id);
        }
    }

    for id in exchange.matching_entity_ids(is_presentation_annotation) {
        let Some(record) = exchange.records.get(&id) else {
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
            0,
        );
        let parameters = all_parameters(record).collect::<Vec<_>>();
        let placement = parameters
            .iter()
            .flat_map(|value| references(value))
            .find_map(|reference| {
                find_placement(reference, exchange, geometry, &mut BTreeSet::new(), 0)
            });
        let mut semantics = parameters
            .iter()
            .flat_map(|value| references(value))
            .filter(|reference| annotations.contains_key(reference))
            .map(pmi_id)
            .collect::<Vec<_>>();
        semantics.extend(
            presentation_semantics
                .get(&id)
                .into_iter()
                .flatten()
                .copied()
                .map(pmi_id),
        );
        push_annotation(
            ir,
            &mut annotations,
            id,
            named_parameter(record, name, 0)
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
            Vec::new(),
            hidden_presentation_annotations
                .contains(&id)
                .then_some(false),
            PmiDefinition::Presentation {
                text,
                placement,
                semantics,
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

    resolve_feature_for_datum_target_relationships(exchange, &annotations, ir, &mut typed);
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
        &shape_aspects,
        &annotations,
        ir,
        &mut typed,
    );

    let targeted_aspects = ir
        .model
        .pmi
        .iter()
        .flat_map(|annotation| &annotation.targets)
        .filter_map(|target| match target {
            PmiTarget::ShapeAspect { source_id } => source_id.strip_prefix('#')?.parse().ok(),
            _ => None,
        })
        .collect::<BTreeSet<u64>>();
    typed.extend(shape_aspects.intersection(&targeted_aspects).copied());
    mark_characteristic_representations(exchange, &annotations, &mut typed);
    StageOutcome {
        value: (),
        claims: typed,
        warnings,
        losses,
        notes: Vec::new(),
    }
}

fn mark_characteristic_representations(
    exchange: &Exchange,
    annotations: &BTreeMap<u64, usize>,
    typed: &mut HashSet<u64>,
) {
    for (id, record) in exchange.entities("DIMENSIONAL_CHARACTERISTIC_REPRESENTATION") {
        let parameters = record
            .partials
            .iter()
            .flat_map(|partial| &partial.parameters)
            .collect::<Vec<_>>();
        let record_references = parameters
            .iter()
            .flat_map(|value| references(value))
            .collect::<Vec<_>>();
        if !record_references
            .iter()
            .any(|reference| annotations.contains_key(reference))
        {
            continue;
        }
        typed.insert(id);
        for representation_id in record_references {
            let Some(representation) = exchange.records.get(&representation_id) else {
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
            typed.extend(
                representation
                    .partials
                    .iter()
                    .flat_map(|partial| &partial.parameters)
                    .flat_map(references)
                    .filter(|reference| {
                        exchange
                            .records
                            .get(reference)
                            .is_some_and(is_measure_record)
                    }),
            );
        }
    }
}

fn resolve_feature_for_datum_target_relationships(
    exchange: &Exchange,
    annotations: &BTreeMap<u64, usize>,
    ir: &mut CadIr,
    typed: &mut HashSet<u64>,
) {
    for (id, record) in exchange.entities("FEATURE_FOR_DATUM_TARGET_RELATIONSHIP") {
        let Some((relating, related)) = relationship_endpoints(record) else {
            continue;
        };
        let Some(&annotation_index) = annotations.get(&related) else {
            continue;
        };
        let Some(annotation) = ir.model.pmi.get_mut(annotation_index) else {
            continue;
        };
        let PmiDefinition::DatumTarget { basis, .. } = &mut annotation.definition else {
            continue;
        };
        push_target(
            basis,
            PmiTarget::ShapeAspect {
                source_id: format!("#{relating}"),
            },
        );
        typed.extend([id, relating]);
    }
}

fn resolve_geometric_item_usages(
    exchange: &Exchange,
    topology: &TopologyData,
    geometry_sources: GeometrySources<'_>,
    shape_aspects: &BTreeSet<u64>,
    annotations: &BTreeMap<u64, usize>,
    ir: &mut CadIr,
    typed: &mut HashSet<u64>,
) {
    let mut aspect_annotations = BTreeMap::<u64, BTreeSet<usize>>::new();
    for (&annotation_id, &annotation_index) in annotations {
        if shape_aspects.contains(&annotation_id) {
            aspect_annotations
                .entry(annotation_id)
                .or_default()
                .insert(annotation_index);
        }
        let Some(record) = exchange.records.get(&annotation_id) else {
            continue;
        };
        for reference in all_parameters(record).flat_map(references) {
            if shape_aspects.contains(&reference) {
                aspect_annotations
                    .entry(reference)
                    .or_default()
                    .insert(annotation_index);
            }
        }
    }

    let mut relationship_aspects = BTreeMap::<u64, BTreeSet<u64>>::new();
    for record in exchange.records.values() {
        let Some((relating, related)) = relationship_endpoints(record) else {
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

    for record in exchange.records.values() {
        let Some(partial) = record
            .partials
            .iter()
            .find(|partial| partial.name == "GEOMETRIC_ITEM_SPECIFIC_USAGE")
        else {
            continue;
        };
        let Some(definition) = partial.parameters.get(2).and_then(first_reference) else {
            continue;
        };
        let Some(identified_item) = partial.parameters.get(4).and_then(first_reference) else {
            continue;
        };
        let mut annotation_indices = aspect_annotations
            .get(&definition)
            .cloned()
            .unwrap_or_default();
        if let Some(aspects) = relationship_aspects.get(&definition) {
            for aspect in aspects {
                annotation_indices.extend(
                    aspect_annotations
                        .get(aspect)
                        .into_iter()
                        .flatten()
                        .copied(),
                );
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
            let annotation = &mut ir.model.pmi[annotation_index];
            for target in &targets {
                if !annotation.targets.contains(target) {
                    annotation.targets.push(target.clone());
                }
            }
        }
        typed.insert(record.id);
    }
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
) -> Vec<PmiTarget> {
    let mut targets = Vec::new();
    for body in topology.body_by_root.get(&id).into_iter().flatten() {
        push_target(&mut targets, PmiTarget::Body { body: body.clone() });
    }
    for face in topology.faces_by_source.get(&id).into_iter().flatten() {
        push_target(&mut targets, PmiTarget::Face { face: face.clone() });
    }
    for edge in topology.edges_by_source.get(&id).into_iter().flatten() {
        push_target(&mut targets, PmiTarget::Edge { edge: edge.clone() });
    }
    for vertex in topology.vertices_by_source.get(&id).into_iter().flatten() {
        push_target(
            &mut targets,
            PmiTarget::Vertex {
                vertex: vertex.clone(),
            },
        );
    }
    for point in geometry_sources.points.get(&id).into_iter().flatten() {
        push_target(
            &mut targets,
            PmiTarget::Point {
                point: point.clone(),
            },
        );
    }
    for curve in geometry_sources.curves.get(&id).into_iter().flatten() {
        push_target(
            &mut targets,
            PmiTarget::Curve {
                curve: curve.clone(),
            },
        );
    }
    targets
}

fn push_target(targets: &mut Vec<PmiTarget>, target: PmiTarget) {
    if !targets.contains(&target) {
        targets.push(target);
    }
}

fn first_reference(value: &Value) -> Option<u64> {
    references(value).into_iter().next()
}

fn relationship_endpoints(record: &RawRecord) -> Option<(u64, u64)> {
    let parameters = record.partials.iter().find_map(|partial| {
        matches!(
            partial.name.as_str(),
            "SHAPE_ASPECT_RELATIONSHIP" | "FEATURE_FOR_DATUM_TARGET_RELATIONSHIP"
        )
        .then_some(partial.parameters.as_slice())
    })?;
    Some((
        parameters.get(2).and_then(first_reference)?,
        parameters.get(3).and_then(first_reference)?,
    ))
}

fn point_sources(ir: &CadIr) -> BTreeMap<u64, Vec<cadmpeg_ir::ids::PointId>> {
    let mut points = BTreeMap::new();
    for point in &ir.model.points {
        let Some(source) = source_numeric_id(point.id.as_str(), "point") else {
            continue;
        };
        points
            .entry(source)
            .or_insert_with(Vec::new)
            .push(point.id.clone());
    }
    points
}

fn curve_sources(ir: &CadIr) -> BTreeMap<u64, Vec<cadmpeg_ir::ids::CurveId>> {
    let mut curves = BTreeMap::new();
    for curve in &ir.model.curves {
        let Some(source) = source_numeric_id(curve.id.as_str(), "curve") else {
            continue;
        };
        curves
            .entry(source)
            .or_insert_with(Vec::new)
            .push(curve.id.clone());
    }
    curves
}

fn source_numeric_id(identity: &str, kind: &str) -> Option<u64> {
    let suffix = identity.strip_prefix(&format!("step:data:{kind}#"))?;
    let suffix = suffix.strip_prefix("poly-point-").unwrap_or(suffix);
    suffix.split('-').next()?.parse().ok()
}

fn datum_references(
    value: &Value,
    precedence: u32,
    exchange: &Exchange,
    annotations: &BTreeMap<u64, usize>,
    typed: &mut HashSet<u64>,
    measurements: &mut MeasureContext<'_>,
) -> Vec<DatumReference> {
    let Some(compartment_id) = value.reference() else {
        return Vec::new();
    };
    let Some(compartment) = exchange.records.get(&compartment_id) else {
        return Vec::new();
    };
    if !is_datum_reference_partial(compartment, "DATUM_REFERENCE_COMPARTMENT")
        && !is_datum_reference_partial(compartment, "DATUM_REFERENCE_ELEMENT")
    {
        return Vec::new();
    }
    typed.insert(compartment_id);
    let compartment_modifiers = datum_modifiers(compartment)
        .and_then(ValueExt::list)
        .into_iter()
        .flatten()
        .filter_map(|modifier| modifier_text(modifier, exchange, typed, measurements))
        .collect::<Vec<_>>();
    let base = datum_base(compartment);
    if is_common_datum_list(base) {
        let Some(Value::Typed(_, members)) = base else {
            return Vec::new();
        };
        let element_ids = members
            .list()
            .unwrap_or_default()
            .iter()
            .filter_map(ValueExt::reference)
            .collect::<Vec<_>>();
        let common_group = (element_ids.len() >= 2).then_some(precedence);
        return element_ids
            .into_iter()
            .filter_map(|element_id| {
                let element = exchange.records.get(&element_id)?;
                if !is_datum_reference_partial(element, "DATUM_REFERENCE_ELEMENT") {
                    return None;
                }
                let datum = datum_base(element).and_then(ValueExt::reference)?;
                if !annotations.contains_key(&datum) {
                    return None;
                }
                let mut modifiers = compartment_modifiers.clone();
                modifiers.extend(
                    datum_modifiers(element)
                        .and_then(ValueExt::list)
                        .into_iter()
                        .flatten()
                        .filter_map(|modifier| {
                            modifier_text(modifier, exchange, typed, measurements)
                        }),
                );
                typed.extend([element_id, datum]);
                Some(DatumReference {
                    datum: pmi_id(datum),
                    precedence,
                    common_group,
                    modifiers,
                })
            })
            .collect();
    }
    datum_ids(base)
        .into_iter()
        .filter(|datum| annotations.contains_key(datum))
        .map(|datum| {
            typed.insert(datum);
            DatumReference {
                datum: pmi_id(datum),
                precedence,
                common_group: None,
                modifiers: compartment_modifiers.clone(),
            }
        })
        .collect()
}

fn is_datum_reference_partial(record: &RawRecord, name: &str) -> bool {
    record.partials.iter().any(|partial| partial.name == name)
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

fn datum_ids(value: Option<&Value>) -> Vec<u64> {
    match value {
        Some(Value::Reference(id)) => vec![*id],
        Some(Value::List(values)) => values
            .iter()
            .flat_map(|value| datum_ids(Some(value)))
            .collect(),
        _ => Vec::new(),
    }
}

fn modifier_text(
    value: &Value,
    exchange: &Exchange,
    typed: &mut HashSet<u64>,
    measurements: &mut MeasureContext<'_>,
) -> Option<String> {
    match value {
        Value::Enumeration(value) => Some(value.to_ascii_lowercase()),
        Value::Typed(_, value) => modifier_text(value, exchange, typed, measurements),
        Value::Reference(id) => {
            let record = exchange.records.get(id)?;
            let parameters = record
                .partials
                .iter()
                .find(|partial| partial.name == "DATUM_REFERENCE_MODIFIER_WITH_VALUE")?
                .parameters
                .as_slice();
            typed.insert(*id);
            let kind = parameters.first()?.enumeration()?.to_ascii_lowercase();
            let measure_id = parameters.get(1)?.reference()?;
            let value = measure(&Value::Reference(measure_id), exchange, measurements)?.value;
            typed.insert(measure_id);
            Some(format!("{kind}:{value}"))
        }
        _ => None,
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

fn hidden_presentation_annotation_ids(exchange: &Exchange) -> BTreeSet<u64> {
    let mut hidden = BTreeSet::new();
    for record in exchange.records.values() {
        let Some(items) = record
            .partials
            .iter()
            .find(|partial| partial.name == "INVISIBILITY")
            .and_then(|partial| partial.parameters.first())
        else {
            continue;
        };
        for target in references(items) {
            if exchange
                .records
                .get(&target)
                .is_some_and(is_supported_invisibility_target)
            {
                hidden.insert(target);
            }
        }
    }
    hidden
}

fn all_parameters(record: &RawRecord) -> impl Iterator<Item = &Value> {
    record
        .partials
        .iter()
        .flat_map(|partial| partial.parameters.iter())
}

fn find_annotation_text(
    id: u64,
    exchange: &Exchange,
    visited: &mut BTreeSet<u64>,
    used: &mut BTreeSet<u64>,
    losses: &mut Vec<LossNote>,
    depth: usize,
) -> Option<String> {
    let mut candidates = BTreeMap::new();
    collect_annotation_text(id, exchange, visited, &mut candidates, losses, depth);
    match candidates.len() {
        0 => None,
        1 => {
            let (text_id, text) = candidates
                .into_iter()
                .next()
                .expect("one annotation text candidate");
            used.insert(text_id);
            Some(text)
        }
        count => {
            losses.push(StepLossCode::PresentationAnnotationTextUnordered.note(format!(
                    "presentation annotation #{id} has {count} reachable text carriers with no ordered composition"
                )));
            None
        }
    }
}

fn collect_annotation_text(
    id: u64,
    exchange: &Exchange,
    visited: &mut BTreeSet<u64>,
    candidates: &mut BTreeMap<u64, String>,
    losses: &mut Vec<LossNote>,
    depth: usize,
) {
    if depth >= 256 || !visited.insert(id) {
        return;
    }
    let Some(record) = exchange.records.get(&id) else {
        return;
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
    for reference in all_parameters(record).flat_map(references) {
        collect_annotation_text(reference, exchange, visited, candidates, losses, depth + 1);
    }
}

fn find_placement(
    id: u64,
    exchange: &Exchange,
    geometry: &GeometryData,
    visited: &mut BTreeSet<u64>,
    depth: usize,
) -> Option<Transform> {
    if depth >= 256 {
        return None;
    }
    if let Some(&(origin, z_axis, x_axis)) = geometry.placements.get(&id) {
        let y_axis = cadmpeg_ir::math::Vector3::new(
            z_axis.y * x_axis.z - z_axis.z * x_axis.y,
            z_axis.z * x_axis.x - z_axis.x * x_axis.z,
            z_axis.x * x_axis.y - z_axis.y * x_axis.x,
        );
        return Some(Transform {
            rows: [
                [x_axis.x, y_axis.x, z_axis.x, origin.x],
                [x_axis.y, y_axis.y, z_axis.y, origin.y],
                [x_axis.z, y_axis.z, z_axis.z, origin.z],
                [0.0, 0.0, 0.0, 1.0],
            ],
        });
    }
    if !visited.insert(id) {
        return None;
    }
    all_parameters(exchange.records.get(&id)?)
        .flat_map(references)
        .find_map(|reference| find_placement(reference, exchange, geometry, visited, depth + 1))
}

fn push_annotation(
    ir: &mut CadIr,
    annotations: &mut BTreeMap<u64, usize>,
    id: u64,
    name: Option<String>,
    targets: Vec<PmiTarget>,
    visible: Option<bool>,
    definition: PmiDefinition,
) {
    annotations.insert(id, ir.model.pmi.len());
    ir.model.pmi.push(PmiAnnotation {
        id: pmi_id(id),
        name: name.filter(|value| !value.is_empty()),
        visible,
        targets,
        definition,
    });
}

fn targets(ids: impl IntoIterator<Item = u64>) -> Vec<PmiTarget> {
    let mut seen = BTreeSet::new();
    ids.into_iter()
        .filter(|id| seen.insert(*id))
        .map(|id| PmiTarget::ShapeAspect {
            source_id: format!("#{id}"),
        })
        .collect()
}

fn pmi_id(id: u64) -> PmiId {
    PmiId(StepIdentity::presentation("pmi", id))
}

fn datum_target_form(value: &str) -> DatumTargetForm {
    match value.trim().to_ascii_lowercase().as_str() {
        "point" => DatumTargetForm::Point,
        "line" => DatumTargetForm::Line,
        "rectangle" => DatumTargetForm::Rectangle,
        "circle" => DatumTargetForm::Circle,
        "circular curve" => DatumTargetForm::CircularCurve,
        _ => DatumTargetForm::Other(value.to_owned()),
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
    ) || dimension_kind(Some(name)).is_some()
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

fn named_parameter<'a>(record: &'a RawRecord, name: &str, index: usize) -> Option<&'a Value> {
    record
        .partials
        .iter()
        .find(|partial| partial.name == name)
        .and_then(|partial| partial.parameters.get(index))
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

fn dimension_kind(name: Option<&str>) -> Option<DimensionKind> {
    match name? {
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
            Some(DimensionKind::Other(name.to_ascii_lowercase()))
        }
        _ => None,
    }
}

fn dimension_descriptor(record: &RawRecord) -> Option<(&str, DimensionKind)> {
    record.partials.iter().find_map(|partial| {
        dimension_kind(Some(partial.name.as_str())).map(|kind| (partial.name.as_str(), kind))
    })
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

fn tolerance_modifiers(record: &RawRecord) -> Vec<String> {
    record
        .partials
        .iter()
        .find(|partial| partial.name == "GEOMETRIC_TOLERANCE_WITH_MODIFIERS")
        .into_iter()
        .flat_map(|partial| partial.parameters.iter())
        .flat_map(modifier_values)
        .collect()
}

fn modifier_values(value: &Value) -> Vec<String> {
    match value {
        Value::Enumeration(value) => vec![value.to_ascii_lowercase()],
        Value::List(values) => values.iter().flat_map(modifier_values).collect(),
        Value::Typed(_, value) => modifier_values(value),
        _ => Vec::new(),
    }
}

fn characteristic_values(
    exchange: &Exchange,
    geometry: &GeometryData,
    losses: &mut Vec<LossNote>,
    graph_limit: usize,
) -> BTreeMap<u64, PmiValue> {
    let mut result = BTreeMap::<u64, PmiValue>::new();
    for (id, record) in exchange.entities("DIMENSIONAL_CHARACTERISTIC_REPRESENTATION") {
        let mut measurements = measure_context(geometry, id, losses, graph_limit);
        let parameters = record
            .partials
            .iter()
            .flat_map(|partial| &partial.parameters)
            .collect::<Vec<_>>();
        let Some(characteristic) =
            parameters
                .iter()
                .flat_map(|value| references(value))
                .find(|id| {
                    exchange
                        .records
                        .get(id)
                        .is_some_and(|record| dimension_descriptor(record).is_some())
                })
        else {
            continue;
        };
        let representation = parameters
            .iter()
            .flat_map(|value| references(value))
            .find(|id| {
                exchange.records.get(id).is_some_and(|record| {
                    record
                        .partials
                        .iter()
                        .any(|partial| partial.name == "SHAPE_DIMENSION_REPRESENTATION")
                })
            });
        let representation_items = representation
            .and_then(|id| exchange.records.get(&id))
            .and_then(|record| {
                record
                    .partials
                    .iter()
                    .find(|partial| partial.name == "SHAPE_DIMENSION_REPRESENTATION")
                    .and_then(|partial| partial.parameters.get(1))
                    .and_then(ValueExt::list)
            });
        let values = if let Some(items) = representation_items {
            characteristic_measure_values(items.iter(), exchange, &mut measurements)
        } else {
            characteristic_measure_values(parameters.iter().copied(), exchange, &mut measurements)
        };
        let named_nominals = values
            .iter()
            .filter(|(name, _)| {
                name.as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case("nominal value"))
            })
            .map(|(_, value)| *value)
            .collect::<Vec<_>>();
        let selected = if named_nominals.len() == 1 {
            named_nominals.first().copied()
        } else if named_nominals.len() > 1 {
            losses.push(StepLossCode::DimensionalNominalAmbiguous.note(format!(
                    "DIMENSIONAL_CHARACTERISTIC_REPRESENTATION #{id} has {} nominal value measures; the nominal is ambiguous",
                    named_nominals.len()
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
    result
}

fn characteristic_measure_values<'a>(
    parameters: impl IntoIterator<Item = &'a Value>,
    exchange: &Exchange,
    measurements: &mut MeasureContext<'_>,
) -> Vec<(Option<String>, PmiValue)> {
    let parameters = parameters.into_iter().collect::<Vec<_>>();
    let mut measure_ids = BTreeSet::new();
    for parameter in &parameters {
        collect_measure_ids(
            parameter,
            exchange,
            &mut BTreeSet::new(),
            0,
            measurements.graph_limit,
            &mut measure_ids,
        );
    }
    let mut values = measure_ids
        .into_iter()
        .filter_map(|id| {
            let value = measure(&Value::Reference(id), exchange, measurements)?;
            let name = exchange
                .records
                .get(&id)
                .and_then(|record| measure_item_name(record, exchange, measurements.losses));
            Some((name, value))
        })
        .collect::<Vec<_>>();
    if values.is_empty() {
        values.extend(
            parameters.iter().filter_map(|value| {
                measure(value, exchange, measurements).map(|value| (None, value))
            }),
        );
    }
    values
}

fn collect_measure_ids(
    value: &Value,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    depth: usize,
    graph_limit: usize,
    measure_ids: &mut BTreeSet<u64>,
) {
    if depth >= graph_limit {
        return;
    }
    match value {
        Value::Reference(id) => {
            if !active.insert(*id) {
                return;
            }
            if let Some(record) = exchange.records.get(id) {
                if is_measure_record(record) {
                    measure_ids.insert(*id);
                } else {
                    for partial in &record.partials {
                        for parameter in &partial.parameters {
                            collect_measure_ids(
                                parameter,
                                exchange,
                                active,
                                depth + 1,
                                graph_limit,
                                measure_ids,
                            );
                        }
                    }
                }
            }
            active.remove(id);
        }
        Value::List(values) => {
            for value in values {
                collect_measure_ids(value, exchange, active, depth + 1, graph_limit, measure_ids);
            }
        }
        Value::Typed(_, value) => {
            collect_measure_ids(value, exchange, active, depth + 1, graph_limit, measure_ids);
        }
        _ => {}
    }
}

fn measure_item_name(
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
                record.id,
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
    graph_limit: usize,
) -> MeasureContext<'a> {
    MeasureContext {
        length_scale: geometry
            .length_scales
            .get(&id)
            .copied()
            .unwrap_or(geometry.length_scale),
        angle_scale: geometry
            .plane_angle_scales
            .get(&id)
            .copied()
            .unwrap_or(geometry.plane_angle_scale),
        graph_limit,
        losses,
    }
}

fn measure(
    value: &Value,
    exchange: &Exchange,
    measurements: &mut MeasureContext<'_>,
) -> Option<PmiValue> {
    measure_inner(value, exchange, &mut BTreeSet::new(), 0, measurements)
}

fn measure_inner(
    value: &Value,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    depth: usize,
    measurements: &mut MeasureContext<'_>,
) -> Option<PmiValue> {
    if depth >= measurements.graph_limit {
        return None;
    }
    match value {
        Value::Integer(value) => Some(PmiValue {
            value: *value as f64,
            quantity: PmiQuantity::Ratio,
        }),
        Value::Real(value) => Some(PmiValue {
            value: *value,
            quantity: PmiQuantity::Ratio,
        }),
        Value::Typed(name, value) => value.number().map(|number| PmiValue {
            value: if name.contains("LENGTH") {
                number * measurements.length_scale
            } else if name.contains("ANGLE") {
                number * measurements.angle_scale
            } else {
                number
            },
            quantity: if name.contains("LENGTH") {
                PmiQuantity::Length
            } else if name.contains("ANGLE") {
                PmiQuantity::Angle
            } else {
                PmiQuantity::Ratio
            },
        }),
        Value::Reference(id) => {
            if !active.insert(*id) {
                return None;
            }
            let Some(record) = exchange.records.get(id) else {
                active.remove(id);
                return None;
            };
            let quantity = record
                .partials
                .iter()
                .flat_map(|partial| &partial.parameters)
                .find_map(measure_quantity)
                .unwrap_or_else(|| {
                    if record.display_name().contains("LENGTH") {
                        PmiQuantity::Length
                    } else if record.display_name().contains("ANGLE") {
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
                    exchange.records.get(unit).is_some_and(|record| {
                        record.partials.iter().any(|partial| {
                            matches!(partial.name.as_str(), "LENGTH_UNIT" | "PLANE_ANGLE_UNIT")
                        })
                    })
                });
            let scale = match quantity {
                PmiQuantity::Length => unit
                .and_then(|unit| {
                    super::geometry::unit_scale_mm(unit, exchange, &mut BTreeSet::new())
                })
                    .unwrap_or_else(|| {
                        measurements.losses.push(StepLossCode::PmiLengthUnitUnresolved.note(format!(
                                "PMI length measure #{id} unit scale did not resolve; the document length scale was used"
                            )));
                        measurements.length_scale
                    }),
                PmiQuantity::Angle => unit
                    .and_then(|unit| {
                    super::geometry::unit_scale_radians(unit, exchange, &mut BTreeSet::new())
                })
                    .unwrap_or_else(|| {
                        measurements.losses.push(StepLossCode::PmiAngleUnitUnresolved.note(format!(
                                "PMI angle measure #{id} unit scale did not resolve; the document plane-angle scale was used"
                            )));
                        measurements.angle_scale
                    }),
                PmiQuantity::Ratio => 1.0,
            };
            let result = record
                .partials
                .iter()
                .flat_map(|partial| &partial.parameters)
                .find_map(|parameter| {
                    scalar_number(parameter)
                        .map(|number| PmiValue {
                            value: number * scale,
                            quantity,
                        })
                        .or_else(|| {
                            measure_inner(parameter, exchange, active, depth + 1, measurements)
                        })
                });
            active.remove(id);
            result
        }
        Value::List(values) => values
            .iter()
            .find_map(|value| measure_inner(value, exchange, active, depth + 1, measurements)),
        _ => None,
    }
}

fn measure_quantity(value: &Value) -> Option<PmiQuantity> {
    match value {
        Value::Typed(name, value) => {
            if name.contains("LENGTH") {
                Some(PmiQuantity::Length)
            } else if name.contains("ANGLE") {
                Some(PmiQuantity::Angle)
            } else if name.contains("RATIO") {
                Some(PmiQuantity::Ratio)
            } else {
                measure_quantity(value)
            }
        }
        Value::List(values) => values.iter().find_map(measure_quantity),
        _ => None,
    }
}

fn scalar_number(value: &Value) -> Option<f64> {
    match value {
        Value::Integer(value) => Some(*value as f64),
        Value::Real(value) => Some(*value),
        Value::Typed(_, value) => scalar_number(value),
        _ => None,
    }
}

fn references(value: &Value) -> Vec<u64> {
    match value {
        Value::Reference(id) => vec![*id],
        Value::List(values) => values.iter().flat_map(references).collect(),
        Value::Typed(_, value) => references(value),
        _ => Vec::new(),
    }
}

trait RecordExt {
    fn simple_name(&self) -> Option<&str>;
    fn display_name(&self) -> String;
    fn parameters(&self) -> &[Value];
    fn parameter(&self, index: usize) -> Option<&Value>;
}

impl RecordExt for RawRecord {
    fn simple_name(&self) -> Option<&str> {
        (self.partials.len() == 1).then(|| self.partials[0].name.as_str())
    }
    fn display_name(&self) -> String {
        self.partials
            .iter()
            .map(|partial| partial.name.as_str())
            .collect::<Vec<_>>()
            .join("+")
    }
    fn parameters(&self) -> &[Value] {
        self.partials
            .first()
            .map(|partial| partial.parameters.as_slice())
            .unwrap_or_default()
    }
    fn parameter(&self, index: usize) -> Option<&Value> {
        self.parameters().get(index)
    }
}

trait ValueExt {
    fn number(&self) -> Option<f64>;
    fn reference(&self) -> Option<u64>;
    fn list(&self) -> Option<&[Value]>;
    fn enumeration(&self) -> Option<&str>;
}

impl ValueExt for Value {
    fn number(&self) -> Option<f64> {
        match self {
            Value::Integer(value) => Some(*value as f64),
            Value::Real(value) => Some(*value),
            _ => None,
        }
    }
    fn reference(&self) -> Option<u64> {
        if let Value::Reference(id) = self {
            Some(*id)
        } else {
            None
        }
    }
    fn list(&self) -> Option<&[Value]> {
        if let Value::List(values) = self {
            Some(values)
        } else {
            None
        }
    }
    fn enumeration(&self) -> Option<&str> {
        if let Value::Enumeration(value) = self {
            Some(value)
        } else {
            None
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
