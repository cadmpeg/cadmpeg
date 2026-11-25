// SPDX-License-Identifier: Apache-2.0
//! The sheet-metal state rules on the fixture's timeline.

use std::collections::{BTreeMap, BTreeSet};

use super::super::fixture::{self, design_findings, id};
use crate::design::container::{Container, UnorderedKind};
use crate::design::dimension::Dimension;
use crate::design::feature::{FeatureOperation, OperationFamily};
use crate::design::names::{NameKind, TopoName};
use crate::design::ops::shared::SheetCutNormal;
use crate::design::ops::sheet_metal::{SheetMetal, SheetMetalKind};
use crate::design::ops::swept::Extrude;
use crate::design::options::{Boolean, BooleanOp, SolidOutput};
use crate::design::refs::Member;
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
