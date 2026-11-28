// SPDX-License-Identifier: Apache-2.0
//! The references from an arena of a held class into a class the scope
//! excludes (§2.1).

use std::collections::{BTreeMap, BTreeSet};

use crate::design::scope::DesignScope;
use crate::document::{ArenaName, CadIr, ScopeViolation};
use crate::physics::solver::{SolverSettings, SolverTarget};
use crate::products::{ProductDefinition, Storage};
use crate::validate::design::fixture::id;
use cadmpeg_core::decode::{ArenaClass, ContentScope};

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
