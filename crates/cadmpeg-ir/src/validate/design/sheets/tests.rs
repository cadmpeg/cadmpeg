// SPDX-License-Identifier: Apache-2.0
//! The sheet-metal state rules on the fixture's timeline.

use std::collections::{BTreeMap, BTreeSet};

use super::super::fixture::{self, design_findings, id};
use crate::design::configuration::FeatureStateOverride;
use crate::design::container::{Container, UnorderedKind};
use crate::design::dimension::Dimension;
use crate::design::feature::{FeatureOperation, OperationFamily};
use crate::design::names::{NameKind, TopoName};
use crate::design::ops::body::{DeleteBodies, Retention};
use crate::design::ops::shared::SheetCutNormal;
use crate::design::ops::sheet_metal::{SheetMetal, SheetMetalKind};
use crate::design::ops::swept::Extrude;
use crate::design::options::{Boolean, BooleanOp, SolidOutput};
use crate::design::refs::{EntityRef, Member};
use crate::design::selection::{Anchor, Members, PickTarget, Picked, Picks, Selection};
use crate::document::CadIr;
use crate::scalar::PositiveLength;

fn ruled(ir: &CadIr, rule: &str) -> usize {
    design_findings(ir)
        .into_iter()
        .filter(|finding| finding.rule.as_str() == rule)
        .count()
}

fn selection<O: crate::design::selection::MemberOrder>(index: u32) -> Selection<O> {
    let pick = Picked {
        target: PickTarget::Entity(TopoName::Generated {
            kind: NameKind::Body,
            feature: fixture::feature("extrude"),
            role: None,
            generator: None,
            index: Some(index),
        }),
        context: None,
        native: None,
        domain: None,
        witness: None,
        resolved: None,
    };
    Selection {
        at: Anchor::Current,
        picks: Picks::Named(Members::new(vec![pick]).unwrap()),
        native: None,
    }
}

fn sheet(kind: SheetMetalKind) -> FeatureOperation {
    FeatureOperation::SheetMetal(Box::new(SheetMetal {
        kind,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }))
}

fn cutting() -> FeatureOperation {
    FeatureOperation::Extrude(Box::new(Extrude {
        profile: selection(0),
        loops: None,
        direction: None,
        origin: None,
        span: None,
        draft: None,
        fit: None,
        outside: None,
        sheet_cut: Some(SheetCutNormal::Defining),
        output: SolidOutput {
            body: None,
            boolean: Some(Boolean::Apply {
                op: BooleanOp::Cut,
                targets: None,
            }),
        },
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }))
}

#[test]
fn a_sheet_cut_extrude_follows_a_sheet_metal_feature() {
    let rule = "design.sheet-cut-no-sheet-metal-body";
    let mut ir = fixture::document();
    ir.design.features[0].definition.operation = cutting();
    assert_eq!(ruled(&ir, rule), 1);
    ir.design.features[0].definition.operation = sheet(SheetMetalKind::Wall {
        profile: selection(0),
        thickness: Some(Dimension::Literal(PositiveLength::new(1.0).unwrap())),
        side: None,
        rule: None,
    });
    ir.design.features[1].definition.operation = cutting();
    assert_eq!(ruled(&ir, rule), 0);
    ir.design.features[0].definition.operation = FeatureOperation::Unresolved {
        family: OperationFamily::Extrude,
    };
    assert_eq!(ruled(&ir, rule), 0);
}

#[test]
fn two_rules_applied_in_one_unordered_container_govern_no_body_together() {
    let rule = "design.apply-rules-govern-one-body";
    let apply = |key: &str, rule: &str, body: u32| {
        let mut feature = fixture::unresolved(key, OperationFamily::SheetMetal);
        feature.definition.operation = sheet(SheetMetalKind::ApplyRule {
            rule: id("sheet-metal-rule", rule),
            bodies: Some(selection(body)),
        });
        feature
    };
    let document = |second: &str, body: u32| {
        let mut ir = fixture::document();
        ir.design.features.push(apply("apply-a", "a", 0));
        ir.design.features.push(apply("apply-b", second, body));
        ir.design.containers.push(Container::Unordered {
            id: id("container", "rules"),
            kind: UnorderedKind::Group,
            members: BTreeSet::from([
                Member::Feature(fixture::feature("apply-a")),
                Member::Feature(fixture::feature("apply-b")),
            ]),
        });
        ir
    };
    assert_eq!(ruled(&document("b", 0), rule), 1);
    assert_eq!(ruled(&document("a", 0), rule), 0);
    assert_eq!(ruled(&document("b", 1), rule), 0);
}

const GOVERN: &str = "design.apply-rules-govern-one-body";

fn wall() -> FeatureOperation {
    sheet(SheetMetalKind::Wall {
        profile: selection(0),
        thickness: Some(Dimension::Literal(PositiveLength::new(1.0).unwrap())),
        side: None,
        rule: None,
    })
}

