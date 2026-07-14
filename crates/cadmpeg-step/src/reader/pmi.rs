// SPDX-License-Identifier: Apache-2.0
//! STEP semantic product-manufacturing information.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::PmiId;
use cadmpeg_ir::pmi::{
    DatumReference, DimensionKind, GeometricToleranceKind, PmiAnnotation, PmiDefinition,
    PmiQuantity, PmiTarget, PmiValue,
};
use cadmpeg_ir::transform::Transform;

use crate::parse::{Exchange, RawRecord, Value};

use super::geometry::GeometryResult;

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

    for (&id, record) in &exchange.records {
        if record.simple_name() != Some("DATUM") {
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
        if record.simple_name() != Some("DATUM_SYSTEM") {
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
            .flat_map(|(index, constituent)| {
                datum_references(
                    constituent,
                    u32::try_from(index + 1).expect("datum precedence exceeds u32"),
                    exchange,
                    &annotations,
                    &mut datum_records,
                    geometry.length_scale,
                )
            })
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
        let Some(kind) = dimension_kind(record.simple_name()) else {
            continue;
        };
        let nominal = characteristic_values(id, exchange, geometry.length_scale)
            .into_iter()
            .next();
        let aspect_ids = record
            .parameters()
            .iter()
            .flat_map(references)
            .filter(|reference| aspects.contains(reference));
        push_annotation(
            ir,
            &mut annotations,
            id,
            record.parameters().iter().find_map(ValueExt::text),
            targets(aspect_ids),
            PmiDefinition::Dimension {
                dimension: kind,
                nominal,
                lower_deviation: None,
                upper_deviation: None,
            },
        );
        typed.insert(id);
    }

    for (&id, record) in &exchange.records {
        if record.simple_name() != Some("PLUS_MINUS_TOLERANCE") {
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
                .filter(|candidate| candidate.simple_name() == Some("TOLERANCE_VALUE"))
        });
        if let (Some(index), Some(limits)) = (dimension, limits) {
            let values = limits
                .parameters()
                .iter()
                .filter_map(|value| measure(value, exchange, geometry.length_scale))
                .collect::<Vec<_>>();
            if let PmiDefinition::Dimension {
                lower_deviation,
                upper_deviation,
                ..
            } = &mut ir.model.pmi[index].definition
            {
                *lower_deviation = values.first().copied();
                *upper_deviation = values.get(1).copied();
            }
            typed.insert(id);
            typed.extend(refs);
        } else {
            warnings.push(format!(
                "PLUS_MINUS_TOLERANCE #{id} has no resolvable dimension and limits"
            ));
        }
    }

    for (&id, record) in &exchange.records {
        let Some(tolerance) = tolerance_kind(record.simple_name()) else {
            continue;
        };
        let refs = record
            .parameters()
            .iter()
            .flat_map(references)
            .collect::<Vec<_>>();
        let magnitude = record
            .parameters()
            .iter()
            .find_map(|value| measure(value, exchange, geometry.length_scale));
        let Some(magnitude) = magnitude else {
            warnings.push(format!(
                "{} #{id} has no numeric magnitude",
                record.display_name()
            ));
            continue;
        };
        let datum_system = refs
            .iter()
            .find(|reference| {
                exchange
                    .records
                    .get(reference)
                    .is_some_and(|candidate| candidate.simple_name() == Some("DATUM_SYSTEM"))
            })
            .map(|id| pmi_id(*id));
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
                    Some("LENGTH_MEASURE_WITH_UNIT" | "PLANE_ANGLE_MEASURE_WITH_UNIT")
                )
            })
        }));
    }

    for (&id, record) in &exchange.records {
        let Some(name) = record.simple_name() else {
            continue;
        };
        if !is_presentation_annotation(name) {
            continue;
        }
        let mut text_records = BTreeSet::new();
        let text = find_annotation_text(id, exchange, &mut text_records);
        let mut placement_records = BTreeSet::new();
        let placement = record
            .parameters()
            .iter()
            .flat_map(references)
            .find_map(|reference| {
                find_placement(reference, exchange, geometry, &mut placement_records)
            });
        let semantics = record
            .parameters()
            .iter()
            .flat_map(references)
            .filter(|reference| annotations.contains_key(reference))
            .map(pmi_id)
            .collect::<Vec<_>>();
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
            Some("DRAUGHTING_MODEL" | "ANNOTATION_PLANE" | "DRAUGHTING_CALLOUT")
        ) {
            typed.insert(id);
        }
    }

    typed.extend(aspects.into_iter().filter(|id| {
        ir.model.pmi.iter().any(|annotation| {
            annotation.targets.iter().any(|target| {
                matches!(target, PmiTarget::ShapeAspect { source_id } if source_id == &format!("#{id}"))
            })
        })
    }));
    PmiResult {
        typed_records: typed,
        warnings,
    }
}

fn datum_references(
    value: &Value,
    precedence: u32,
    exchange: &Exchange,
    annotations: &BTreeMap<u64, usize>,
    typed: &mut BTreeSet<u64>,
    length_scale: f64,
) -> Vec<DatumReference> {
    let Some(compartment_id) = value.reference() else {
        return Vec::new();
    };
    let Some(compartment) = exchange.records.get(&compartment_id) else {
        return Vec::new();
    };
    if !matches!(
        compartment.simple_name(),
        Some("DATUM_REFERENCE_COMPARTMENT" | "DATUM_REFERENCE_ELEMENT")
    ) {
        return Vec::new();
    }
    typed.insert(compartment_id);
    let modifiers = compartment
        .parameter(5)
        .and_then(ValueExt::list)
        .into_iter()
        .flatten()
        .filter_map(|modifier| modifier_text(modifier, exchange, typed, length_scale))
        .collect::<Vec<_>>();
    datum_ids(compartment.parameter(4))
        .into_iter()
        .filter(|datum| annotations.contains_key(datum))
        .map(|datum| {
            typed.insert(datum);
            DatumReference {
                datum: pmi_id(datum),
                precedence,
                modifiers: modifiers.clone(),
            }
        })
        .collect()
}

