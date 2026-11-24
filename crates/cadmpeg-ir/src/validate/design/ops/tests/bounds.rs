// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::f64::consts::TAU;

use super::super::super::fixture::{design_findings, id, parameter};
use super::{positive, solid, with};
use crate::design::configuration::{FeatureStateOverride, ParameterOverride};
use crate::design::dimension::Dimension;
use crate::design::feature::FeatureOperation;
use crate::design::ops::shared::RadialDimension;
use crate::design::ops::swept::{Primitive, PrimitiveShape};
use crate::design::parameter::ParameterValue;
use crate::design::units::{Quantity, ValueKind};
use crate::document::CadIr;
use crate::report::Severity;
use crate::scalar::{Angle, FiniteReal, NonNegativeLength};

const ANGLE: &str = "primitive angle";
const RADII: &str = "PrimitiveShape::Cone has both radii zero";

fn cone(angle: Dimension<Angle>, top: Dimension<NonNegativeLength>) -> FeatureOperation {
    let bottom = Dimension::Literal(NonNegativeLength::new(0.0).unwrap());
    FeatureOperation::Primitive(Box::new(Primitive {
        shape: PrimitiveShape::Cone {
            radii: [
                RadialDimension::Radius(bottom),
                RadialDimension::Radius(top),
            ],
            height: Dimension::Literal(positive(1.0)),
            angle: Some(angle),
        },
        placement: None,
        output: solid(),
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }))
}

fn literal_angle(value: f64) -> Dimension<Angle> {
    Dimension::Literal(Angle::new(value).unwrap())
}

fn literal_radius(value: f64) -> Dimension<NonNegativeLength> {
    Dimension::Literal(NonNegativeLength::new(value).unwrap())
}

fn number(value: f64) -> ParameterValue {
    ParameterValue::Number(FiniteReal::new(value).unwrap())
}

/// The fixture with the extrude replaced by `operation`, and a parameter
/// `key` of `quantity` caching `value`.
fn bound(operation: FeatureOperation, key: &str, quantity: Quantity, value: Option<f64>) -> CadIr {
    let mut ir = with(operation);
    let mut cached = parameter(key, ValueKind::Quantity(quantity));
    cached.value = value.map(number);
    ir.design.parameters.push(cached);
    ir
}

fn found(ir: &CadIr, severity: Severity, fragment: &str) -> Vec<(Option<String>, String)> {
    design_findings(ir)
        .into_iter()
        .filter(|finding| finding.severity == severity && finding.message.contains(fragment))
        .map(|finding| (finding.entity, finding.message))
        .collect()
}

fn in_configuration(key: &str) -> String {
    format!("in configuration test:design:configuration#{key}, by the cached values")
}

#[test]
fn a_bound_on_a_cached_value_is_a_warning_in_each_configuration() {
    let angle = Dimension::Parameter(id("parameter", "angle"));
    let ir = bound(
        cone(angle.clone(), literal_radius(1.0)),
        "angle",
        Quantity::ANGLE,
        Some(7.0),
    );
    let warnings = found(&ir, Severity::Warning, ANGLE);
    assert_eq!(warnings.len(), 2, "{warnings:#?}");
    for key in ["base", "large"] {
        assert!(
            warnings.iter().any(|(entity, message)| entity.as_deref()
                == Some("test:design:feature#extrude")
                && message.starts_with("primitive angle 7 lies outside (0, 2π]")
                && message.contains(&in_configuration(key))),
            "{warnings:#?}"
        );
    }
    assert!(found(&ir, Severity::Error, ANGLE).is_empty());

    let at_bound = bound(
        cone(angle.clone(), literal_radius(1.0)),
        "angle",
        Quantity::ANGLE,
        Some(TAU),
    );
    assert!(design_findings(&at_bound)
        .iter()
        .all(|finding| !finding.message.contains(ANGLE)));
    let unstated = bound(
        cone(angle, literal_radius(1.0)),
        "angle",
        Quantity::ANGLE,
        None,
    );
    assert!(design_findings(&unstated)
        .iter()
        .all(|finding| !finding.message.contains(ANGLE)));

    let literal = with(cone(literal_angle(7.0), literal_radius(1.0)));
    assert_eq!(found(&literal, Severity::Error, ANGLE).len(), 1);
    assert!(found(&literal, Severity::Warning, ANGLE).is_empty());
}