fn apply(rule: &str, body: Option<u32>) -> FeatureOperation {
    sheet(SheetMetalKind::ApplyRule {
        rule: id("sheet-metal-rule", rule),
        bodies: body.map(selection),
    })
}

/// Appends a feature of `operation` to the design.
fn add(ir: &mut CadIr, key: &str, operation: FeatureOperation) {
    let mut feature = fixture::unresolved(key, OperationFamily::SheetMetal);
    feature.definition.operation = operation;
    ir.design.features.push(feature);
}

/// Appends `member` to the fixture's timeline.
fn append(ir: &mut CadIr, member: Member) {
    let timeline = ir
        .design
        .containers
        .iter_mut()
        .find_map(|container| match container {
            Container::Ordered { id, members, .. } if id.as_str().ends_with("#timeline") => {
                Some(members)
            }
            _ => None,
        })
        .unwrap();
    timeline.push(member);
}

/// Gives the feature `key` the definition of `operation` in configuration
/// "large".
fn in_large(ir: &mut CadIr, key: &str, operation: FeatureOperation) {
    let feature = fixture::feature(key);
    let mut definition = ir
        .design
        .features
        .iter()
        .find(|record| record.id == feature)
        .unwrap()
        .definition
        .clone();
    definition.operation = operation;
    let large = ir
        .design
        .configurations
        .iter_mut()
        .find(|configuration| configuration.id.as_str().ends_with("#large"))
        .unwrap();
    large.overrides.features.insert(
        feature,
        FeatureStateOverride {
            suppressed: None,
            definition: Some(definition),
        },
    );
}

/// The fixture with the timeline ending in an unordered group of the
/// features `first` and `second`.
fn grouped(first: FeatureOperation, second: FeatureOperation) -> CadIr {
    let mut ir = fixture::document();
    add(&mut ir, "first", first);
    add(&mut ir, "second", second);
    ir.design.containers.push(Container::Unordered {
        id: id("container", "rules"),
        kind: UnorderedKind::Group,
        members: BTreeSet::from([
            Member::Feature(fixture::feature("first")),
            Member::Feature(fixture::feature("second")),
        ]),
    });
    append(&mut ir, Member::Container(id("container", "rules")));
    ir
}

#[test]
fn rules_applied_in_force_in_no_one_configuration_govern_no_body_together() {
    let mut ir = grouped(apply("a", Some(0)), wall());
    assert_eq!(ruled(&ir, GOVERN), 0);
    in_large(&mut ir, "second", apply("b", Some(0)));
    assert_eq!(ruled(&ir, GOVERN), 1);
    in_large(&mut ir, "first", wall());
    assert_eq!(ruled(&ir, GOVERN), 0);
}

#[test]
fn rules_applied_in_order_or_to_no_listed_body_are_not_paired() {
    assert_eq!(
        ruled(&grouped(apply("a", None), apply("b", None)), GOVERN),
        0
    );
    let mut ir = fixture::document();
    for (key, rule) in [("first", "a"), ("second", "b")] {
        add(&mut ir, key, apply(rule, Some(0)));
        append(&mut ir, Member::Feature(fixture::feature(key)));
    }
    assert_eq!(ruled(&ir, GOVERN), 0);
}

/// Gives the feature `key` the suppression `suppressed` in configuration
/// "large".
fn suppressed_in_large(ir: &mut CadIr, key: &str, suppressed: bool) {
    let large = ir
        .design
        .configurations
        .iter_mut()
        .find(|configuration| configuration.id.as_str().ends_with("#large"))
        .unwrap();
    large.overrides.features.insert(
        fixture::feature(key),
        FeatureStateOverride {
            suppressed: Some(suppressed),
            definition: None,
        },
    );
}

/// The feature `key` of the design.
fn feature_mut<'d>(ir: &'d mut CadIr, key: &str) -> &'d mut crate::design::feature::Feature {
    let feature = fixture::feature(key);
    ir.design
        .features
        .iter_mut()
        .find(|record| record.id == feature)
        .unwrap()
}

#[test]
fn rules_applied_that_are_never_in_force_together_govern_no_body_together() {
    let mut ir = grouped(apply("a", Some(0)), apply("b", Some(0)));
    assert_eq!(ruled(&ir, GOVERN), 1);
    // "first" is suppressed in the base and in force in "large", where
    // "second" is suppressed.
    feature_mut(&mut ir, "first").suppressed = Some(Dimension::Literal(true));
    suppressed_in_large(&mut ir, "first", false);
    suppressed_in_large(&mut ir, "second", true);
    assert_eq!(ruled(&ir, GOVERN), 0);
    // In force in "large" too, the two govern one body there.
    suppressed_in_large(&mut ir, "second", false);
    assert_eq!(ruled(&ir, GOVERN), 1);
}

