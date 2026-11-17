// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use crate::design::configuration::{
    ArrangementKind, DefinitionOverride, FeatureStateOverride, ParameterOverride, PartNumber,
};
use crate::design::dimension::Dimension;
use crate::design::parameter::{Deviations, Tolerance};
use crate::design::units::{Quantity, ValueKind};
use crate::pmi::LimitsAndFits;
use crate::scalar::FiniteReal;
use crate::validate::design::fixture::{
    arrangement, assert_finding, design_findings, document, feature, id, parameter,
};

#[test]
fn a_scope_names_an_active_configuration_of_its_own() {
    let mut ir = document();
    ir.design.scopes[0].active_configuration = None;
    assert_finding(&ir, "states no active configuration");
    let mut ir = document();
    ir.design.scopes[0].active_configuration = Some(id("configuration", "missing"));
    assert_finding(&ir, "is no configuration of the scope");
}

#[test]
fn configuration_parents_stay_in_one_scope_without_cycles() {
    let mut ir = document();
    ir.design.configurations[0].parent = Some(id("configuration", "large"));
    assert_finding(&ir, "is its own ancestor through `parent`");
    let mut ir = document();
    ir.design.configurations[1].parent = Some(id("configuration", "missing"));
    assert_finding(&ir, "is no configuration of the design");
}

#[test]
fn overrides_name_what_they_override_and_state_a_field() {
    let mut ir = document();
    ir.design
        .parameters
        .push(parameter("driven", ValueKind::Quantity(Quantity::LENGTH)));
    let overrides = &mut ir.design.configurations[1].overrides;
    overrides.parameters.insert(
        id("parameter", "driven"),
        ParameterOverride {
            expression: None,
            value: None,
        },
    );
    overrides.features.insert(
        feature("extrude"),
        FeatureStateOverride {
            suppressed: None,
            definition: None,
        },
    );
    overrides
        .definitions
        .insert(id("definition", "part"), DefinitionOverride::default());
    assert_finding(&ir, "which is not defined by an expression");
    assert_finding(&ir, "states neither an expression nor a value");
    assert_finding(&ir, "feature-state override of");
    assert_finding(&ir, "definition override of");
    let mut ir = document();
    ir.design.features[0].suppressed = Some(Dimension::Parameter(id("parameter", "flag")));
    ir.design.configurations[1].overrides.features.insert(
        feature("extrude"),
        FeatureStateOverride {
            suppressed: Some(false),
            definition: None,
        },
    );
    assert_finding(&ir, "whose suppression is bound to a parameter");
}

#[test]
fn part_number_rules_need_what_they_read() {
    let mut ir = document();
    ir.design.configurations[0].overrides.definitions.insert(
        id("definition", "part"),
        DefinitionOverride {
            part_number: Some(PartNumber::ParentConfigurationName),
            ..DefinitionOverride::default()
        },
    );
    ir.design.configurations[1].overrides.definitions.insert(
        id("definition", "part"),
        DefinitionOverride {
            part_number: Some(PartNumber::DocumentName),
            ..DefinitionOverride::default()
        },
    );
    assert_finding(
        &ir,
        "is the parent configuration's name, but no parent is stated",
    );
    assert_finding(
        &ir,
        "neither the definition nor the override states a `Title`",
    );
}

#[test]
fn arrangements_follow_their_kind_and_configuration() {
    let mut ir = document();
    let mut explode = arrangement("explode", ArrangementKind::Explode);
    explode.overrides.definitions.insert(
        id("definition", "part"),
        DefinitionOverride {
            label: Some("x".to_owned()),
            ..DefinitionOverride::default()
        },
    );
    explode.overrides.features.insert(
        feature("extrude"),
        FeatureStateOverride {
            suppressed: Some(true),
            definition: None,
        },
    );
    ir.design.arrangements.push(explode);
    assert_finding(&ir, "an arrangement's overrides hold a definition entry");
    assert_finding(
        &ir,
        "an explode arrangement states a parameter or feature override",
    );
    let mut ir = document();
    let mut owned = arrangement("owned", ArrangementKind::Display);
    owned.configuration = Some(id("configuration", "large"));
    ir.design.arrangements.push(owned);
    ir.design
        .arrangements
        .push(arrangement("other", ArrangementKind::Display));
    ir.design.scopes[0].active_arrangements =
        BTreeSet::from([id("arrangement", "owned"), id("arrangement", "other")]);
    assert_finding(&ir, "active arrangements name two arrangements of one kind");
    ir.design.scopes[0].active_arrangements = BTreeSet::from([id("arrangement", "owned")]);
    assert_finding(&ir, "is in force in configuration");
    ir.design.configurations[0].active_arrangements = BTreeSet::from([id("arrangement", "other")]);
    assert!(design_findings(&ir).is_empty());
}

#[test]
fn parameter_tolerances_need_a_quantity() {
    let mut ir = document();
    let mut text = parameter("text", ValueKind::Text);
    text.tolerance = Some(Tolerance::Basic);
    text.decimals = Some(2);
    let mut angle = parameter("angle", ValueKind::Quantity(Quantity::ANGLE));
    angle.tolerance = Some(Tolerance::Fit {
        fit: LimitsAndFits {
            form_variance: "H".to_owned(),
            zone_variance: "hole".to_owned(),
            grade: "7".to_owned(),
            source: "ISO 286".to_owned(),
        },
        mate: None,
        deviations: None,
    });
    let mut length = parameter("length", ValueKind::Quantity(Quantity::LENGTH));
    length.tolerance = Some(Tolerance::Deviations(Deviations {
        lower: FiniteReal::new(0.2).unwrap(),
        upper: FiniteReal::new(-0.1).unwrap(),
    }));
    ir.design.parameters.extend([angle, length, text]);
    assert_finding(&ir, "states a tolerance but its kind is no quantity");
    assert_finding(&ir, "states `decimals` but its kind is no quantity");
    assert_finding(&ir, "states a fit but its kind is no length");
    assert_finding(&ir, "is not below its upper deviation");
}
