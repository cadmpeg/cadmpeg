// SPDX-License-Identifier: Apache-2.0
//! The references from an arena of a held class into a class the scope
//! excludes (§2.1).

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{ArenaClass, ContentScope};
use cadmpeg_core::text::NonBlankString;

use crate::design::dimension::{ParamRef, ParameterRef, RefTarget};
use crate::design::expr::{parse, Expression, Neutral};
use crate::design::scope::DesignScope;
use crate::design::units::{Quantity, ValueKind};
use crate::document::{ArenaName, CadIr, ScopeViolation};
use crate::physics::solver::{SolverSettings, SolverTarget};
use crate::process::groups::{ProcessDefinition, ProcessOwner, ProcessParameter};
use crate::process::setup::Setup;
use crate::process::stock::Wcs;
use crate::products::{ProductDefinition, Storage};
use crate::validate::design::fixture::{id, parameter};

fn part() -> ProductDefinition {
    ProductDefinition::new(id("definition", "part"), Storage::Embedded)
}

fn design_scope() -> DesignScope {
    DesignScope {
        definition: id("definition", "part"),
        roots: Vec::new(),
        active_configuration: None,
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current: Vec::new(),
        unproduced: BTreeSet::new(),
    }
}

fn populated(arena: &str) -> ScopeViolation {
    ScopeViolation::Populated {
        arena: ArenaName::parse(arena).unwrap(),
    }
}

fn referenced(class: ArenaClass, arena: &str) -> ScopeViolation {
    ScopeViolation::Referenced {
        class,
        arena: ArenaName::parse(arena).unwrap(),
    }
}

#[test]
fn a_design_scope_references_the_model_through_its_definition() {
    let mut ir = CadIr::empty();
    ir.model.product_definitions.push(part());
    ir.design.scopes.push(design_scope());
    ir.set_scope(ContentScope::container().with(ArenaClass::Design, true));
    assert_eq!(
        ir.scope_violations(),
        [
            ScopeViolation::Unmet {
                held: ArenaClass::Design,
                required: ArenaClass::Model,
            },
            populated("product_definitions"),
            referenced(ArenaClass::Model, "design.scopes"),
        ]
    );
}

#[test]
fn a_setup_references_the_model_through_its_scope() {
    let mut ir = CadIr::empty();
    ir.model.product_definitions.push(part());
    ir.process.setups.push(Setup {
        id: id("setup", "1"),
        scope: id("definition", "part"),
        name: None,
        job: None,
        axes: None,
        steps: Vec::new(),
        workpieces: Vec::new(),
        fixtures: None,
        stock: None,
        wcs: Wcs {
            orientation: None,
            origin: None,
            placement: None,
        },
        work_offsets: Vec::new(),
        machine: None,
        native_ref: None,
    });
    ir.set_scope(ContentScope::container().with(ArenaClass::Process, true));
    assert_eq!(
        ir.scope_violations(),
        [
            ScopeViolation::Unmet {
                held: ArenaClass::Process,
                required: ArenaClass::Design,
            },
            populated("product_definitions"),
            referenced(ArenaClass::Model, "process.setups"),
        ]
    );
}

#[test]
fn a_process_expression_references_the_design_through_a_design_parameter() {
    let mut ir = CadIr::empty();
    ir.design
        .parameters
        .push(parameter("depth", ValueKind::Quantity(Quantity::LENGTH)));
    let depth = ParamRef::Design(ParameterRef {
        target: RefTarget::Parameter(id("parameter", "depth")),
        through: None,
    });
    ir.process.parameters.push(ProcessParameter {
        id: id("process-parameter", "stepdown"),
        owner: ProcessOwner::Setup(id("setup", "1")),
        name: NonBlankString::new("stepdown").unwrap(),
        kind: Some(ValueKind::Quantity(Quantity::LENGTH)),
        definition: ProcessDefinition::Expression(Expression::Neutral(Neutral(
            parse("$0 / 2", &[depth], &[]).unwrap(),
        ))),
        value: None,
        native_ref: None,
    });
    ir.set_scope(ContentScope::geometry().with(ArenaClass::Process, true));
    assert_eq!(
        ir.scope_violations(),
        [
            ScopeViolation::Unmet {
                held: ArenaClass::Process,
                required: ArenaClass::Design,
            },
            populated("design.parameters"),
            referenced(ArenaClass::Design, "process.parameters"),
        ]
    );
}

#[test]
fn an_arena_of_an_excluded_class_is_reported_once() {
    let mut ir = CadIr::empty();
    ir.model.product_definitions.push(part());
    ir.design.scopes.push(design_scope());
    ir.model.solver_settings.push(SolverSettings {
        id: crate::ids::SolverSettingsId::mint("test:model:solver#hinge").unwrap(),
        solver: "physx".to_owned(),
        schema: "PhysxJointAPI".to_owned(),
        target: SolverTarget::Joint {
            joint: id("feature", "hinge"),
        }
        .into(),
        values: BTreeMap::new(),
        native_ref: None,
    });
    ir.set_scope(ContentScope::container());
    assert_eq!(
        ir.scope_violations(),
        [
            populated("product_definitions"),
            populated("solver_settings"),
            populated("design.scopes"),
        ]
    );
    ir.set_scope(ContentScope::geometry());
    assert_eq!(
        ir.scope_violations(),
        [
            populated("design.scopes"),
            referenced(ArenaClass::Design, "solver_settings"),
        ]
    );
}
