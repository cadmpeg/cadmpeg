// SPDX-License-Identifier: Apache-2.0
//! STEP semantic product-manufacturing information.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::PmiId;
use cadmpeg_ir::pmi::{
    DatumReference, DimensionKind, GeometricToleranceKind, LimitsAndFits, PmiAnnotation,
    PmiDefinition, PmiQuantity, PmiTarget, PmiValue,
};
use cadmpeg_ir::transform::Transform;

use crate::parse::{Exchange, RawRecord, Value};

use super::geometry::GeometryResult;
use crate::vocab::{
    ANGULARITY_TOLERANCE, ANGULAR_LOCATION, ANGULAR_SIZE, ANNOTATION_PLANE,
    CIRCULAR_RUNOUT_TOLERANCE, COMMON_DATUM_LIST, CONCENTRICITY_TOLERANCE, CYLINDRICITY_TOLERANCE,
    DATUM, DATUM_FEATURE, DATUM_REFERENCE_COMPARTMENT, DATUM_REFERENCE_ELEMENT,
    DATUM_REFERENCE_MODIFIER_WITH_VALUE, DATUM_SYSTEM, DIAMETER_SIZE,
    DIMENSIONAL_CHARACTERISTIC_REPRESENTATION, DIMENSIONAL_LOCATION, DIMENSIONAL_SIZE,
    DIMENSIONAL_SIZE_WITH_DATUM_FEATURE, DRAUGHTING_CALLOUT, DRAUGHTING_MODEL,
    DRAUGHTING_MODEL_ITEM_ASSOCIATION, FLATNESS_TOLERANCE, GEOMETRIC_TOLERANCE,
    GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE, LEADER_CURVE, LEADER_DIRECTED_CALLOUT,
    LEADER_DIRECTED_DIMENSION, LENGTH_MEASURE_WITH_UNIT, LENGTH_UNIT, LIMITS_AND_FITS,
    LINE_PROFILE_TOLERANCE, MEASURE_REPRESENTATION_ITEM, MEASURE_WITH_UNIT, PARALLELISM_TOLERANCE,
    PERPENDICULARITY_TOLERANCE, PLANE_ANGLE_MEASURE_WITH_UNIT, PLANE_ANGLE_UNIT,
    PLUS_MINUS_TOLERANCE, POSITION_TOLERANCE, RADIUS_SIZE, ROUNDNESS_TOLERANCE, SHAPE_ASPECT,
    SHAPE_DIMENSION_REPRESENTATION, STRAIGHTNESS_TOLERANCE, SURFACE_PROFILE_TOLERANCE,
    SYMMETRY_TOLERANCE, TESSELLATED_ANNOTATION_OCCURRENCE, TEXT_LITERAL,
    TEXT_LITERAL_WITH_ASSOCIATED_CURVES, TOLERANCE_VALUE, TOTAL_RUNOUT_TOLERANCE,
};

pub(super) struct PmiResult {
    pub typed_records: BTreeSet<u64>,
    pub warnings: Vec<String>,
}

