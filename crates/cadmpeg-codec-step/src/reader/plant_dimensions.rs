// SPDX-License-Identifier: Apache-2.0
//! The declared length unit of a plant geometry context, checked against the
//! end-to-end lengths of its plant items (ISO 10303-227).
//!
//! A `DIMENSIONAL_CHARACTERISTIC_REPRESENTATION` relates a
//! `DIMENSIONAL_LOCATION` between two `PLANT_ITEM_CONNECTOR`s to a
//! `SHAPE_DIMENSION_REPRESENTATION` that holds the `'end to end length'`
//! measure with its own unit. Each connector's `'connect point'` is an item of
//! a representation that a `PROPERTY_DEFINITION` of the connector carries, and
//! the connector's `of_shape` names the item's product definition shape,
//! whose shape representation states the geometry context. The decode reads
//! every value as stated and reports a context whose items agree on another
//! length unit.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

use super::geometry::unit_scale_mm;
use super::{RecordExt, ValueExt};
use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

/// Two ratios of a dimension to its connector distance agree within this
/// fraction. Plant coordinates are stated to about single precision (a
/// relative step near 6e-8); the bound keeps a wide margin above it.
const RATIO_AGREEMENT_RELATIVE_TOLERANCE: f64 = 1.0e-6;

/// A common ratio contradicts the declared unit when it lies beyond this
/// factor of one. The closest pair of common length units, the inch and the
/// centimetre, differs by 2.54; dimension rounding stays far below 10%.
const CONTRADICTION_MIN_FACTOR: f64 = 1.1;

/// The fraction of a context's compared dimensions that must agree on the
/// common ratio.
const CLEAR_MAJORITY_FRACTION: f64 = 0.75;

/// One end-to-end length and the distance between its connect points.
struct Comparison {
    /// The length in millimetres by its own unit.
    length_mm: f64,
    /// The connector distance in the context's declared unit, in millimetres.
    distance_mm: f64,
}

