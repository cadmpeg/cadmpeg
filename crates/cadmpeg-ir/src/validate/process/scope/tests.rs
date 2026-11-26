// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::design::dimension::Dimension;
use crate::design::names::{NameEntry, NameKind, NameTable, TopoName};
use crate::design::refs::{EntityRef, Member, Position};
use crate::design::selection::{Anchor, Members, PickTarget, Picked, Picks, Selection};
use crate::document::CadIr;
use crate::process::groups::ProcessOwner;
use crate::process::operation::{GeometryRole, GeometrySource, ProcessGeometry};
use crate::report::Severity;
use crate::validate::design::fixture as design;
use crate::validate::process::fixture::{
    assert_error, assert_finding, assert_no_process_finding, document, id, place_pin,
    width_through, workpiece_selection,
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
    place_pin(&mut ir);
    let reference = width_through;
    let through = |occurrence: &str| Some(Dimension::Parameter(reference(occurrence)));
    ir.process.operations[2].technology.stepdown = through("pin");
    assert_no_process_finding(&ir);

    ir.process.operations[2].technology.stepdown = through("missing");
    assert_error(
        &ir,
        "an occurrence path names occurrence test:design:occurrence#missing, which does not exist",
    );

    ir.process.operations[2].technology.stepdown = through("nested");
    assert_error(
        &ir,
        "occurrence path step test:design:occurrence#nested is not an occurrence of the previous step's prototype",
    );

    ir.process.operations[2].technology.stepdown = None;
    ir.process.parameters[0].owner = ProcessOwner::Tool(id("tool", "1"));
    ir.process.tools[0].nc.number = Some(Dimension::Parameter(reference("pin")));
    assert_no_process_finding(&ir);
    ir.process.tools[0].nc.number = Some(Dimension::Parameter(reference("nested")));
    assert_error(
        &ir,
        "occurrence path step test:design:occurrence#nested is not an occurrence",
    );
}

/// A path step's `configuration` follows the `ConfigurationRef` rule for the
/// step's prototype (§5.8).
#[test]
fn a_path_steps_configuration_is_one_of_its_prototype() {
    use crate::design::dimension::{ParamRef, ParameterRef};
    use crate::design::refs::ConfigurationRef;
    let configured = |configuration: ConfigurationRef| {
        let mut reference = width_through("pin");
        if let ParamRef::Design(ParameterRef {
            through: Some(path),
            ..
        }) = &mut reference
        {
            path.0[0].configuration = Some(configuration);
        }
        let mut ir = document();
        place_pin(&mut ir);
        ir.process.operations[2].technology.stepdown = Some(Dimension::Parameter(reference));
        ir
    };
    let named = cadmpeg_core::text::NonBlankString::new("light").unwrap();
    assert_no_process_finding(&configured(ConfigurationRef::Named(named)));
    assert_error(
        &configured(ConfigurationRef::Local(design::id("configuration", "base"))),
        "names no configuration of the prototype's scope",
    );
}

/// The name of a face that `feature` generates.
fn face_of(feature: &str) -> TopoName {
    TopoName::Generated {
        kind: NameKind::Face,
        feature: design::feature(feature),
        role: None,
        generator: None,
        index: None,
    }
}

/// A pick of the face `feature` generates, resolved to `resolved`.
fn named(feature: &str, resolved: Option<Vec<EntityRef>>) -> Picked {
    Picked {
        target: PickTarget::Entity(face_of(feature)),
        context: None,
        native: None,
        domain: None,
        witness: None,
        resolved,
    }
}

/// The fixture with the cube's first face named as a face of the extrude,
/// and the operation's geometry a selection of `pick` in the current state.
fn picking(pick: Picked) -> CadIr {
    let mut ir = document();
    let face = EntityRef::Face(ir.geometry.faces[0].id.clone());
    ir.design.names = NameTable::from(BTreeMap::from([(
        face,
        NameEntry {
            name: Some(face_of("extrude")),
            native: None,
        },
    )]));
    let mut selection = workpiece_selection(Some(0));
    selection.selection = Selection {
        at: Anchor::Current,
        picks: Picks::Named(Members::new(vec![pick]).unwrap()),
        native: None,
    };
    ir.process.operations[2].geometry[0].source = GeometrySource::Selection(vec![selection]);
    ir
}

#[test]
fn a_process_pick_resolves_in_the_current_geometry_of_its_setup_scope() {
    assert_no_process_finding(&picking(named("extrude", None)));
    assert_finding(
        &picking(named("fillet", None)),
        Severity::Warning,
        "matches no entity of its anchored state",
    );
    let elsewhere = EntityRef::Face(design::id("face", "elsewhere"));
    assert_error(
        &picking(named("extrude", Some(vec![elsewhere]))),
        "is not a record of its anchored state",
    );
}
