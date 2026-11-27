// SPDX-License-Identifier: Apache-2.0
//! Part 10, Assembly operations (§6.7): own-occurrence bases, the order of
//! placements, spans, couplings, participant operands, and the definitions
//! a driven feature belongs to.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    assert_no_finding_containing, combine, empty_construction, entity, feature, hole, in_context,
    joint, named, own_occurrences, participant, pick, split_by_tools, unread, with,
};
use crate::design::container::{Container, OrderedKind, Rollback, UnorderedKind};
use crate::design::feature::{Driver, FeatureOperation, OperationFamily};
use crate::design::names::NameKind;
use crate::design::ops::assembly::{
    AssemblyContext, ContextEffect, Place, PlaceSet, PlacementBase,
};
use crate::design::ops::body::{Kept, NewComponent, SplitMode};
use crate::design::ops::dressup::StageHole;
use crate::design::ops::shared::{Coupling, JointConnector, JointKind};
use crate::design::options::BooleanOp;
use crate::design::refs::Member;
use crate::design::scope::DesignScope;
use crate::design::selection::{
    OccurrencePath, PathStep, PickContext, PickTarget, Picked, StepRef,
};
use crate::document::CadIr;
use crate::ids::OccurrenceId;
use crate::products::{Occurrence, ProductDefinition, Storage};
use crate::validate::design::fixture::{self, assert_finding, id};

/// Adds definition `key`, embedded, with no scope of its own.
fn add_definition(ir: &mut CadIr, key: &str) {
    ir.model.product_definitions.push(ProductDefinition::new(
        id("definition", key),
        Storage::Embedded,
    ));
}

/// Adds occurrence `key` of `prototype`, owned by `owner`.
fn add_occurrence(ir: &mut CadIr, key: &str, prototype: &str, owner: &str) {
    let occurrence: OccurrenceId = id("occurrence", key);
    ir.model.occurrences.push(Occurrence::new(
        occurrence.clone(),
        id("definition", prototype),
        None,
    ));
    let owner = format!("test:design:definition#{owner}");
    ir.model
        .product_definitions
        .iter_mut()
        .find(|definition| definition.id.as_str() == owner)
        .unwrap()
        .occurrences
        .push(occurrence);
}

fn place(key: &str, base: PlacementBase) -> FeatureOperation {
    FeatureOperation::Place(Box::new(Place {
        occurrence: id("occurrence", key),
        placement: Some(empty_construction()),
        scale: None,
        base: Some(base),
        constraints: Vec::new(),
        packaged: None,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }))
}

#[test]
fn an_own_occurrence_base_needs_the_prototypes_own_occurrence() {
    const FRAGMENT: &str = "has no occurrence of its own in the scope";
    let mut ir = with(place("a", PlacementBase::OwnOccurrence));
    own_occurrences(&mut ir, &["a"]);
    assert_finding(&ir, FRAGMENT);
    own_occurrences(&mut ir, &["own"]);
    assert_no_finding_containing(&ir, FRAGMENT);
    let mut parent = with(place("a", PlacementBase::Parent));
    own_occurrences(&mut parent, &["a"]);
    assert_no_finding_containing(&parent, FRAGMENT);
}

/// The fixture with a `Place` of `a` in its timeline and a `PlaceSet` of
/// `a` and `b`, in the timeline where `ordered`, else in a group of its own.
fn placed_twice(ordered: bool) -> CadIr {
    let mut ir = with(place("a", PlacementBase::Parent));
    own_occurrences(&mut ir, &["a", "b"]);
    let set = FeatureOperation::PlaceSet(Box::new(PlaceSet {
        occurrences: vec![id("occurrence", "a"), id("occurrence", "b")],
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }));
    if ordered {
        ir.design.features[1].definition.operation = set;
        return ir;
    }
    let mut grouped = fixture::unresolved("set", OperationFamily::PlaceSet);
    grouped.definition.operation = set;
    ir.design.features.push(grouped);
    ir.design.containers.push(Container::Unordered {
        id: id("container", "group"),
        kind: UnorderedKind::Group,
        members: BTreeSet::from([Member::Feature(feature("set"))]),
    });
    ir.design.scopes[0].roots.push(id("container", "group"));
    ir
}

#[test]
fn placements_of_one_occurrence_are_ordered() {
    const FRAGMENT: &str = "are not ordered in one container chain";
    assert_no_finding_containing(&placed_twice(true), FRAGMENT);
    assert_finding(&placed_twice(false), FRAGMENT);
}

