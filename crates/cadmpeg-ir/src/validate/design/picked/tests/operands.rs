// SPDX-License-Identifier: Apache-2.0
//! The operand rules on the fixture cube: piece picks, mate connectors,
//! federation seeds, and break-corner edges.

use std::collections::BTreeMap;

use cadmpeg_core::text::NonBlankString;

use super::super::super::fixture::{self, id};
use super::{body, bottom, current, edge, face, positive, ruled, sheet, with};
use crate::design::construction::{Construction, ConstructionKind};
use crate::design::feature::FeatureOperation;
use crate::design::ops::assembly::{AssemblyContext, ContextEffect};
use crate::design::ops::body::{
    BodyMate, Kept, Split, SplitMode, SplitTool, TransformBodies, TransformStep, Transformation,
};
use crate::design::ops::shared::{JointConnector, JointKind};
use crate::design::ops::sheet_metal::{CornerBreak, SheetMetalKind};
use crate::design::ops::surface::Knit;
use crate::design::options::SheetOutput;
use crate::design::refs::{ConfigurationRef, EntityRef};
use crate::design::selection::{
    Anchor, DefinitionRef, MemberOrder, Members, OccurrencePath, PathStep, PickContext, PickTarget,
    Picked, Picks, Selection, StepRef,
};
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

/// The path through the occurrences `keys`.
fn steps(keys: &[&str]) -> OccurrencePath {
    let steps = keys.iter().map(|key| PathStep {
        occurrence: StepRef::Occurrence(id("occurrence", key)),
        configuration: None,
    });
    OccurrencePath(steps.collect())
}

/// A pick of the component that `context` reads.
fn component(context: Option<PickContext>) -> Picked {
    Picked {
        target: PickTarget::Component,
        context,
        native: None,
        domain: None,
        witness: None,
        resolved: None,
    }
}

fn named<O: MemberOrder>(picks: Vec<Picked>) -> Selection<O> {
    Selection {
        at: Anchor::Current,
        picks: Picks::Named(Members::new(picks).unwrap()),
        native: None,
    }
}

/// The component under the occurrences `path` steps through.
fn under<O: MemberOrder>(
    path: &[&str],
    context: fn(OccurrencePath) -> PickContext,
) -> Selection<O> {
    named(vec![component(Some(context(steps(path))))])
}

/// A piece pick of a split without targets that lies under no participant
/// gets one finding, from the participant rule alone.
#[test]
fn a_piece_pick_under_no_participant_is_reported_once() {
    let split = |piece: &[&str]| {
        FeatureOperation::Split(Box::new(Split {
            mode: SplitMode::ByTools {
                targets: None,
                tools: SplitTool::Plane(Box::new(empty())),
                method: None,
                kept: Some(Kept::Pieces {
                    keep: None,
                    remove: Some(Box::new(under(piece, PickContext::Occurrence))),
                }),
            },
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }))
    };
    let participant = "design.piece-pick-targets-entity-no-participant";
    let operands = "design.piece-pick-outside-split-operands";
    let mut ir = fixture::document();
    ir.design.features[0].definition.assembly = Some(AssemblyContext {
        participants: BTreeMap::from([(
            Some(ContextEffect::Definitions),
            under(&["a"], PickContext::Occurrence),
        )]),
    });
    with(&mut ir, |_| split(&["a", "c"]));
    assert!(ruled(&ir, participant).is_empty());
    assert!(ruled(&ir, operands).is_empty());
    with(&mut ir, |_| split(&["b"]));
    assert_eq!(ruled(&ir, participant).len(), 1);
    assert!(ruled(&ir, operands).is_empty());
}

/// §5.8: an `InContext` piece pick runs from its assembly, and `own` places
/// the owner's definition there, so a participant's path from the owner lies
/// under `own`.
#[test]
fn an_in_context_piece_pick_is_read_through_its_own_place_in_the_assembly() {
    let in_context = |target| PickContext::InContext {
        assembly: DefinitionRef {
            definition: id("definition", "assembly"),
            configuration: None,
        },
        own: Some(OccurrencePath(vec![PathStep {
            occurrence: StepRef::Occurrence(id("occurrence", "p")),
            configuration: None,
        }])),
        target: Some(target),
    };
    let split = |piece: &[&str]| {
        FeatureOperation::Split(Box::new(Split {
            mode: SplitMode::ByTools {
                targets: None,
                tools: SplitTool::Plane(Box::new(empty())),
                method: None,
                kept: Some(Kept::Pieces {
                    keep: Some(Box::new(under(piece, in_context))),
                    remove: None,
                }),
            },
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }))
    };
    let participant = "design.piece-pick-targets-entity-no-participant";
    let mut ir = fixture::document();
    ir.design.features[0].definition.assembly = Some(AssemblyContext {
        participants: BTreeMap::from([(
            Some(ContextEffect::Definitions),
            under(&["a"], PickContext::Occurrence),
        )]),
    });
    with(&mut ir, |_| split(&["p", "a", "c"]));
    assert!(ruled(&ir, participant).is_empty());
    with(&mut ir, |_| split(&["p", "b"]));
    assert_eq!(ruled(&ir, participant).len(), 1);
    with(&mut ir, |_| split(&["a", "c"]));
    assert_eq!(ruled(&ir, participant).len(), 1);
}