pub(super) fn decode(exchange: &Exchange, geometry: &GeometryResult, ir: &mut CadIr) -> PmiResult {
    let aspects = exchange
        .records
        .iter()
        .filter(|(_, record)| is_shape_aspect(record))
        .map(|(&id, _)| id)
        .collect::<BTreeSet<_>>();
    let mut typed = BTreeSet::new();
    let mut warnings = Vec::new();
    let mut annotations = BTreeMap::<u64, usize>::new();

    let mut presentation_semantics = BTreeMap::<u64, Vec<u64>>::new();
    let characteristic_values =
        characteristic_values(exchange, geometry.length_scale, geometry.plane_angle_scale);
    for (&id, record) in &exchange.records {
        if record.simple_name() != Some(DATUM) {
            continue;
        }
        let identification = record
            .parameters()
            .iter()
            .rev()
            .find_map(ValueExt::text)
            .unwrap_or_else(|| format!("#{id}"));
        push_annotation(
            ir,
            &mut annotations,
            id,
            record.parameter(0).and_then(ValueExt::text),
            targets([id]),
            PmiDefinition::Datum { identification },
        );
        typed.insert(id);
    }

    for (&id, record) in &exchange.records {
        if record.simple_name() != Some(DATUM_SYSTEM) {
            continue;
        }
        let constituents = record
            .parameters()
            .iter()
            .rev()
            .find_map(ValueExt::list)
            .unwrap_or_default();
        let mut datum_records = BTreeSet::new();
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
                    geometry.length_scale,
                    geometry.plane_angle_scale,
                ))
            })
            .flatten()
            .collect::<Vec<_>>();
        push_annotation(
            ir,
            &mut annotations,
            id,
            record.parameter(0).and_then(ValueExt::text),
            targets(
                record
                    .parameters()
                    .iter()
                    .flat_map(references)
                    .filter(|id| aspects.contains(id)),
            ),
            PmiDefinition::DatumSystem {
                references: datum_references,
            },
        );
        typed.insert(id);
        typed.extend(datum_records);
    }

    for (&id, record) in &exchange.records {
        let Some(mut kind) = dimension_kind(record.simple_name()) else {
            continue;
        };
        let name = record.parameters().iter().find_map(ValueExt::text);
        if matches!(kind, DimensionKind::Size) {
            let category = if record
                .simple_name()
                .is_some_and(|name| name.starts_with(DIMENSIONAL_SIZE_WITH_DATUM_FEATURE))
            {
                record.parameters().iter().rev().find_map(ValueExt::text)
            } else {
                name.clone()
            };
            kind = match category.as_deref().map(str::to_ascii_lowercase).as_deref() {
                Some("diameter") => DimensionKind::Diameter,
                Some("radius") => DimensionKind::Radius,
                _ => kind,
            };
        }
        let nominal = characteristic_values
            .get(&id)
            .and_then(|values| values.first())
            .copied();
        let aspect_ids = record
            .parameters()
            .iter()
            .flat_map(references)
            .filter(|reference| aspects.contains(reference));
        push_annotation(
            ir,
            &mut annotations,
            id,
            name,
            targets(aspect_ids),
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

    for (&id, record) in &exchange.records {
        if record.simple_name() != Some(PLUS_MINUS_TOLERANCE) {
            continue;
        }
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
                .filter(|candidate| candidate.simple_name() == Some(TOLERANCE_VALUE))
        });
        let fit = refs.iter().find_map(|reference| {
            let record = exchange.records.get(reference)?;
            (record.simple_name() == Some(LIMITS_AND_FITS)).then(|| {
                (
                    *reference,
                    LimitsAndFits {
                        form_variance: record
                            .parameter(0)
                            .and_then(ValueExt::text)
                            .unwrap_or_default(),
                        zone_variance: record
                            .parameter(1)
                            .and_then(ValueExt::text)
                            .unwrap_or_default(),
                        grade: record
                            .parameter(2)
                            .and_then(ValueExt::text)
                            .unwrap_or_default(),
                        source: record
                            .parameter(3)
                            .and_then(ValueExt::text)
                            .unwrap_or_default(),
                    },
                )
            })
        });
        if let (Some(index), Some(limits)) = (dimension, limits) {
            let lower = limits.parameters().first().and_then(|value| {
                measure(
                    value,
                    exchange,
                    geometry.length_scale,
                    geometry.plane_angle_scale,
                )
            });
            let upper = limits.parameters().get(1).and_then(|value| {
                measure(
                    value,
                    exchange,
                    geometry.length_scale,
                    geometry.plane_angle_scale,
                )
            });
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

    for (&id, record) in &exchange.records {
        let Some(tolerance) = record
            .partials
            .iter()
            .find_map(|partial| tolerance_kind(Some(&partial.name)))
        else {
            continue;
        };
        let refs = record
            .parameters()
            .iter()
            .flat_map(references)
            .collect::<Vec<_>>();
        let magnitude = record.parameters().iter().find_map(|value| {
            measure(
                value,
                exchange,
                geometry.length_scale,
                geometry.plane_angle_scale,
            )
        });
        let Some(magnitude) = magnitude else {
            warnings.push(format!(
                "{} #{id} has no numeric magnitude",
                record.display_name()
            ));
            continue;
        };
        // This path decodes simple tolerance entities. Datum references belong
        // to GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE complex instances; a
        // surplus reference on a simple entity does not alter its semantics.
        let has_datum_reference = record
            .partials
            .iter()
            .any(|partial| partial.name == GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE);
        let datum_system = if has_datum_reference {
            refs.iter().find_map(|id| {
                let annotation = &ir.model.pmi[*annotations.get(id)?];
                matches!(annotation.definition, PmiDefinition::DatumSystem { .. })
                    .then(|| annotation.id.clone())
            })
        } else {
            None
        };
        push_annotation(
            ir,
            &mut annotations,
            id,
            record.parameter(0).and_then(ValueExt::text),
            targets(refs.iter().copied().filter(|id| aspects.contains(id))),
            PmiDefinition::GeometricTolerance {
                tolerance,
                magnitude,
                datum_system,
            },
        );
        typed.insert(id);
        typed.extend(refs.iter().copied().filter(|reference| {
            exchange.records.get(reference).is_some_and(|candidate| {
                matches!(
                    candidate.simple_name(),
                    Some(LENGTH_MEASURE_WITH_UNIT | PLANE_ANGLE_MEASURE_WITH_UNIT)
                )
            })
        }));
    }

    for (&id, record) in &exchange.records {
        if record.simple_name() != Some(DRAUGHTING_MODEL_ITEM_ASSOCIATION) {
            continue;
        }
        let Some(definition) = record.parameter(2).and_then(ValueExt::reference) else {
            continue;
        };
        if annotations.contains_key(&definition) {
            for item in record.parameter(4).into_iter().flat_map(references) {
                presentation_semantics
                    .entry(item)
                    .or_default()
                    .push(definition);
            }
            typed.insert(id);
        }
    }

    for (&id, record) in &exchange.records {
        let Some(name) = record.simple_name() else {
            continue;
        };
        if !is_presentation_annotation(name) {
            continue;
        }
        let mut text_records = BTreeSet::new();
        let text = find_annotation_text(id, exchange, &mut text_records, 0);
        let mut placement_records = BTreeSet::new();
        let placement = record
            .parameters()
            .iter()
            .flat_map(references)
            .find_map(|reference| {
                find_placement(reference, exchange, geometry, &mut placement_records, 0)
            });
        let mut semantics = record
            .parameters()
            .iter()
            .flat_map(references)
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
            record.parameter(0).and_then(ValueExt::text),
            Vec::new(),
            PmiDefinition::Presentation {
                text,
                placement,
                semantics,
            },
        );
        typed.insert(id);
        typed.extend(text_records);
        typed.extend(placement_records);
    }
    for (&id, record) in &exchange.records {
        if matches!(
            record.simple_name(),
            Some(DRAUGHTING_MODEL | ANNOTATION_PLANE | DRAUGHTING_CALLOUT)
        ) {
            typed.insert(id);
        }
    }

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
    typed.extend(aspects.intersection(&targeted_aspects).copied());
    mark_characteristic_representations(exchange, &annotations, &mut typed);
    PmiResult {
        typed_records: typed,
        warnings,
    }
}

