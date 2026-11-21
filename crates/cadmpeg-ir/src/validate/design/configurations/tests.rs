// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use crate::design::configuration::{
    ArrangementKind, DefinitionOverride, FeatureStateOverride, ParameterOverride, PartNumber,
};
use crate::design::dimension::Dimension;
use crate::design::parameter::{Deviations, Tolerance};
use crate::design::units::{Quantity, ValueKind};
use crate::document::CadIr;
use crate::ids::ProductDefinitionId;
use crate::pmi::LimitsAndFits;
use crate::products::{
    ExternalDocument, Occurrence, ProductDefinition, ProductDefinitionKind, Storage,
};
use crate::scalar::FiniteReal;
use crate::transform::Transform;
use crate::validate::design::fixture::{
    arrangement, assert_finding, assert_no_design_finding, design_findings, document, feature, id,
    parameter,
};
use crate::validate::design::records::DesignRecords;

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
            zone: crate::pmi::FitZone::Hole,
            grade: "7".to_owned(),
            source: Some(crate::pmi::FitStandard::Iso286),
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

fn definition_mut<'a>(ir: &'a mut CadIr, key: &str) -> &'a mut ProductDefinition {
    let definition: ProductDefinitionId = id("definition", key);
    ir.model
        .product_definitions
        .iter_mut()
        .find(|record| record.id == definition)
        .unwrap()
}

/// `ir` with a product definition `key` that has no bodies and no scope.
fn add_definition(ir: &mut CadIr, key: &str) {
    ir.model.product_definitions.push(ProductDefinition {
        kind: Some(ProductDefinitionKind::Part),
        ..ProductDefinition::new(id("definition", key), Storage::Embedded)
    });
}

/// `ir` with occurrence `key` of `prototype` at `transform`, owned by
/// definition `owner`.
fn place_at(ir: &mut CadIr, owner: &str, key: &str, prototype: &str, transform: Option<Transform>) {
    let occurrence = Occurrence::new(
        id("occurrence", key),
        id("definition", prototype),
        transform,
    );
    definition_mut(ir, owner)
        .occurrences
        .push(occurrence.id.clone());
    ir.model.occurrences.push(occurrence);
}

fn place(ir: &mut CadIr, owner: &str, key: &str, prototype: &str) {
    place_at(ir, owner, key, prototype, None);
}

/// `ir` with a product definition `key` that has no bodies and no scope,
/// which configuration `base` overrides.
fn override_definition(ir: &mut CadIr, key: &str) {
    add_definition(ir, key);
    ir.design.configurations[0].overrides.definitions.insert(
        id("definition", key),
        DefinitionOverride {
            label: Some(key.to_owned()),
            ..DefinitionOverride::default()
        },
    );
}

/// The baseline as a Fusion design: its scope `part` holds `extrude` of
/// component `part` and `fillet` of component `bracket`, which has no scope
/// and which configuration `base` overrides.
fn fusion() -> CadIr {
    let mut ir = document();
    ir.design.features[0].component = Some(id("definition", "part"));
    ir.design.features[1].component = Some(id("definition", "bracket"));
    override_definition(&mut ir, "bracket");
    ir
}

#[test]
fn a_definition_override_names_a_definition_the_scope_spans() {
    let ir = fusion();
    assert_finding(&ir, "which is no definition the scope spans");

    let mut ir = fusion();
    add_definition(&mut ir, "assembly");
    place(&mut ir, "assembly", "root", "part");
    place(&mut ir, "part", "bracket", "bracket");
    assert_no_design_finding(&ir);
    override_definition(&mut ir, "nested");
    place(&mut ir, "bracket", "nested", "nested");
    assert_no_design_finding(&ir);
    override_definition(&mut ir, "foreign");
    place(&mut ir, "assembly", "foreign", "foreign");
    assert_finding(&ir, "override names test:design:definition#foreign");

    let mut ir = fusion();
    place(&mut ir, "part", "bracket", "bracket");
    assert_no_design_finding(&ir);
    definition_mut(&mut ir, "bracket").storage =
        Storage::External(ExternalDocument::path("bracket.f3d"));
    assert_finding(&ir, "override names test:design:definition#bracket");

    let mut ir = document();
    override_definition(&mut ir, "other");
    add_definition(&mut ir, "assembly");
    place(&mut ir, "assembly", "root", "part");
    place(&mut ir, "part", "other", "other");
    assert_no_design_finding(&ir);
    let mut ir = document();
    override_definition(&mut ir, "other");
    place(&mut ir, "part", "other", "other");
    assert_no_design_finding(&ir);
    definition_mut(&mut ir, "other").storage = Storage::File {
        document: ExternalDocument::document_id("other"),
        source: None,
    };
    assert_no_design_finding(&ir);
}

#[test]
fn a_fusion_root_component_spans_the_components_its_root_occurrences_place() {
    let mut ir = fusion();
    let root = definition_mut(&mut ir, "part");
    root.kind = None;
    root.storage = Storage::File {
        document: ExternalDocument::document_id("design"),
        source: None,
    };
    definition_mut(&mut ir, "bracket").kind = None;
    override_definition(&mut ir, "pin");
    definition_mut(&mut ir, "pin").kind = None;
    let frame = |x: f64| {
        Transform::affine([
            [1.0, 0.0, 0.0, x],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
    };
    place_at(&mut ir, "part", "bracket:1", "bracket", frame(10.0));
    place_at(&mut ir, "part", "bracket:2", "bracket", frame(20.0));
    place_at(&mut ir, "bracket", "pin:1", "pin", frame(1.0));
    assert!(definition_mut(&mut ir, "part").is_root());
    assert_no_design_finding(&ir);
    let records = DesignRecords::new(&ir);
    assert_eq!(
        records.spanned_definitions("test:design:definition#part"),
        BTreeSet::from([
            "test:design:definition#bracket",
            "test:design:definition#part",
            "test:design:definition#pin",
        ])
    );
}