/// A step's configuration selects a variant of its occurrence, so a piece
/// lies under a participant by its occurrences alone. A participant with no
/// occurrence path from a definition leaves every piece unjudged.
#[test]
fn a_piece_pick_is_judged_by_its_occurrences_against_every_participant() {
    let split = |piece: Selection<_>| {
        FeatureOperation::Split(Box::new(Split {
            mode: SplitMode::ByTools {
                targets: None,
                tools: SplitTool::Plane(Box::new(empty())),
                method: None,
                kept: Some(Kept::Pieces {
                    keep: Some(Box::new(piece)),
                    remove: None,
                }),
            },
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }))
    };
    let participants = |extra: Vec<Picked>| {
        let mut picks = vec![component(Some(PickContext::Occurrence(steps(&["a"]))))];
        picks.extend(extra);
        Some(AssemblyContext {
            participants: BTreeMap::from([(Some(ContextEffect::Definitions), named(picks))]),
        })
    };
    let rule = "design.piece-pick-targets-entity-no-participant";
    let mut ir = fixture::document();
    ir.design.features[0].definition.assembly = participants(Vec::new());
    let mut variant = steps(&["a", "c"]);
    let large = NonBlankString::new("large").unwrap();
    variant.0[0].configuration = Some(ConfigurationRef::Named(large));
    let variant = component(Some(PickContext::Occurrence(variant)));
    with(&mut ir, |_| split(named(vec![variant])));
    assert!(ruled(&ir, rule).is_empty());
    with(&mut ir, |_| split(under(&["b"], PickContext::Occurrence)));
    assert_eq!(ruled(&ir, rule).len(), 1);
    let definition = PickContext::Definition(DefinitionRef {
        definition: id("definition", "part"),
        configuration: None,
    });
    for unrooted in [Some(definition), None] {
        ir.design.features[0].definition.assembly = participants(vec![component(unrooted)]);
        assert!(ruled(&ir, rule).is_empty());
    }
}

#[test]
fn a_constrained_mate_first_connector_picks_a_moved_body() {
    let transform = |ir: &CadIr, moved: EntityRef, kind: JointKind| {
        let connector = || JointConnector::Entity(current(vec![face(ir, bottom(ir))]));
        FeatureOperation::TransformBodies(Box::new(TransformBodies {
            bodies: current(vec![moved]),
            transformation: Transformation {
                steps: vec![TransformStep::Constrained {
                    mates: vec![BodyMate {
                        connectors: [connector(), connector()],
                        kind,
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
    let coincident = || JointKind::Coincident { offset: None };
    with(&mut ir, |ir| transform(ir, body(ir), coincident()));
    assert!(ruled(&ir, rule).is_empty());
    with(&mut ir, |ir| transform(ir, moved.clone(), coincident()));
    assert_eq!(ruled(&ir, rule).len(), 1);
    let native = JointKind::Native(NonBlankString::new("test:mate").unwrap());
    with(&mut ir, |ir| transform(ir, moved.clone(), native));
    assert!(ruled(&ir, rule).is_empty());
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

#[test]
fn a_break_corner_reads_the_sheet_of_its_state_configuration() {
    let rule = "design.break-corner-edge-shorter-than-sheet";
    let mut ir = fixture::document();
    ir.design.features[1].definition.operation = sheet(SheetMetalKind::BreakCorner {
        corners: input(current(vec![edge(&ir, 0)])),
        size: positive(0.5),
        shape: CornerBreak::Chamfer,
    });
    let wall = |thickness| {
        sheet(SheetMetalKind::Wall {
            profile: current(Vec::new()),
            thickness: Some(positive(thickness)),
            side: None,
            rule: None,
        })
    };
    with(&mut ir, |_| wall(0.1));
    assert!(ruled(&ir, rule).is_empty());
    // The current state is the active configuration's, "base", which gives
    // the wall a thickness longer than every edge of the cube.
    let mut thick = ir.design.features[0].definition.clone();
    thick.operation = wall(100.0);
    ir.design.configurations[0].overrides.features.insert(
        fixture::feature("extrude"),
        crate::design::configuration::FeatureStateOverride {
            suppressed: None,
            definition: Some(thick),
        },
    );
    assert_eq!(ruled(&ir, rule).len(), 1);
}
