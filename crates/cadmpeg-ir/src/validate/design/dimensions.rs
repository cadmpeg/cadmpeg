// SPDX-License-Identifier: Apache-2.0
//! Dimensions that bind design parameters (§5.12, §5.14).
//!
//! A bound design dimension is `{"kind": "parameter" | "negated", "value":
//! <id>}` on the wire, and the only other design value of that form is a
//! parameter reference's `target`, so a record's wire form names every
//! parameter its dimensions bind. Whether a bound parameter's kind matches
//! the dimension's value type is checked on the typed walk of each record,
//! which knows that type.

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU32;

use cadmpeg_core::text::NonBlankString;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value as Json;

use super::records::{DesignRecords, MemberKey};
use super::walk::BoundDimension;
use super::Held;
use crate::design::dimension::Dimension;
use crate::design::ops::sketch::{RadialMeasure, SketchConstraint, SketchRelation, SketchSpace};
use crate::design::options::Hand;
use crate::design::parameter::{ParameterDefinition, ParameterValue};
use crate::design::units::{PositiveArea, Quantity, ValueKind};
use crate::report::check::Finding;
use crate::scalar::{
    Angle, FiniteReal, Fraction, InteriorAngle, Length, NonNegativeLength, NonZeroLength,
    NonZeroReal, PositiveLength, PositiveReal, SlopeAngle,
};

/// One parameter a dimension binds, with the record that holds it.
struct Binding<'a> {
    holder: &'a str,
    /// The scope of the dimension's owner, where it is known.
    scope: Option<&'a str>,
    parameter: String,
}

pub(super) fn check_dimensions(index: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    let bindings = design_bindings(index);
    for binding in &bindings {
        match index.parameters.get(binding.parameter.as_str()) {
            None => super::error(
                findings,
                binding.holder,
                crate::rule!(
                    "design.dimension-binds-no-parameter-design",
                    "a dimension binds {}, which is no parameter of the design",
                    binding.parameter
                ),
            ),
            Some(parameter) => {
                if let Some(scope) = binding.scope {
                    if parameter.scope.as_str() != scope {
                        super::error(
                            findings,
                            binding.holder,
                            crate::rule!("design.dimension-binds-parameter-not-owner-scope", 
                                "a dimension binds {}, a parameter of {}, not of its owner's scope {scope}",
                                binding.parameter,
                                parameter.scope.as_str()
                            ),
                        );
                    }
                }
            }
        }
    }
    let has_driven = index
        .ir
        .design
        .parameters
        .iter()
        .any(|parameter| matches!(parameter.definition, ParameterDefinition::Driven));
    if has_driven {
        let mut bound: BTreeSet<String> = bindings
            .into_iter()
            .map(|binding| binding.parameter)
            .collect();
        bound.extend(other_bindings(index));
        for parameter in &index.ir.design.parameters {
            if matches!(parameter.definition, ParameterDefinition::Driven)
                && !bound.contains(parameter.id.as_str())
            {
                super::warning(
                    findings,
                    parameter.id.as_str(),
                    crate::rule!(
                        "design.driven-parameter-bound-no-dimension",
                        "driven parameter is bound by no dimension"
                    ),
                );
            }
        }
    }
    let design = &index.ir.design;
    check_sketch_constraints(
        index,
        &design.sketch_constraints,
        |id| id.as_str(),
        findings,
    );
    check_sketch_constraints(
        index,
        &design.spatial_sketch_constraints,
        |id| id.as_str(),
        findings,
    );
}