#[test]
fn an_arrangement_overriding_a_definition_is_read_over_each_configuration() {
    let mut ir = grouped(apply("a", Some(0)), wall());
    assert_eq!(ruled(&ir, GOVERN), 0);
    let mut definition = feature_mut(&mut ir, "second").definition.clone();
    definition.operation = apply("b", Some(0));
    let mut arrangement = fixture::arrangement(
        "positional",
        crate::design::configuration::ArrangementKind::Positional,
    );
    arrangement.overrides.features.insert(
        fixture::feature("second"),
        FeatureStateOverride {
            suppressed: None,
            definition: Some(definition),
        },
    );
    ir.design.arrangements.push(arrangement);
    assert_eq!(ruled(&ir, GOVERN), 1);
}

#[test]
fn a_suppressed_feature_makes_no_sheet_and_cuts_none() {
    let rule = "design.sheet-cut-no-sheet-metal-body";
    let mut ir = fixture::document();
    ir.design.features[0].definition.operation = wall();
    ir.design.features[1].definition.operation = cutting();
    assert_eq!(ruled(&ir, rule), 0);
    ir.design.features[0].suppressed = Some(Dimension::Literal(true));
    assert_eq!(ruled(&ir, rule), 1);
    // The cut is suppressed wherever the sheet is.
    ir.design.features[1].suppressed = Some(Dimension::Literal(true));
    assert_eq!(ruled(&ir, rule), 0);
    // In "large" the cut is in force and the sheet is not.
    suppressed_in_large(&mut ir, "fillet", false);
    assert_eq!(ruled(&ir, rule), 1);
}

/// A `DeleteBodies` that removes the results of the feature "extrude",
/// narrowed to `resolved` where that is stated.
fn delete_extrude(retain: Retention, resolved: Option<Vec<EntityRef>>) -> FeatureOperation {
    let pick = Picked {
        target: PickTarget::FeatureResult(fixture::feature("extrude")),
        context: None,
        native: None,
        domain: None,
        witness: None,
        resolved,
    };
    FeatureOperation::DeleteBodies(Box::new(DeleteBodies {
        bodies: Selection {
            at: Anchor::Current,
            picks: Picks::Named(Members::new(vec![pick]).unwrap()),
            native: None,
        },
        retain: Some(retain),
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }))
}

#[test]
fn a_narrowed_delete_removes_no_sheet_whole() {
    let rule = "design.sheet-cut-no-sheet-metal-body";
    let mut ir = fixture::document();
    ir.design.features[0].definition.operation = wall();
    let body = EntityRef::Body(ir.geometry.bodies[0].id.clone());
    ir.design.features[1].definition.operation =
        delete_extrude(Retention::DeleteSelected, Some(vec![body]));
    add(&mut ir, "cut", cutting());
    append(&mut ir, Member::Feature(fixture::feature("cut")));
    assert_eq!(ruled(&ir, rule), 0);
}

#[test]
fn a_feature_after_the_delete_keeps_no_sheet() {
    let rule = "design.sheet-cut-no-sheet-metal-body";
    // wall, `then`, `last`, cut.
    let document = |then: FeatureOperation, last: FeatureOperation| {
        let mut ir = fixture::document();
        ir.design.features[0].definition.operation = wall();
        ir.design.features[1].definition.operation = then;
        add(&mut ir, "last", last);
        append(&mut ir, Member::Feature(fixture::feature("last")));
        add(&mut ir, "cut", cutting());
        append(&mut ir, Member::Feature(fixture::feature("cut")));
        ir
    };
    let delete = || delete_extrude(Retention::DeleteSelected, None);
    assert_eq!(
        ruled(&document(delete(), FeatureOperation::MaterialRemoval), rule),
        1
    );
    assert_eq!(
        ruled(&document(FeatureOperation::MaterialRemoval, delete()), rule),
        0
    );
}

#[test]
fn a_sheet_cut_extrude_cuts_a_sheet_no_feature_before_it_deletes() {
    let rule = "design.sheet-cut-no-sheet-metal-body";
    let deleted = |retain| {
        let mut ir = fixture::document();
        ir.design.features[0].definition.operation = wall();
        ir.design.features[1].definition.operation = delete_extrude(retain, None);
        add(&mut ir, "cut", cutting());
        append(&mut ir, Member::Feature(fixture::feature("cut")));
        ir
    };
    assert_eq!(ruled(&deleted(Retention::DeleteSelected), rule), 1);
    assert_eq!(ruled(&deleted(Retention::KeepSelected), rule), 0);
    let mut suppressed = deleted(Retention::DeleteSelected);
    suppressed.design.features[1].suppressed = Some(Dimension::Literal(true));
    assert_eq!(ruled(&suppressed, rule), 0);
}