/// `placed_twice(ordered)` with its `Place` a combine whose new component is
/// `a`.
fn combined(ordered: bool) -> CadIr {
    let mut ir = placed_twice(ordered);
    let mut creating = combine(
        Some(named(vec![entity(NameKind::Body, 1)])),
        BooleanOp::Join,
    );
    creating.new_component = Some(NewComponent {
        occurrence: id("occurrence", "a"),
    });
    ir.design.features[0].definition.operation = FeatureOperation::Combine(Box::new(creating));
    ir
}

#[test]
fn a_combine_is_before_every_placement_of_its_new_component() {
    const FRAGMENT: &str = "which this combine's new_component creates";
    assert_no_finding_containing(&combined(true), FRAGMENT);
    assert_finding(&combined(false), FRAGMENT);
}

/// The fixture with an assembly `assembly` that has a scope of its own, and a
/// definition `member` that the part places, and the assembly where `twice`.
fn spanned(twice: bool) -> CadIr {
    let mut ir = fixture::document();
    add_definition(&mut ir, "assembly");
    add_definition(&mut ir, "member");
    add_occurrence(&mut ir, "in-part", "member", "part");
    if twice {
        add_occurrence(&mut ir, "in-assembly", "member", "assembly");
    }
    ir.design.scopes.push(DesignScope {
        definition: id("definition", "assembly"),
        roots: Vec::new(),
        active_configuration: None,
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current: Vec::new(),
        unproduced: BTreeSet::new(),
    });
    ir
}

#[test]
fn no_definition_is_spanned_by_two_scopes() {
    const FRAGMENT: &str = "which the scope of";
    assert_no_finding_containing(&spanned(false), FRAGMENT);
    assert_finding(&spanned(true), FRAGMENT);
}

#[test]
fn a_coupling_without_joints_picks_its_members() {
    const FRAGMENT: &str = "a Coupling with no joints has a connector that picks no member";
    let coupling = JointKind::Coupling {
        coupling: Coupling::Screw { pitch: None },
        joints: None,
    };
    assert_finding(&with(joint(coupling.clone())), FRAGMENT);
    let mut picking = joint(coupling);
    if let FeatureOperation::Joint(joint) = &mut picking {
        let face = || JointConnector::Entity(named(vec![entity(NameKind::Face, 1)]));
        joint.connectors = [face(), face()];
    }
    assert_no_finding_containing(&with(picking), FRAGMENT);
}

/// `picked`, through the occurrence `key`.
fn through(key: &str, mut picked: Picked) -> Picked {
    picked.context = Some(PickContext::Occurrence(OccurrencePath(vec![PathStep {
        occurrence: StepRef::Occurrence(id("occurrence", key)),
        configuration: None,
    }])));
    picked
}

/// A hole whose one series stage picks the parts `parts` and makes the part
/// feature `fillet`, in a context whose participants are `participants`.
fn series(parts: &[&str], participants: &[&str]) -> CadIr {
    let mut operation = hole(None, true);
    if let FeatureOperation::Hole(hole) = &mut operation {
        hole.series[0].parts = named(parts.iter().map(|key| participant(key)).collect());
        hole.series[0].hole = StageHole::PartFeature(feature("fillet"));
    }
    let members = named(participants.iter().map(|key| participant(key)).collect());
    in_context(operation, members)
}

#[test]
fn a_series_stage_picks_participants() {
    const FRAGMENT: &str = "picks a part that is no participant";
    let mut ir = series(&["a"], &["a"]);
    own_occurrences(&mut ir, &["a", "b"]);
    assert_no_finding_containing(&ir, FRAGMENT);
    let mut ir = series(&["b"], &["a"]);
    own_occurrences(&mut ir, &["a", "b"]);
    assert_finding(&ir, FRAGMENT);
}

#[test]
fn a_part_feature_stage_names_a_hole_of_a_part_placed_once_among_the_participants() {
    const OTHER: &str = "which is no feature of the part's definition";
    const ONCE: &str = "occurrences among the participants; it has one";
    let mut ir = series(&["a"], &["a"]);
    own_occurrences(&mut ir, &["a"]);
    assert_no_finding_containing(&ir, OTHER);
    assert_no_finding_containing(&ir, ONCE);
    let mut ir = series(&["a"], &["a", "b"]);
    own_occurrences(&mut ir, &["a", "b"]);
    assert_finding(&ir, ONCE);
    let mut ir = series(&["a"], &["a"]);
    add_definition(&mut ir, "other");
    add_occurrence(&mut ir, "a", "other", "part");
    assert_finding(&ir, OTHER);
}