fn mark_characteristic_representations(
    exchange: &Exchange,
    annotations: &BTreeMap<u64, usize>,
    typed: &mut BTreeSet<u64>,
) {
    for (&id, record) in &exchange.records {
        if record.simple_name() != Some(DIMENSIONAL_CHARACTERISTIC_REPRESENTATION) {
            continue;
        }
        let record_references = record
            .parameters()
            .iter()
            .flat_map(references)
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
            if representation.simple_name() != Some(SHAPE_DIMENSION_REPRESENTATION) {
                continue;
            }
            typed.insert(representation_id);
            typed.extend(
                representation
                    .parameters()
                    .iter()
                    .flat_map(references)
                    .filter(|reference| {
                        exchange.records.get(reference).is_some_and(|record| {
                            matches!(
                                record.simple_name(),
                                Some(
                                    LENGTH_MEASURE_WITH_UNIT
                                        | PLANE_ANGLE_MEASURE_WITH_UNIT
                                        | MEASURE_WITH_UNIT
                                        | MEASURE_REPRESENTATION_ITEM
                                )
                            )
                        })
                    }),
            );
        }
    }
}

fn datum_references(
    value: &Value,
    precedence: u32,
    exchange: &Exchange,
    annotations: &BTreeMap<u64, usize>,
    typed: &mut BTreeSet<u64>,
    length_scale: f64,
    angle_scale: f64,
) -> Vec<DatumReference> {
    let Some(compartment_id) = value.reference() else {
        return Vec::new();
    };
    let Some(compartment) = exchange.records.get(&compartment_id) else {
        return Vec::new();
    };
    if !matches!(
        compartment.simple_name(),
        Some(DATUM_REFERENCE_COMPARTMENT | DATUM_REFERENCE_ELEMENT)
    ) {
        return Vec::new();
    }
    typed.insert(compartment_id);
    let compartment_modifiers = compartment
        .parameter(5)
        .and_then(ValueExt::list)
        .into_iter()
        .flatten()
        .filter_map(|modifier| modifier_text(modifier, exchange, typed, length_scale, angle_scale))
        .collect::<Vec<_>>();
    let base = compartment.parameter(4);
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
                if element.simple_name() != Some(DATUM_REFERENCE_ELEMENT) {
                    return None;
                }
                let datum = element.parameter(4).and_then(ValueExt::reference)?;
                if !annotations.contains_key(&datum) {
                    return None;
                }
                let mut modifiers = compartment_modifiers.clone();
                modifiers.extend(
                    element
                        .parameter(5)
                        .and_then(ValueExt::list)
                        .into_iter()
                        .flatten()
                        .filter_map(|modifier| {
                            modifier_text(modifier, exchange, typed, length_scale, angle_scale)
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

fn is_common_datum_list(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Typed(kind, _)) if kind == COMMON_DATUM_LIST)
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
    typed: &mut BTreeSet<u64>,
    length_scale: f64,
    angle_scale: f64,
) -> Option<String> {
    match value {
        Value::Enumeration(value) => Some(value.to_ascii_lowercase()),
        Value::Typed(_, value) => modifier_text(value, exchange, typed, length_scale, angle_scale),
        Value::Reference(id) => {
            let record = exchange.records.get(id)?;
            if record.simple_name() != Some(DATUM_REFERENCE_MODIFIER_WITH_VALUE) {
                return None;
            }
            typed.insert(*id);
            let kind = record.parameter(0)?.enumeration()?.to_ascii_lowercase();
            let measure_id = record.parameter(1)?.reference()?;
            let value = measure(
                &Value::Reference(measure_id),
                exchange,
                length_scale,
                angle_scale,
            )?
            .value;
            typed.insert(measure_id);
            Some(format!("{kind}:{value}"))
        }
        _ => None,
    }
}

fn is_presentation_annotation(name: &str) -> bool {
    name.starts_with("ANNOTATION_") && name.ends_with("_OCCURRENCE")
        || matches!(
            name,
            TESSELLATED_ANNOTATION_OCCURRENCE
                | LEADER_CURVE
                | LEADER_DIRECTED_CALLOUT
                | LEADER_DIRECTED_DIMENSION
        )
}

fn find_annotation_text(
    id: u64,
    exchange: &Exchange,
    visited: &mut BTreeSet<u64>,
    depth: usize,
) -> Option<String> {
    if depth >= 256 || !visited.insert(id) {
        return None;
    }
    let record = exchange.records.get(&id)?;
    if matches!(
        record.simple_name(),
        Some(TEXT_LITERAL | TEXT_LITERAL_WITH_ASSOCIATED_CURVES)
    ) {
        return record.parameter(0).and_then(ValueExt::text);
    }
    record
        .parameters()
        .iter()
        .flat_map(references)
        .find_map(|reference| find_annotation_text(reference, exchange, visited, depth + 1))
}

fn find_placement(
    id: u64,
    exchange: &Exchange,
    geometry: &GeometryResult,
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
    exchange
        .records
        .get(&id)?
        .parameters()
        .iter()
        .flat_map(references)
        .find_map(|reference| find_placement(reference, exchange, geometry, visited, depth + 1))
}

fn push_annotation(
    ir: &mut CadIr,
    annotations: &mut BTreeMap<u64, usize>,
    id: u64,
    name: Option<String>,
    targets: Vec<PmiTarget>,
    definition: PmiDefinition,
) {
    annotations.insert(id, ir.model.pmi.len());
    ir.model.pmi.push(PmiAnnotation {
        id: pmi_id(id),
        name: name.filter(|value| !value.is_empty()),
        targets,
        definition,
    });
}

fn targets(ids: impl IntoIterator<Item = u64>) -> Vec<PmiTarget> {
    ids.into_iter()
        .map(|id| PmiTarget::ShapeAspect {
            source_id: format!("#{id}"),
        })
        .collect()
}

fn pmi_id(id: u64) -> PmiId {
    PmiId(format!("step:presentation:pmi#{id}"))
}

fn is_shape_aspect(record: &RawRecord) -> bool {
    matches!(
        record.simple_name(),
        Some(SHAPE_ASPECT | DATUM_FEATURE | DATUM)
    )
}

fn dimension_kind(name: Option<&str>) -> Option<DimensionKind> {
    match name? {
        name if name == DIMENSIONAL_SIZE || name.starts_with("DIMENSIONAL_SIZE_") => {
            Some(DimensionKind::Size)
        }
        name if name == DIMENSIONAL_LOCATION || name.starts_with("DIMENSIONAL_LOCATION_") => {
            Some(DimensionKind::Location)
        }
        name if name == ANGULAR_SIZE
            || name.starts_with("ANGULAR_SIZE_")
            || name == ANGULAR_LOCATION
            || name.starts_with("ANGULAR_LOCATION_") =>
        {
            Some(DimensionKind::Angular)
        }
        DIAMETER_SIZE => Some(DimensionKind::Diameter),
        RADIUS_SIZE => Some(DimensionKind::Radius),
        name if name.ends_with("_SIZE") || name.ends_with("_LOCATION") => {
            Some(DimensionKind::Other(name.to_ascii_lowercase()))
        }
        _ => None,
    }
}

fn tolerance_kind(name: Option<&str>) -> Option<GeometricToleranceKind> {
    use GeometricToleranceKind as Kind;
    Some(match name? {
        STRAIGHTNESS_TOLERANCE => Kind::Straightness,
        FLATNESS_TOLERANCE => Kind::Flatness,
        ROUNDNESS_TOLERANCE => Kind::Roundness,
        CYLINDRICITY_TOLERANCE => Kind::Cylindricity,
        LINE_PROFILE_TOLERANCE => Kind::LineProfile,
        SURFACE_PROFILE_TOLERANCE => Kind::SurfaceProfile,
        ANGULARITY_TOLERANCE => Kind::Angularity,
        PERPENDICULARITY_TOLERANCE => Kind::Perpendicularity,
        PARALLELISM_TOLERANCE => Kind::Parallelism,
        POSITION_TOLERANCE => Kind::Position,
        CONCENTRICITY_TOLERANCE => Kind::Concentricity,
        SYMMETRY_TOLERANCE => Kind::Symmetry,
        CIRCULAR_RUNOUT_TOLERANCE => Kind::CircularRunout,
        TOTAL_RUNOUT_TOLERANCE => Kind::TotalRunout,
        GEOMETRIC_TOLERANCE => Kind::Other("geometric_tolerance".into()),
        name if name.ends_with("_TOLERANCE") && name != PLUS_MINUS_TOLERANCE => {
            Kind::Other(name.to_ascii_lowercase())
        }
        _ => return None,
    })
}

fn characteristic_values(
    exchange: &Exchange,
    length_scale: f64,
    angle_scale: f64,
) -> BTreeMap<u64, Vec<PmiValue>> {
    let mut result = BTreeMap::<u64, Vec<PmiValue>>::new();
    for record in exchange
        .records
        .values()
        .filter(|record| record.simple_name() == Some(DIMENSIONAL_CHARACTERISTIC_REPRESENTATION))
    {
        let Some(characteristic) = record.parameters().iter().flat_map(references).find(|id| {
            exchange
                .records
                .get(id)
                .is_some_and(|record| dimension_kind(record.simple_name()).is_some())
        }) else {
            continue;
        };
        result.entry(characteristic).or_default().extend(
            record
                .parameters()
                .iter()
                .filter_map(|value| measure(value, exchange, length_scale, angle_scale)),
        );
    }
    result
}

fn measure(
    value: &Value,
    exchange: &Exchange,
    length_scale: f64,
    angle_scale: f64,
) -> Option<PmiValue> {
    measure_inner(
        value,
        exchange,
        length_scale,
        angle_scale,
        &mut BTreeSet::new(),
        0,
    )
}

fn measure_inner(
    value: &Value,
    exchange: &Exchange,
    length_scale: f64,
    angle_scale: f64,
    active: &mut BTreeSet<u64>,
    depth: usize,
) -> Option<PmiValue> {
    if depth >= super::MAX_RECORD_GRAPH_DEPTH {
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
                number * length_scale
            } else if name.contains("ANGLE") {
                number * angle_scale
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
            let record = exchange.records.get(id)?;
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
                            matches!(partial.name.as_str(), LENGTH_UNIT | PLANE_ANGLE_UNIT)
                        })
                    })
                });
            let scale = match quantity {
                PmiQuantity::Length => unit
                    .and_then(|unit| {
                        super::geometry::unit_scale_mm(unit, exchange, &mut BTreeSet::new())
                    })
                    .unwrap_or(length_scale),
                PmiQuantity::Angle => unit
                    .and_then(|unit| {
                        super::geometry::unit_scale_radians(unit, exchange, &mut BTreeSet::new())
                    })
                    .unwrap_or(angle_scale),
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
                            measure_inner(
                                parameter,
                                exchange,
                                length_scale,
                                angle_scale,
                                active,
                                depth + 1,
                            )
                        })
                });
            result
        }
        Value::List(values) => values.iter().find_map(|value| {
            measure_inner(
                value,
                exchange,
                length_scale,
                angle_scale,
                active,
                depth + 1,
            )
        }),
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
    fn text(&self) -> Option<String>;
    fn number(&self) -> Option<f64>;
    fn reference(&self) -> Option<u64>;
    fn list(&self) -> Option<&[Value]>;
    fn enumeration(&self) -> Option<&str>;
}

impl ValueExt for Value {
    fn text(&self) -> Option<String> {
        if let Value::String(bytes) = self {
            crate::strings::decode(bytes).ok()
        } else {
            None
        }
    }
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