fn datum_ids(value: Option<&Value>) -> Vec<u64> {
    match value {
        Some(Value::Reference(id)) => vec![*id],
        Some(Value::Typed(kind, value)) if kind == "COMMON_DATUM_LIST" => value
            .list()
            .unwrap_or_default()
            .iter()
            .flat_map(|value| datum_ids(Some(value)))
            .collect(),
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
) -> Option<String> {
    match value {
        Value::Enumeration(value) => Some(value.to_ascii_lowercase()),
        Value::Typed(_, value) => modifier_text(value, exchange, typed, length_scale),
        Value::Reference(id) => {
            let record = exchange.records.get(id)?;
            if record.simple_name() != Some("DATUM_REFERENCE_MODIFIER_WITH_VALUE") {
                return None;
            }
            typed.insert(*id);
            let kind = record.parameter(0)?.enumeration()?.to_ascii_lowercase();
            let measure_id = record.parameter(1)?.reference()?;
            let value = measure(&Value::Reference(measure_id), exchange, length_scale)?.value;
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
            "TESSELLATED_ANNOTATION_OCCURRENCE"
                | "LEADER_CURVE"
                | "LEADER_DIRECTED_CALLOUT"
                | "LEADER_DIRECTED_DIMENSION"
        )
}

fn find_annotation_text(
    id: u64,
    exchange: &Exchange,
    visited: &mut BTreeSet<u64>,
) -> Option<String> {
    if !visited.insert(id) {
        return None;
    }
    let record = exchange.records.get(&id)?;
    if matches!(
        record.simple_name(),
        Some("TEXT_LITERAL" | "TEXT_LITERAL_WITH_ASSOCIATED_CURVES")
    ) {
        return record.parameter(1).and_then(ValueExt::text);
    }
    record
        .parameters()
        .iter()
        .flat_map(references)
        .find_map(|reference| find_annotation_text(reference, exchange, visited))
}

fn find_placement(
    id: u64,
    exchange: &Exchange,
    geometry: &GeometryResult,
    visited: &mut BTreeSet<u64>,
) -> Option<Transform> {
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
        .find_map(|reference| find_placement(reference, exchange, geometry, visited))
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
        Some("SHAPE_ASPECT" | "DATUM_FEATURE" | "DATUM")
    )
}

fn dimension_kind(name: Option<&str>) -> Option<DimensionKind> {
    match name? {
        "DIMENSIONAL_SIZE" => Some(DimensionKind::Size),
        "DIMENSIONAL_LOCATION" => Some(DimensionKind::Location),
        "ANGULAR_SIZE" | "ANGULAR_LOCATION" => Some(DimensionKind::Angular),
        "DIAMETER_SIZE" => Some(DimensionKind::Diameter),
        "RADIUS_SIZE" => Some(DimensionKind::Radius),
        name if name.ends_with("_SIZE") || name.ends_with("_LOCATION") => {
            Some(DimensionKind::Other(name.to_ascii_lowercase()))
        }
        _ => None,
    }
}

fn tolerance_kind(name: Option<&str>) -> Option<GeometricToleranceKind> {
    use GeometricToleranceKind as Kind;
    Some(match name? {
        "STRAIGHTNESS_TOLERANCE" => Kind::Straightness,
        "FLATNESS_TOLERANCE" => Kind::Flatness,
        "ROUNDNESS_TOLERANCE" => Kind::Roundness,
        "CYLINDRICITY_TOLERANCE" => Kind::Cylindricity,
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
        "GEOMETRIC_TOLERANCE" => Kind::Other("geometric_tolerance".into()),
        name if name.ends_with("_TOLERANCE") && name != "PLUS_MINUS_TOLERANCE" => {
            Kind::Other(name.to_ascii_lowercase())
        }
        _ => return None,
    })
}

fn characteristic_values(id: u64, exchange: &Exchange, scale: f64) -> Vec<PmiValue> {
    exchange
        .records
        .values()
        .filter(|record| {
            record.simple_name() == Some("DIMENSIONAL_CHARACTERISTIC_REPRESENTATION")
                && record
                    .parameters()
                    .iter()
                    .flat_map(references)
                    .any(|item| item == id)
        })
        .flat_map(RecordExt::parameters)
        .filter_map(|value| measure(value, exchange, scale))
        .collect()
}

fn measure(value: &Value, exchange: &Exchange, scale: f64) -> Option<PmiValue> {
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
                number * scale
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
            let record = exchange.records.get(id)?;
            let quantity = if record.display_name().contains("LENGTH") {
                PmiQuantity::Length
            } else if record.display_name().contains("ANGLE") {
                PmiQuantity::Angle
            } else {
                PmiQuantity::Ratio
            };
            record.parameters().iter().find_map(|parameter| {
                parameter
                    .number()
                    .map(|number| PmiValue {
                        value: if quantity == PmiQuantity::Length {
                            number * scale
                        } else {
                            number
                        },
                        quantity,
                    })
                    .or_else(|| measure(parameter, exchange, scale))
            })
        }
        Value::List(values) => values
            .iter()
            .find_map(|value| measure(value, exchange, scale)),
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
        &self.partials[0].parameters
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