/// One loss for each plant geometry context whose items agree that their
/// end-to-end lengths measure the connector distances in another unit.
pub(super) fn check(
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<LossNote>, CodecError> {
    let mut connect_points = BTreeMap::<u64, Vec<[f64; 3]>>::new();
    for (_, record) in exchange.entities("PROPERTY_DEFINITION_REPRESENTATION") {
        let (Some(property), Some(representation)) = (reference(record, 0), reference(record, 1))
        else {
            continue;
        };
        let Some(connector) = exchange
            .records()
            .get(&property)
            .filter(|property| property.simple_name() == Some("PROPERTY_DEFINITION"))
            .and_then(|property| reference(property, 2))
        else {
            continue;
        };
        for item in items(exchange, representation) {
            if let Some(point) = connect_point(exchange, item) {
                connect_points.entry(connector).or_default().push(point);
            }
        }
    }
    let mut contexts = BTreeMap::<u64, (u64, f64)>::new();
    let mut comparisons = BTreeMap::<u64, Vec<Comparison>>::new();
    for (_, record) in exchange.entities("DIMENSIONAL_CHARACTERISTIC_REPRESENTATION") {
        let (Some(dimension), Some(representation)) = (reference(record, 0), reference(record, 1))
        else {
            continue;
        };
        let Some(location) = exchange
            .records()
            .get(&dimension)
            .filter(|location| location.simple_name() == Some("DIMENSIONAL_LOCATION"))
        else {
            continue;
        };
        let (Some(first), Some(second)) = (reference(location, 2), reference(location, 3)) else {
            continue;
        };
        let Some(length_mm) = end_to_end_length(exchange, representation, ctx)? else {
            continue;
        };
        let (Some([p]), Some([q])) = (
            connect_points.get(&first).map(Vec::as_slice),
            connect_points.get(&second).map(Vec::as_slice),
        ) else {
            continue;
        };
        let Some(context) = connector_context(exchange, first) else {
            continue;
        };
        let declared = match contexts.get(&context) {
            Some(&(_, scale)) => scale,
            None => {
                let Some((unit, scale)) = context_length_unit(exchange, context, ctx)? else {
                    continue;
                };
                contexts.insert(context, (unit, scale));
                scale
            }
        };
        let distance = (0..3)
            .map(|axis| (p[axis] - q[axis]).powi(2))
            .sum::<f64>()
            .sqrt();
        if distance > 0.0 {
            comparisons.entry(context).or_default().push(Comparison {
                length_mm,
                distance_mm: distance * declared,
            });
        }
    }
    let mut losses = Vec::new();
    for (context, compared) in comparisons {
        let Some(&(unit, declared)) = contexts.get(&context) else {
            continue;
        };
        if let Some((ratio, agreeing)) = common_ratio(&compared) {
            losses.push(StepLossCode::LengthUnitContradicted.note(format!(
                "geometry context #{context} declares length unit #{unit} of {declared:.6e} mm; \
                 {agreeing} of {} end-to-end lengths of its plant items agree on a ratio of \
                 {ratio:.6e} to their connector distances, which implies a unit of {:.6e} mm; \
                 the decode keeps the declared unit",
                compared.len(),
                declared * ratio
            )));
        }
    }
    Ok(losses)
}

/// The ratio of length to distance that a clear majority of `compared`
/// shares, with its count, where it lies beyond [`CONTRADICTION_MIN_FACTOR`]
/// of one.
fn common_ratio(compared: &[Comparison]) -> Option<(f64, usize)> {
    let mut ratios: Vec<f64> = compared
        .iter()
        .map(|comparison| comparison.length_mm / comparison.distance_mm)
        .filter(|ratio| ratio.is_finite() && *ratio > 0.0)
        .collect();
    ratios.sort_by(f64::total_cmp);
    let median = *ratios.get(ratios.len() / 2)?;
    let agreeing = ratios
        .iter()
        .filter(|ratio| ((*ratio - median) / median).abs() <= RATIO_AGREEMENT_RELATIVE_TOLERANCE)
        .count();
    let total = f64::from(u32::try_from(compared.len()).ok()?);
    let share = f64::from(u32::try_from(agreeing).ok()?) / total;
    let away = !(1.0 / CONTRADICTION_MIN_FACTOR..=CONTRADICTION_MIN_FACTOR).contains(&median);
    (share >= CLEAR_MAJORITY_FRACTION && away).then_some((median, agreeing))
}

fn reference(record: &RawRecord, index: usize) -> Option<u64> {
    record.parameter(index).and_then(Value::reference)
}

/// The items of a representation record.
fn items(exchange: &Exchange, representation: u64) -> Vec<u64> {
    exchange
        .records()
        .get(&representation)
        .and_then(super::topology::representation_item_values)
        .map(|values| values.iter().filter_map(Value::reference).collect())
        .unwrap_or_default()
}

fn has_name(exchange: &Exchange, value: Option<&Value>, name: &str) -> bool {
    matches!(value, Some(Value::String(bytes)) if exchange
        .decode_string(bytes)
        .is_ok_and(|text| text.trim() == name))
}

/// The stated coordinates of a `CARTESIAN_POINT` named `'connect point'`.
fn connect_point(exchange: &Exchange, item: u64) -> Option<[f64; 3]> {
    let record = exchange.records().get(&item)?;
    if record.simple_name() != Some("CARTESIAN_POINT")
        || !has_name(exchange, record.parameter(0), "connect point")
    {
        return None;
    }
    let values = record.parameter(1)?.list()?;
    let [x, y, z] = values else {
        return None;
    };
    Some([x.number()?, y.number()?, z.number()?])
}

/// The `'end to end length'` measure of a dimension representation, in
/// millimetres by its own unit.
fn end_to_end_length(
    exchange: &Exchange,
    representation: u64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<f64>, CodecError> {
    for item in items(exchange, representation) {
        let Some(record) = exchange.records().get(&item) else {
            continue;
        };
        let named = record
            .partial("REPRESENTATION_ITEM")
            .is_some_and(|partial| {
                has_name(exchange, partial.parameters.first(), "end to end length")
            });
        let Some(measure) = record.partial("MEASURE_WITH_UNIT").filter(|_| named) else {
            continue;
        };
        let Some(value) = measure.parameters.first().and_then(ValueExt::typed_number) else {
            continue;
        };
        let Some(unit) = measure.parameters.get(1).and_then(Value::reference) else {
            continue;
        };
        let Some(scale) = unit_scale_mm(unit, exchange, &mut BTreeSet::new(), ctx)? else {
            continue;
        };
        if value > 0.0 {
            return Ok(Some(value * scale.get()));
        }
    }
    Ok(None)
}

/// The one geometry context of the shape representations of a connector's
/// product definition shape.
fn connector_context(exchange: &Exchange, connector: u64) -> Option<u64> {
    let shape = reference(exchange.records().get(&connector)?, 2)?;
    let mut contexts = BTreeSet::new();
    for (_, record) in exchange.entities("SHAPE_DEFINITION_REPRESENTATION") {
        if reference(record, 0) != Some(shape) {
            continue;
        }
        let Some(representation) = reference(record, 1) else {
            continue;
        };
        let Some(represented) = exchange.records().get(&representation) else {
            continue;
        };
        let geometric = represented.partials.iter().any(|partial| {
            partial.name.ends_with("SHAPE_REPRESENTATION")
                && partial.name != "SHAPE_DIMENSION_REPRESENTATION"
        });
        if let Some(context) = reference(represented, 2).filter(|_| geometric) {
            contexts.insert(context);
        }
    }
    match contexts.into_iter().collect::<Vec<_>>().as_slice() {
        [only] => Some(*only),
        _ => None,
    }
}

/// The length unit a context assigns and its scale in millimetres.
fn context_length_unit(
    exchange: &Exchange,
    context: u64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<(u64, f64)>, CodecError> {
    let Some(units) = exchange
        .records()
        .get(&context)
        .and_then(|record| record.partial("GLOBAL_UNIT_ASSIGNED_CONTEXT"))
        .and_then(|partial| partial.parameters.first())
        .and_then(Value::list)
    else {
        return Ok(None);
    };
    for unit in units.iter().filter_map(Value::reference) {
        let is_length = exchange
            .records()
            .get(&unit)
            .is_some_and(|record| record.partial("LENGTH_UNIT").is_some());
        if !is_length {
            continue;
        }
        if let Some(scale) = unit_scale_mm(unit, exchange, &mut BTreeSet::new(), ctx)? {
            return Ok(Some((unit, scale.get())));
        }
    }
    Ok(None)
}