/// The parameters bound by the dimensions of the design records whose owner
/// scope is known: features and the feature states of overrides, sketches
/// and their entities and constraints, containers, and sheet-metal rules.
fn design_bindings<'a>(index: &DesignRecords<'a>) -> Vec<Binding<'a>> {
    let design = &index.ir.design;
    let mut found = Vec::new();
    let mut scratch = Vec::new();
    let mut add = |holder: &'a str, scope: Option<&'a str>, wire: Option<Json>| {
        for parameter in wire.iter().flat_map(bound_parameters) {
            found.push(Binding {
                holder,
                scope,
                parameter,
            });
        }
    };
    for feature in &design.features {
        let id = feature.id.as_str();
        add(
            id,
            index.scope_of(MemberKey::Feature(id)),
            wire_if_bound(&mut scratch, feature),
        );
    }
    for configuration in &design.configurations {
        let scope = Some(configuration.scope.as_str());
        for state in configuration.overrides.features.values() {
            add(
                configuration.id.as_str(),
                scope,
                wire_if_bound(&mut scratch, state),
            );
        }
    }
    for arrangement in &design.arrangements {
        let scope = Some(arrangement.scope.as_str());
        for state in arrangement.overrides.features.values() {
            add(
                arrangement.id.as_str(),
                scope,
                wire_if_bound(&mut scratch, state),
            );
        }
    }
    for container in &design.containers {
        let id = container.id().as_str();
        add(
            id,
            index.scope_of(MemberKey::Container(id)),
            wire_if_bound(&mut scratch, container),
        );
    }
    let mut owners: BTreeMap<&str, Option<&'a str>> = BTreeMap::new();
    for sketch in &design.sketches {
        let scope = index.scope_of(MemberKey::Feature(sketch.owner.as_str()));
        owners.insert(sketch.id.as_str(), scope);
        add(
            sketch.id.as_str(),
            scope,
            wire_if_bound(&mut scratch, sketch),
        );
    }
    for sketch in &design.spatial_sketches {
        let scope = index.scope_of(MemberKey::Feature(sketch.owner.as_str()));
        owners.insert(sketch.id.as_str(), scope);
        add(
            sketch.id.as_str(),
            scope,
            wire_if_bound(&mut scratch, sketch),
        );
    }
    let owner = |sketch: &str| owners.get(sketch).copied().flatten();
    for entity in &design.sketch_entities {
        add(
            entity.id.as_str(),
            owner(entity.sketch.as_str()),
            wire_if_bound(&mut scratch, entity),
        );
    }
    for constraint in &design.sketch_constraints {
        add(
            constraint.id.as_str(),
            owner(constraint.sketch.as_str()),
            wire_if_bound(&mut scratch, constraint),
        );
    }
    for entity in &design.spatial_sketch_entities {
        add(
            entity.id.as_str(),
            owner(entity.sketch.as_str()),
            wire_if_bound(&mut scratch, entity),
        );
    }
    for constraint in &design.spatial_sketch_constraints {
        add(
            constraint.id.as_str(),
            owner(constraint.sketch.as_str()),
            wire_if_bound(&mut scratch, constraint),
        );
    }
    for rule in &design.sheet_metal_rules {
        add(
            rule.id.as_str(),
            Some(rule.scope.as_str()),
            wire_if_bound(&mut scratch, rule),
        );
    }
    found
}

/// The parameters the dimensions of planar and spatial sketch constraints
/// bind.
pub(super) fn sketch_constraint_bindings(index: &DesignRecords<'_>) -> BTreeSet<String> {
    let design = &index.ir.design;
    let mut scratch = Vec::new();
    let mut found = BTreeSet::new();
    for constraint in &design.sketch_constraints {
        if let Some(wire) = wire_if_bound(&mut scratch, constraint) {
            found.extend(bound_parameters(&wire));
        }
    }
    for constraint in &design.spatial_sketch_constraints {
        if let Some(wire) = wire_if_bound(&mut scratch, constraint) {
            found.extend(bound_parameters(&wire));
        }
    }
    found
}

/// The design parameters a PMI annotation or a process record binds, which
/// count as dimensions that bind a driven parameter.
fn other_bindings(index: &DesignRecords<'_>) -> Vec<String> {
    let ir = index.ir;
    let mut found = Vec::new();
    let mut scratch = Vec::new();
    for annotation in &ir.model.pmi {
        found.extend(
            wire_if_bound(&mut scratch, annotation)
                .map(|wire| bound_parameters(&wire))
                .into_iter()
                .flatten(),
        );
    }
    if !ir.process.is_empty() {
        if let Some(process) = wire_if_bound(&mut scratch, &ir.process) {
            found.extend(design_targets(&process));
        }
    }
    found
}

