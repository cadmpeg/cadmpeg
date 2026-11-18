// SPDX-License-Identifier: Apache-2.0

use crate::design::dimension::{Dimension, ParamRef, ParameterRef, RefTarget};
use crate::design::refs::{Member, Position};
use crate::design::selection::{Anchor, OccurrencePath, PathStep, StepRef};
use crate::design::units::{Quantity, ValueKind};
use crate::process::groups::ProcessOwner;
use crate::process::operation::{GeometryRole, GeometrySource, ProcessGeometry};
use crate::products::Occurrence;
use crate::validate::design::fixture as design;
use crate::validate::process::fixture::{
    assert_error, assert_no_process_finding, document, id, workpiece_selection,
};

#[test]
fn every_setup_machines_a_design_scope() {
    let mut ir = document();
    ir.process.setups[0].scope = design::id("definition", "missing");
    assert_error(&ir, "names no definition with a design scope");
}

#[test]
fn workpiece_indices_index_their_setups_workpieces() {
    let mut ir = document();
    ir.process.operations[2].geometry[0].source =
        GeometrySource::Selection(vec![workpiece_selection(Some(1))]);
    assert_error(
        &ir,
        "workpiece 1 at /geometry/0/source/value/0 indexes none of the 1 workpieces",
    );

    let mut ir = document();
    ir.process.features[0].geometry = vec![ProcessGeometry {
        role: GeometryRole::Machine,
        source: GeometrySource::Selection(vec![workpiece_selection(Some(3))]),
        slot: None,
        combine: None,
    }];
    assert_error(
        &ir,
        "workpiece 3 at /geometry/0/source/value/0 indexes none",
    );
}

#[test]
fn a_process_selection_is_anchored_current_or_pinned_in_the_scope() {
    let mut ir = document();
    ir.process.operations[2].geometry[0].source =
        GeometrySource::Selection(vec![workpiece_selection(Some(0))]);
    if let GeometrySource::Selection(selections) = &mut ir.process.operations[2].geometry[0].source
    {
        selections[0].selection.at = Anchor::Input;
    }
    assert_error(&ir, "is anchored `input`");

    let mut ir = document();
    if let GeometrySource::Selection(selections) = &mut ir.process.operations[2].geometry[0].source
    {
        selections[0].selection.at = Anchor::Pinned(design::after("extrude"));
    }
    assert_no_process_finding(&ir);

    let mut ir = document();
    if let GeometrySource::Selection(selections) = &mut ir.process.operations[2].geometry[0].source
    {
        selections[0].selection.at =
            Anchor::Pinned(Position::After(Member::Feature(design::feature("missing"))));
    }
    assert_error(&ir, "names no member of a container");
}

#[test]
fn a_record_no_setup_uses_holds_no_selection() {
    let mut ir = document();
    ir.process.operations[2].features.clear();
    ir.process.features[0].geometry = vec![ProcessGeometry {
        role: GeometryRole::Machine,
        source: GeometrySource::Selection(vec![workpiece_selection(None)]),
        slot: None,
        combine: None,
    }];
    assert_error(&ir, "no setup uses the record, so it holds no selection");
}

/// The fixture with a second part, its own scope, and a setup machining it
/// that uses the fixture's machine.
#[test]
fn the_setups_that_use_a_record_machine_one_scope() {
    let mut ir = document();
    let mut part = ir.model.product_definitions[0].clone();
    part.id = design::id("definition", "second");
    part.bodies.clear();
    ir.model.product_definitions.push(part);
    let mut scope = ir.design.scopes[0].clone();
    scope.definition = design::id("definition", "second");
    scope.roots.clear();
    scope.current.clear();
    scope.active_configuration = None;
    ir.design.scopes.push(scope);
    let mut setup = ir.process.setups[0].clone();
    setup.id = id("setup", "2");
    setup.scope = design::id("definition", "second");
    setup.steps.clear();
    ir.process.setups.push(setup);
    assert_error(&ir, "the setups that use the record machine several scopes");
}

/// A `through` path steps from the setup's scope into the occurrences it
/// places.
#[test]
fn a_through_path_resolves_from_the_setups_scope() {
    let mut ir = document();
    let mut pin = ir.model.product_definitions[0].clone();
    pin.id = design::id("definition", "pin");
    pin.bodies.clear();
    pin.occurrences = vec![design::id("occurrence", "nested")];
    ir.model.product_definitions[0].occurrences = vec![design::id("occurrence", "pin")];
    ir.model.product_definitions.push(pin);
    let occurrence = |key: &str| Occurrence {
        id: design::id("occurrence", key),
        prototype: design::id("definition", "pin"),
        transform: None,
        suppressed: None,
        visible: None,
        representation: None,
        overrides: Vec::new(),
        name: None,
        native_ref: None,
    };
    ir.model.occurrences.push(occurrence("nested"));
    ir.model.occurrences.push(occurrence("pin"));
    ir.design.parameters.push(design::parameter(
        "width",
        ValueKind::Quantity(Quantity::LENGTH),
    ));
    let reference = |occurrence: &str| {
        ParamRef::Design(ParameterRef {
            target: RefTarget::Parameter(design::id("parameter", "width")),
            through: Some(OccurrencePath(vec![PathStep {
                occurrence: StepRef::Occurrence(design::id("occurrence", occurrence)),
                configuration: None,
            }])),
        })
    };
    let through = |occurrence: &str| Some(Dimension::Parameter(reference(occurrence)));
    ir.process.operations[2].technology.stepdown = through("pin");
    assert_no_process_finding(&ir);

    ir.process.operations[2].technology.stepdown = through("missing");
    assert_error(
        &ir,
        "`through` path names test:design:occurrence#missing, which is no occurrence",
    );

    ir.process.operations[2].technology.stepdown = through("nested");
    assert_error(
        &ir,
        "`through` path steps to test:design:occurrence#nested, which is not placed in test:design:definition#part",
    );

    ir.process.operations[2].technology.stepdown = None;
    ir.process.parameters[0].owner = ProcessOwner::Tool(id("tool", "1"));
    ir.process.tools[0].nc.number = Some(Dimension::Parameter(reference("pin")));
    assert_no_process_finding(&ir);
    ir.process.tools[0].nc.number = Some(Dimension::Parameter(reference("nested")));
    assert_error(&ir, "which is not placed in test:design:definition#part");
}
