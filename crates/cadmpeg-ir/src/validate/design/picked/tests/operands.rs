// SPDX-License-Identifier: Apache-2.0
//! The operand rules on the fixture cube: piece picks, mate connectors,
//! federation seeds, and break-corner edges.

use std::collections::BTreeMap;

use super::super::super::fixture;
use super::{body, bottom, current, edge, face, positive, ruled, sheet, with};
use crate::design::construction::{Construction, ConstructionKind};
use crate::design::feature::FeatureOperation;
use crate::design::ops::body::{
    BodyMate, Kept, Split, SplitMode, SplitTool, TransformBodies, TransformStep, Transformation,
};
use crate::design::ops::shared::{JointConnector, JointKind};
use crate::design::ops::sheet_metal::{CornerBreak, SheetMetalKind};
use crate::design::ops::surface::Knit;
use crate::design::options::SheetOutput;
use crate::design::refs::EntityRef;
use crate::design::selection::{Anchor, MemberOrder, Selection};
use crate::document::CadIr;
use crate::ids::BodyId;

fn empty<K: ConstructionKind>() -> Construction<K> {
    Construction {
        inputs: Vec::new(),
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved: None,
        resolved_in: BTreeMap::new(),
    }
}

/// A face of the cube other than its bottom.
fn side(ir: &CadIr) -> usize {
    (bottom(ir) + 1) % ir.geometry.faces.len()
}

#[test]
fn a_split_piece_pick_lies_in_the_split_targets() {
    let split = |ir: &CadIr, piece: usize| {
        FeatureOperation::Split(Box::new(Split {
            mode: SplitMode::ByTools {
                targets: Some(current(vec![face(ir, bottom(ir))])),
                tools: SplitTool::Plane(Box::new(empty())),
                method: None,
                kept: Some(Kept::Pieces {
                    keep: Some(Box::new(current(vec![face(ir, piece)]))),
                    remove: None,
                }),
            },
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }))
    };
    let rule = "design.piece-pick-outside-split-operands";
    let mut ir = fixture::document();
    with(&mut ir, |ir| split(ir, bottom(ir)));
    assert!(ruled(&ir, rule).is_empty());
    with(&mut ir, |ir| split(ir, side(ir)));
    assert_eq!(ruled(&ir, rule).len(), 1);
}

#[test]
fn a_constrained_mate_first_connector_picks_a_moved_body() {
    let transform = |ir: &CadIr, moved: EntityRef| {
        let connector = || JointConnector::Entity(current(vec![face(ir, bottom(ir))]));
        FeatureOperation::TransformBodies(Box::new(TransformBodies {
            bodies: current(vec![moved]),
            transformation: Transformation {
                steps: vec![TransformStep::Constrained {
                    mates: vec![BodyMate {
                        connectors: [connector(), connector()],
                        kind: JointKind::Coincident { offset: None },
                        alignment: None,
                    }],
                }],
                resolved: None,
            },
            mode: None,
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }))
    };
    let rule = "design.constrained-mate-connector-off-moved-body";
    let mut ir = fixture::document();
    let mut other = ir.geometry.bodies[0].clone();
    other.id = BodyId::try_from(format!("{}-other", other.id.as_str())).unwrap();
    let moved = EntityRef::Body(other.id.clone());
    ir.design.scopes[0].current[0].bodies.push(other.id.clone());
    let part = ir.model.product_definitions.last_mut().unwrap();
    part.bodies.push(other.id.clone());
    ir.geometry.bodies.push(other);
    with(&mut ir, |ir| transform(ir, body(ir)));
    assert!(ruled(&ir, rule).is_empty());
    with(&mut ir, |ir| transform(ir, moved.clone()));
    assert_eq!(ruled(&ir, rule).len(), 1);
}

#[test]
fn a_federation_seed_is_an_element_of_the_knit() {
    let knit = |ir: &CadIr, seed: usize| {
        FeatureOperation::Knit(Box::new(Knit {
            elements: current(vec![face(ir, bottom(ir))]),
            tolerance: None,
            simplify: None,
            healing: None,
            federation: Some(current(vec![face(ir, seed)])),
            output: SheetOutput {
                solid: None,
                boolean: None,
            },
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }))
    };
    let rule = "design.federation-seed-outside-knit-elements";
    let mut ir = fixture::document();
    with(&mut ir, |ir| knit(ir, bottom(ir)));
    assert!(ruled(&ir, rule).is_empty());
    with(&mut ir, |ir| knit(ir, side(ir)));
    assert_eq!(ruled(&ir, rule).len(), 1);
}

/// `selection` anchored in the state entering its feature.
fn input<O: MemberOrder>(mut selection: Selection<O>) -> Selection<O> {
    selection.at = Anchor::Input;
    selection
}

#[test]
fn a_break_corner_edge_is_no_shorter_than_the_sheet() {
    let rule = "design.break-corner-edge-shorter-than-sheet";
    let mut ir = fixture::document();
    let corner = sheet(SheetMetalKind::BreakCorner {
        corners: input(current(vec![edge(&ir, 0)])),
        size: positive(0.5),
        shape: CornerBreak::Chamfer,
    });
    ir.design.features[1].definition.operation = corner;
    for (thickness, found) in [(0.1, 0), (100.0, 1)] {
        with(&mut ir, |_| {
            sheet(SheetMetalKind::Wall {
                profile: current(Vec::new()),
                thickness: Some(positive(thickness)),
                side: None,
                rule: None,
            })
        });
        assert_eq!(ruled(&ir, rule).len(), found, "thickness {thickness}");
    }
    with(&mut ir, |_| FeatureOperation::Unresolved {
        family: crate::design::feature::OperationFamily::Extrude,
    });
    assert!(ruled(&ir, rule).is_empty());
}