/// A split with no targets that keeps the piece picked through `piece`, in
/// a context whose participant is `a`.
fn pieces(piece: &str) -> CadIr {
    let mut operation = split_by_tools(None);
    if let FeatureOperation::Split(split) = &mut operation {
        if let SplitMode::ByTools { kept, .. } = &mut split.mode {
            *kept = Some(Kept::Pieces {
                keep: Some(Box::new(named(vec![through(
                    piece,
                    entity(NameKind::Face, 1),
                )]))),
                remove: None,
            });
        }
    }
    let mut ir = in_context(operation, named(vec![participant("a")]));
    own_occurrences(&mut ir, &["a", "b"]);
    ir
}

#[test]
fn a_piece_pick_of_an_assembly_split_is_of_a_participant() {
    const FRAGMENT: &str = "a piece pick targets an entity of no participant";
    assert_no_finding_containing(&pieces("a"), FRAGMENT);
    assert_finding(&pieces("b"), FRAGMENT);
}

/// The fixture's extrude driven by the cut of a second scope, `assembly`,
/// whose `Definitions` participant is `a`, an occurrence of `prototype`.
fn driven(prototype: &str) -> CadIr {
    let mut ir = fixture::document();
    add_definition(&mut ir, "assembly");
    if prototype != "part" {
        add_definition(&mut ir, prototype);
    }
    add_occurrence(&mut ir, "a", prototype, "assembly");
    let mut cut = fixture::unresolved("cut", OperationFamily::Extrude);
    cut.definition.assembly = Some(AssemblyContext {
        participants: BTreeMap::from([(
            Some(ContextEffect::Definitions),
            named(vec![participant("a")]),
        )]),
    });
    ir.design.features.push(cut);
    ir.design.containers.push(Container::Ordered {
        id: id("container", "assembly"),
        kind: OrderedKind::Timeline,
        members: vec![Member::Feature(feature("cut"))],
        folders: Vec::new(),
        rollback: Some(Rollback::End),
        placement: None,
    });
    ir.design.scopes.push(DesignScope {
        definition: id("definition", "assembly"),
        roots: vec![id("container", "assembly")],
        active_configuration: None,
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current: Vec::new(),
        unproduced: BTreeSet::new(),
    });
    ir.design.features[0].driven_by = Some(Driver::Feature(feature("cut")));
    ir
}

#[test]
fn a_driven_feature_belongs_to_a_definition_its_driver_changes() {
    const FRAGMENT: &str = "whose Definitions participants resolve in no definition";
    assert_no_finding_containing(&driven("part"), FRAGMENT);
    assert_finding(&driven("other"), FRAGMENT);
}

#[test]
fn an_unlisted_participant_leaves_the_operand_rules_unchecked() {
    let mut ir = series(&["b"], &[]);
    if let Some(context) = &mut ir.design.features[0].definition.assembly {
        for selection in context.participants.values_mut() {
            *selection = unread();
        }
    }
    own_occurrences(&mut ir, &["a", "b"]);
    assert_no_finding_containing(&ir, "picks a part that is no participant");
}

#[test]
fn coupling_joints_pick_joint_features() {
    let coupling = || {
        joint(JointKind::Coupling {
            coupling: Coupling::Screw { pitch: None },
            joints: Some(named(vec![
                pick(PickTarget::Feature(feature("fillet"))),
                pick(PickTarget::Feature(feature("other"))),
            ])),
        })
    };
    let mut ir = with(coupling());
    ir.design
        .features
        .push(fixture::unresolved("other", OperationFamily::Extrude));
    assert_finding(&ir, "which is no Joint feature");
    for feature in &mut ir.design.features[1..] {
        feature.definition.operation = FeatureOperation::Unresolved {
            family: OperationFamily::Joint,
        };
    }
    assert_no_finding_containing(&ir, "which is no Joint feature");
}

#[test]
fn a_coupling_native_joint_pick_counts_toward_no_two() {
    let coupling = |picks| {
        with(joint(JointKind::Coupling {
            coupling: Coupling::Screw { pitch: None },
            joints: Some(named(picks)),
        }))
    };
    assert_finding(
        &coupling(vec![
            pick(PickTarget::Feature(feature("fillet"))),
            pick(PickTarget::Native { within: None }),
        ]),
        "Coupling.joints picks other than two other Joint features",
    );
}