/// Parses `record`'s JSON representation only when its text mentions a
/// `"parameter"` or `"negated"` tag, skipping `serde_json::Value` allocation
/// for records that hold only literal dimensions.
fn wire_if_bound<T: Serialize>(scratch: &mut Vec<u8>, record: &T) -> Option<Json> {
    scratch.clear();
    crate::hash::finite_json::write_json_value_bytes(scratch, record).ok()?;
    let text = std::str::from_utf8(scratch).ok()?;
    if !text.contains("\"parameter\"") && !text.contains("\"negated\"") {
        return None;
    }
    crate::native::replay::value(text).ok()
}

/// The ids a wire value binds as `{"kind": "parameter" | "negated",
/// "value": <id>}` outside a parameter reference's `target`.
fn bound_parameters(wire: &Json) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![(wire, false)];
    while let Some((node, in_target)) = pending.pop() {
        match node {
            Json::Array(items) => pending.extend(items.iter().map(|item| (item, false))),
            Json::Object(object) => {
                let binding = object.len() == 2
                    && matches!(
                        object.get("kind").and_then(Json::as_str),
                        Some("parameter" | "negated")
                    );
                if let (true, false, Some(Json::String(id))) =
                    (binding, in_target, object.get("value"))
                {
                    found.push(id.clone());
                    continue;
                }
                pending.extend(object.iter().map(|(key, item)| (item, key == "target")));
            }
            Json::Null | Json::Bool(_) | Json::Number(_) | Json::String(_) => {}
        }
    }
    found
}

/// The design parameter ids a wire value names as a parameter reference's
/// `target`.
fn design_targets(wire: &Json) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![wire];
    while let Some(node) = pending.pop() {
        match node {
            Json::Array(items) => pending.extend(items),
            Json::Object(object) => {
                if let Some(Json::Object(target)) = object.get("target") {
                    if target.get("kind").and_then(Json::as_str) == Some("parameter") {
                        if let Some(Json::String(id)) = target.get("value") {
                            found.push(id.clone());
                        }
                    }
                }
                pending.extend(object.values());
            }
            Json::Null | Json::Bool(_) | Json::Number(_) | Json::String(_) => {}
        }
    }
    found
}

/// The value type of a dimension: the kind a parameter it binds has, whether
/// `Negated` applies to it, and the values it admits.
pub(super) trait Bound: DeserializeOwned {
    /// The value type's name in a finding.
    const NAME: &'static str;
    /// Whether the value type is a signed quantity, a Boolean, or an
    /// integer, which `Negated` may bind.
    const SIGNED: bool;
    /// The kind a bound parameter has.
    fn kind() -> ValueKind;
}

macro_rules! bound {
    ($($type:ty => $name:literal, $signed:literal, $kind:expr;)*) => {$(
        impl Bound for $type {
            const NAME: &'static str = $name;
            const SIGNED: bool = $signed;
            fn kind() -> ValueKind {
                $kind
            }
        }
    )*};
}

bound! {
    Length => "length", true, ValueKind::Quantity(Quantity::LENGTH);
    NonZeroLength => "nonzero length", true, ValueKind::Quantity(Quantity::LENGTH);
    PositiveLength => "positive length", false, ValueKind::Quantity(Quantity::LENGTH);
    NonNegativeLength => "nonnegative length", false, ValueKind::Quantity(Quantity::LENGTH);
    Angle => "angle", true, ValueKind::Quantity(Quantity::ANGLE);
    SlopeAngle => "slope angle", true, ValueKind::Quantity(Quantity::ANGLE);
    InteriorAngle => "interior angle", false, ValueKind::Quantity(Quantity::ANGLE);
    FiniteReal => "real", true, ValueKind::Quantity(Quantity::REAL);
    NonZeroReal => "nonzero real", true, ValueKind::Quantity(Quantity::REAL);
    PositiveReal => "positive real", false, ValueKind::Quantity(Quantity::REAL);
    Fraction => "fraction", false, ValueKind::Quantity(Quantity::REAL);
    PositiveArea => "positive area", false, ValueKind::Quantity(Quantity::AREA);
    bool => "Boolean", true, ValueKind::Boolean;
    i64 => "integer", true, ValueKind::Integer;
    u32 => "count", false, ValueKind::Integer;
    NonZeroU32 => "nonzero count", false, ValueKind::Integer;
    NonBlankString => "text", false, ValueKind::Text;
    Hand => "hand", false, ValueKind::Text;
}

/// A dimension's value type, with the dimension's type parameter erased.
pub(super) struct ValueType {
    name: &'static str,
    signed: bool,
    kind: ValueKind,
    admits: fn(&ParameterValue) -> bool,
}