#[test]
fn a_negated_dimension_reads_its_cache_with_the_opposite_sign() {
    let negated = Dimension::Negated(id("parameter", "angle"));
    let breaks = bound(
        cone(negated.clone(), literal_radius(1.0)),
        "angle",
        Quantity::ANGLE,
        Some(1.0),
    );
    assert_eq!(
        found(
            &breaks,
            Severity::Warning,
            "primitive angle -1 lies outside"
        )
        .len(),
        2
    );
    let holds = bound(
        cone(negated, literal_radius(1.0)),
        "angle",
        Quantity::ANGLE,
        Some(-1.0),
    );
    assert!(design_findings(&holds)
        .iter()
        .all(|finding| !finding.message.contains(ANGLE)));
}

#[test]
fn a_configuration_reads_the_cache_its_override_states() {
    let mut ir = bound(
        cone(
            Dimension::Parameter(id("parameter", "angle")),
            literal_radius(1.0),
        ),
        "angle",
        Quantity::ANGLE,
        Some(TAU),
    );
    let large = ir
        .design
        .configurations
        .iter_mut()
        .find(|configuration| configuration.id.as_str().ends_with("#large"))
        .unwrap();
    large.overrides.parameters.insert(
        id("parameter", "angle"),
        ParameterOverride {
            expression: None,
            value: Some(number(7.0)),
        },
    );
    let warnings = found(&ir, Severity::Warning, ANGLE);
    assert_eq!(warnings.len(), 1, "{warnings:#?}");
    assert!(warnings[0].1.contains(&in_configuration("large")));
}

#[test]
fn a_definition_is_read_in_the_configurations_where_it_is_in_force() {
    let mut ir = bound(
        cone(
            Dimension::Parameter(id("parameter", "angle")),
            literal_radius(1.0),
        ),
        "angle",
        Quantity::ANGLE,
        Some(7.0),
    );
    let mut definition = ir.design.features[0].definition.clone();
    definition.operation = cone(literal_angle(1.0), literal_radius(1.0));
    let large = ir
        .design
        .configurations
        .iter_mut()
        .find(|configuration| configuration.id.as_str().ends_with("#large"))
        .unwrap();
    large.overrides.features.insert(
        id("feature", "extrude"),
        FeatureStateOverride {
            suppressed: None,
            definition: Some(definition),
        },
    );
    let warnings = found(&ir, Severity::Warning, ANGLE);
    assert_eq!(warnings.len(), 1, "{warnings:#?}");
    assert!(warnings[0].1.contains(&in_configuration("base")));
}

#[test]
fn a_bound_over_two_values_reads_the_literal_and_the_cache() {
    let top = Dimension::Parameter(id("parameter", "top"));
    let ir = bound(
        cone(literal_angle(1.0), top.clone()),
        "top",
        Quantity::LENGTH,
        Some(0.0),
    );
    assert_eq!(found(&ir, Severity::Warning, RADII).len(), 2);
    assert!(found(&ir, Severity::Error, RADII).is_empty());
    let holds = bound(
        cone(literal_angle(1.0), top),
        "top",
        Quantity::LENGTH,
        Some(0.5),
    );
    assert!(design_findings(&holds)
        .iter()
        .all(|finding| !finding.message.contains(RADII)));
    let literal = with(cone(literal_angle(1.0), literal_radius(0.0)));
    assert_eq!(found(&literal, Severity::Error, RADII).len(), 1);
}
