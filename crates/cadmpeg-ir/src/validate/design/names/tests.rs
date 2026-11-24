// SPDX-License-Identifier: Apache-2.0
//! The order rule of names on the current geometry (part 10, Names).

use std::collections::{BTreeMap, BTreeSet};

use crate::design::container::{Container, OrderedKind, Rollback};
use crate::design::feature::OperationFamily;
use crate::design::names::{NameEntry, NameTable, TopoName};
use crate::design::refs::{EntityRef, Member};
use crate::design::scope::DesignScope;
use crate::design::selection::{OccurrencePath, PathStep, StepRef};
use crate::ids::{OccurrenceId, ProductDefinitionId};
use crate::products::{InstanceBodies, Occurrence, ProductDefinition, Storage};
use crate::validate::design::fixture::{
    assert_finding, body, design_findings, document, feature, id, unresolved,
};
use crate::CadIr;

const UNORDERED: &str = "which the named state does not depend on";

/// The fixture whose first face is named by the feature `proto` of the
/// definition `child`, and whose body is `instance` bodies of the occurrence
/// of `child` where `instance` holds, and the part's own body otherwise.
fn named_by_prototype(instance: bool) -> CadIr {
    let mut ir = document();
    let body = body(&ir);
    let occurrence: OccurrenceId = id("occurrence", "child");
    let prototype: ProductDefinitionId = id("definition", "child");
    ir.model.product_definitions[0]
        .occurrences
        .push(occurrence.clone());
    ir.model
        .occurrences
        .push(Occurrence::new(occurrence.clone(), prototype.clone(), None));
    ir.model
        .product_definitions
        .push(ProductDefinition::new(prototype.clone(), Storage::Embedded));
    if instance {
        ir.model.product_definitions[0].bodies.clear();
        ir.model.product_definitions[0]
            .instance_bodies
            .push(InstanceBodies {
                occurrence: OccurrencePath(vec![PathStep {
                    occurrence: StepRef::Occurrence(occurrence),
                    configuration: None,
                }]),
                bodies: vec![body],
            });
    }
    ir.design.scopes.push(DesignScope {
        definition: prototype,
        roots: vec![id("container", "child-timeline")],
        active_configuration: None,
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current: Vec::new(),
        unproduced: BTreeSet::default(),
    });
    ir.design.containers.push(Container::Ordered {
        id: id("container", "child-timeline"),
        kind: OrderedKind::Timeline,
        members: vec![Member::Feature(feature("proto"))],
        folders: Vec::new(),
        rollback: Some(Rollback::End),
        placement: None,
    });
    ir.design
        .features
        .push(unresolved("proto", OperationFamily::Extrude));
    let name: TopoName = serde_json::from_value(serde_json::json!({
        "kind": "generated",
        "value": {"kind": "face", "feature": feature("proto")},
    }))
    .unwrap();
    ir.design.names = NameTable::from(BTreeMap::from([(
        EntityRef::Face(ir.geometry.faces[0].id.clone()),
        NameEntry {
            name: Some(name),
            native: None,
        },
    )]));
    ir
}

#[test]
fn a_name_on_an_instance_body_mentions_the_features_of_the_placed_prototype() {
    assert_finding(&named_by_prototype(false), UNORDERED);
    let findings = design_findings(&named_by_prototype(true));
    assert!(
        !findings
            .iter()
            .any(|finding| finding.message.contains(UNORDERED)),
        "{findings:#?}"
    );
}