/// The value type of a `Dimension<T>`.
pub(super) fn value_type<T: Bound>() -> ValueType {
    ValueType {
        name: T::NAME,
        signed: T::SIGNED,
        kind: T::kind(),
        admits: admits::<T>,
    }
}

/// Whether `T` admits `value`: whether `value` deserializes as a `T`.
fn admits<T: Bound>(value: &ParameterValue) -> bool {
    from_cache::<T>(value, false).is_some()
}

/// The value of a dimension of type `T` that binds a parameter whose cache is
/// `value`, with the opposite sign or the complement where `negated` (§5.14).
/// `None` where `T` does not admit it.
pub(super) fn from_cache<T: Bound>(value: &ParameterValue, negated: bool) -> Option<T> {
    let wire = match (value, negated) {
        (ParameterValue::Number(number), false) => Json::from(number.get()),
        (ParameterValue::Number(number), true) => Json::from(-number.get()),
        (ParameterValue::Integer(integer), false) => Json::from(*integer),
        (ParameterValue::Integer(integer), true) => Json::from(integer.checked_neg()?),
        (ParameterValue::Boolean(boolean), negated) => Json::Bool(*boolean != negated),
        (ParameterValue::Text(text), false) => Json::String(text.clone()),
        (ParameterValue::Text(_), true) => return None,
    };
    serde_json::from_value::<T>(wire).ok()
}

/// A bound dimension's parameter has the kind its value type takes, a cached
/// value it admits (Warning), and `Negated` binds only a signed quantity, a
/// Boolean, or an integer. A parameter of a `Native` kind states no kind to
/// compare.
pub(super) fn check_bound_dimensions(
    index: &DesignRecords<'_>,
    held: &[Held<'_>],
    findings: &mut Vec<Finding>,
) {
    for held in held {
        let holder = held.holder.record();
        for dimension in &held.dimensions {
            check_bound(index, holder, dimension, findings);
        }
    }
}

fn check_bound(
    index: &DesignRecords<'_>,
    holder: &str,
    dimension: &BoundDimension<'_>,
    findings: &mut Vec<Finding>,
) {
    let value = &dimension.value;
    let id = dimension.parameter.as_str();
    if dimension.negated && !value.signed {
        super::error(
            findings,
            holder,
            crate::rule!(
                "design.dimension-negated-only-signed-quantity-boolean",
                "a {} dimension is `negated`; only a signed quantity or a Boolean is",
                value.name
            ),
        );
    }
    let Some(record) = index.parameters.get(id) else {
        return;
    };
    if matches!(record.kind, ValueKind::Native(_)) {
        return;
    }
    if record.kind != value.kind {
        super::error(
            findings,
            holder,
            crate::rule!(
                "design.dimension-binds-kind",
                "a {} dimension binds {id}, whose kind is {:?}",
                value.name,
                record.kind
            ),
        );
        return;
    }
    if let Some(cached) = &record.value {
        if !(value.admits)(cached) {
            super::warning(
                findings,
                holder,
                crate::rule!(
                    "design.dimension-binds-cached-value-does-not",
                    "a {} dimension binds {id}, whose cached value {cached:?} it does not admit",
                    value.name
                ),
            );
        }
    }
}

/// A parameter that a sketch constraint's diameter dimension binds has no
/// `display`.
fn check_sketch_constraints<S: SketchSpace>(
    index: &DesignRecords<'_>,
    constraints: &[SketchConstraint<S>],
    id: fn(&S::Constraint) -> &str,
    findings: &mut Vec<Finding>,
) {
    for constraint in constraints {
        let SketchRelation::Radial {
            measure: RadialMeasure::Diameter,
            value: Dimension::Parameter(parameter) | Dimension::Negated(parameter),
            ..
        } = &constraint.relation
        else {
            continue;
        };
        let Some(record) = index.parameters.get(parameter.as_str()) else {
            continue;
        };
        if record.display.is_some() {
            super::error(
                findings,
                id(&constraint.id),
                crate::rule!(
                    "design.diameter-dimension-binds-states-display",
                    "a diameter dimension binds {}, which states a `display`",
                    parameter.as_str()
                ),
            );
        }
    }
}
