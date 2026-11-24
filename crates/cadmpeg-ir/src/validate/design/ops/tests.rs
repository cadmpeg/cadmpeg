// SPDX-License-Identifier: Apache-2.0
//! The feature-vocabulary rules on variations of the design fixture: the
//! fixture's extrude takes each operation in turn.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::text::NonBlankString;

use super::super::fixture::{self, assert_finding, assert_no_design_finding, design_findings, id};
use crate::design::construction::{Construction, ConstructionKind, FrameAxis, PostStep};
use crate::design::container::Container;
use crate::design::dimension::Dimension;
use crate::design::evaluation::EvaluationStatus;
use crate::design::feature::{Driver, FeatureOperation, OperationFamily};
use crate::design::names::{InstanceIndex, NameKind, TopoName};
use crate::design::ops::assembly::{
    AssemblyContext, ComponentInstance, ComponentPattern, ContextEffect, MirrorComponents,
    MirrorResult, Place, PlaceSet, PlacementBase,
};
use crate::design::ops::body::{
    Combine, DeleteBodies, InstanceChange, InstanceOverride, Kept, NewComponent, Pattern,
    PatternInstances, PatternSeeds, PieceForm, ScaleFactors, Split, SplitMethod, SplitMode,
    SplitTool, TemplateInput, TemplateInstance, TransformBodies, TransformMode, TransformStep,
    Transformation,
};
use crate::design::ops::datum::{Datum, DatumElement};
use crate::design::ops::dressup::{
    BlendControls, Bore, Draft, DraftAngle, DraftSet, Fillet, FilletSection, FilletSet, FilletSize,
    Hole, HoleHead, HolePositions, HoleShape, StageHole, StandardSize, ThreadSpec, Transition,
    TransitionKind,
};
use crate::design::ops::shared::{
    AxisLimits, Bounds, CompositeStage, Coupling, Joint, JointConnector, JointKind, JointMotion,
    JointOffset, Limit, LinearLayout, LinearRun, PatternLayout, PatternRun, RunSides, Spacing,
    Span,
};
use crate::design::ops::sheet_metal::{
    BendAllowance, BendSpec, Flange, FlangeWidth, KFactorRow, KFactorTable, SheetMetal,
    SheetMetalKind, SheetMetalRule,
};
use crate::design::ops::sketch::{
    DefiningRole, EntityDefinition, EntityRole, Operand, Planar, PlanarCurve, Sketch,
    SketchConstraint, SketchEntity, SketchPlane, SketchRelation, SketchSolve,
};
use crate::design::ops::surface::{Knit, MeshFeature, MeshKind};
use crate::design::ops::swept::{
    Extrude, ExtrudeDraft, HelicalSection, HelicalSweep, Helix, HelixShape, HelixStart, HoleDraft,
    Loft, PathExtent, Primitive, PrimitiveShape, SpineLimits, Sweep, SweepControls, SweepSection,
};
use crate::design::options::{
    BodyForm, Boolean, BooleanOp, NativeOption, SheetOutput, SolidOutput, Targets,
};
use crate::design::parameter::DesignParameter;
use crate::design::refs::Member;
use crate::design::selection::{
    Anchor, MemberOrder, Members, OccurrencePath, PathStep, PickContext, PickTarget, Picked, Picks,
    Propagation, RuleTerm, Selection, SelectionRule, SetSelection, StepRef, Unordered,
};
use crate::design::units::ValueKind;
use crate::document::CadIr;
use crate::features::{FeatureId, ParameterId};
use crate::ids::OccurrenceId;
use crate::math::Point2;
use crate::products::{ExternalDocument, Occurrence, ProductDefinition, Storage};
use crate::report::Severity;
use crate::scalar::{Angle, Fraction, Length, NonZeroReal, PositiveLength};
use crate::sketches::{SketchConstraintId, SketchEntityId, SketchId};
use crate::transform::Transform;
use crate::units::{FinitePoint2, UnitVector3};

fn empty_construction<K: ConstructionKind>() -> Construction<K> {
    Construction {
        inputs: Vec::new(),
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved: None,
        resolved_in: BTreeMap::new(),
    }
}

fn feature(key: &str) -> FeatureId {
    fixture::feature(key)
}

fn pick(target: PickTarget) -> Picked {
    Picked {
        target,
        context: None,
        native: None,
        domain: None,
        witness: None,
        resolved: None,
    }
}

fn named<O: MemberOrder>(picks: Vec<Picked>) -> Selection<O> {
    Selection {
        at: Anchor::Input,
        picks: Picks::Named(Members::new(picks).unwrap()),
        native: None,
    }
}

fn unread<O: MemberOrder>() -> Selection<O> {
    Selection {
        at: Anchor::Input,
        picks: Picks::Unread,
        native: None,
    }
}

fn entity(kind: NameKind, index: u32) -> Picked {
    pick(PickTarget::Entity(TopoName::Generated {
        kind,
        feature: feature("extrude"),
        role: None,
        generator: None,
        index: Some(index),
    }))
}

fn sketch_id(key: &str) -> SketchId {
    id("sketch", key)
}

fn sketch_pick() -> Picked {
    pick(PickTarget::Sketch(sketch_id("profile")))
}

fn length(value: f64) -> Length {
    Length::new(value).unwrap()
}

fn positive(value: f64) -> PositiveLength {
    PositiveLength::new(value).unwrap()
}

fn solid() -> SolidOutput {
    SolidOutput {
        body: None,
        boolean: None,
    }
}

fn with(operation: FeatureOperation) -> CadIr {
    let mut ir = fixture::document();
    ir.design.features[0].definition.operation = operation;
    ir
}

fn extrude() -> Extrude {
    Extrude {
        profile: named(vec![sketch_pick()]),
        loops: None,
        direction: None,
        origin: None,
        span: None,
        draft: None,
        fit: None,
        outside: None,
        sheet_cut: None,
        output: solid(),
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }
}

/// The sketch `sketch_pick` picks, owned by the extrude.
fn profile_sketch() -> Sketch {
    Sketch {
        id: sketch_id("profile"),
        owner: feature("extrude"),
        name: None,
        plane: SketchPlane::Block,
        solve: SketchSolve {
            state: None,
            status: EvaluationStatus::Current,
        },
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
        native_ref: None,
    }
}

fn extruding(change: impl FnOnce(&mut Extrude)) -> CadIr {
    let mut operation = extrude();
    change(&mut operation);
    let picks_sketch = matches!(
        &operation.profile.picks,
        Picks::Named(picks) if picks.as_slice().contains(&sketch_pick())
    );
    let mut ir = with(FeatureOperation::Extrude(Box::new(operation)));
    if picks_sketch {
        let mut sketch = fixture::unresolved("sketch", OperationFamily::Sketch);
        sketch.definition.operation = FeatureOperation::Sketch(sketch_id("profile"));
        ir.design.features.push(sketch);
        if let Container::Ordered { members, .. } = &mut ir.design.containers[0] {
            members.insert(0, Member::Feature(feature("sketch")));
        }
        ir.design.sketches.push(Sketch {
            owner: feature("sketch"),
            ..profile_sketch()
        });
    }
    ir
}

#[track_caller]
fn assert_warning(ir: &CadIr, fragment: &str) {
    let findings = design_findings(ir);
    assert!(
        findings
            .iter()
            .any(|f| f.severity == Severity::Warning && f.message.contains(fragment)),
        "no design warning containing {fragment:?} in {findings:#?}"
    );
}

#[test]
fn a_typed_extrude_breaks_no_rule() {
    assert_no_design_finding(&extruding(|_| {}));
}

#[test]
fn an_unread_profile_and_an_unresolved_operation_are_not_checked() {
    assert_no_design_finding(&extruding(|e| e.profile = unread()));
    assert_no_design_finding(&fixture::document());
}

// ----------------------------------------------------------------- shared option types

#[test]
fn a_native_key_is_in_one_map() {
    let option = NativeOption::Bool(Dimension::Literal(true));
    let key = crate::codec_name!("sldprt:flag");
    assert_finding(
        &extruding(|e| {
            e.native_options.insert(key.clone(), option.clone());
            e.source_settings.insert(key.clone(), option.clone());
        }),
        "in both native_options and source_settings",
    );
}

#[test]
fn a_profile_holds_a_pick_of_what_profiles_are_made_of() {
    assert_finding(
        &extruding(|e| e.profile = named(Vec::new())),
        "holds no pick",
    );
    assert_finding(
        &extruding(|e| e.profile = named(vec![entity(NameKind::Vertex, 1)])),
        "which no profile is made of",
    );
    assert_no_design_finding(&extruding(|e| {
        e.profile = named(vec![entity(NameKind::Face, 1), entity(NameKind::Edge, 2)]);
    }));
}

#[test]
fn a_merged_name_has_a_kind_only_where_its_members_share_one() {
    let merged = |kinds: [NameKind; 2]| {
        let from = kinds
            .into_iter()
            .zip(1..)
            .map(|(kind, index)| match entity(kind, index).target {
                PickTarget::Entity(name) => name,
                _ => unreachable!(),
            })
            .collect();
        pick(PickTarget::Entity(TopoName::Merged {
            feature: None,
            from,
        }))
    };
    assert_finding(
        &extruding(|e| e.profile = named(vec![merged([NameKind::Vertex; 2])])),
        "which no profile is made of",
    );
    assert_no_finding_containing(
        &extruding(|e| e.profile = named(vec![merged([NameKind::Vertex, NameKind::Face])])),
        "which no profile is made of",
    );
}

#[test]
fn booleans_fit_their_outputs() {
    let apply = |op| Boolean::Apply { op, targets: None };
    assert_finding(
        &extruding(|e| e.output.boolean = Some(apply(BooleanOp::Assemble))),
        "occurs only as a Combine's op",
    );
    assert_finding(
        &extruding(|e| {
            e.output.body = Some(BodyForm::Solid);
            e.output.boolean = Some(apply(BooleanOp::Sew));
        }),
        "Sew occurs only in an output whose result is a sheet",
    );
    assert_no_design_finding(&extruding(|e| {
        e.output.body = Some(BodyForm::Sheet);
        e.output.boolean = Some(apply(BooleanOp::Sew));
    }));
    assert_finding(
        &extruding(|e| {
            e.output.boolean = Some(Boolean::Apply {
                op: BooleanOp::Join,
                targets: Some(Targets::Selected(named(vec![entity(NameKind::Face, 1)]))),
            });
        }),
        "its picks resolve to bodies",
    );
}

#[test]
fn spans_have_distinct_ends_and_nonzero_amounts() {
    assert_finding(
        &extruding(|e| {
            e.span = Some(Span::Between {
                start: Box::new(Limit::AtOrigin),
                end: Box::new(Limit::AtOrigin),
            });
        }),
        "AtOrigin at both start and end",
    );
    assert_finding(
        &extruding(|e| {
            e.span = Some(Span::Symmetric {
                amount: Dimension::Literal(length(0.0)),
                measure: None,
            });
        }),
        "symmetric span of zero",
    );
}

// ----------------------------------------------------------------- swept solids and the loft

#[test]
fn an_extrude_draft_states_an_angle_where_its_span_allows() {
    let draft = |start| ExtrudeDraft {
        end: None,
        start,
        neutral: None,
        holes: None,
    };
    assert_finding(
        &extruding(|e| e.draft = Some(draft(None))),
        "states neither end nor start",
    );
    let angle = || Dimension::Literal(Angle::new(0.1).unwrap());
    assert_finding(
        &extruding(|e| {
            e.span = Some(Span::Symmetric {
                amount: Dimension::Literal(length(2.0)),
                measure: None,
            });
            e.draft = Some(draft(Some(angle())));
        }),
        "under a symmetric span states start",
    );
    assert_finding(
        &extruding(|e| {
            e.draft = Some(ExtrudeDraft {
                end: Some(angle()),
                holes: Some(HoleDraft::Angles {
                    end: angle(),
                    start: Some(angle()),
                }),
                ..draft(None)
            });
        }),
        "HoleDraft::Angles states start",
    );
}

#[test]
fn primitives_have_their_own_placement_and_admitted_literals() {
    let primitive = |shape| {
        FeatureOperation::Primitive(Box::new(Primitive {
            shape,
            placement: None,
            output: solid(),
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }))
    };
    let prism = |sides| PrimitiveShape::Prism {
        sides: Dimension::Literal(sides),
        circumradius: Dimension::Literal(positive(1.0)),
        height: Dimension::Literal(positive(1.0)),
    };
    assert_no_design_finding(&with(primitive(prism(3))));
    assert_finding(&with(primitive(prism(2))), "a count is at least 3");
    let mut placed = with(primitive(prism(4)));
    placed.design.features[0].placement = Some(Box::new(empty_construction()));
    assert_finding(&placed, "states a Feature.placement");
    let literal = |value| Dimension::Literal(length(value));
    let wedge = PrimitiveShape::Wedge {
        y: [literal(0.0), literal(1.0)],
        base: [literal(0.0), literal(0.0), literal(0.0), literal(1.0)],
        top: [literal(0.0), literal(0.0), literal(0.0), literal(1.0)],
    };
    assert_finding(&with(primitive(wedge)), "degenerate in one axis");
}

#[test]
fn a_loft_has_two_stations_and_no_lone_cross_section() {
    let loft = Loft {
        start: None,
        sections: Vec::new(),
        end: None,
        cross: Vec::new(),
        spine: None,
        guides: Vec::new(),
        correspondence: None,
        shape: crate::design::ops::swept::LoftShape {
            style: None,
            max_degree: None,
            rebuild: None,
            ruled: None,
            closed: None,
            relimit: None,
            merge_tangent_faces: None,
            smoothing: None,
            section_law: None,
        },
        output: solid(),
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    };
    assert_finding(
        &with(FeatureOperation::Loft(Box::new(loft))),
        "it needs at least 2",
    );
}

// ----------------------------------------------------------------- dress-up and holes

fn fillet(sets: Vec<FilletSet>, transitions: Vec<Transition>) -> FeatureOperation {
    FeatureOperation::Fillet(Box::new(Fillet {
        sets,
        controls: BlendControls {
            corner_style: None,
            corners: Vec::new(),
            limits: Vec::new(),
            keep: None,
            transitions,
            overflow: None,
            keep_features: None,
            round_corners: None,
        },
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }))
}

fn edge_set(size: Option<FilletSize>) -> FilletSet {
    FilletSet::Edges {
        edges: named(vec![entity(NameKind::Edge, 1)]),
        section: FilletSection {
            size,
            profile: None,
            weight: None,
        },
        spine: None,
        hold: [None, None],
    }
}

#[test]
fn a_fillet_has_sets_and_transitions_that_fit_them() {
    let radius = Some(FilletSize::Radius(Dimension::Literal(positive(1.0))));
    assert_no_design_finding(&with(fillet(vec![edge_set(radius.clone())], Vec::new())));
    assert_finding(
        &with(fillet(Vec::new(), Vec::new())),
        "Fillet.sets is empty",
    );
    assert_finding(
        &with(fillet(vec![edge_set(Some(FilletSize::Held))], Vec::new())),
        "Held occurs in a set with no hold",
    );
    let blend = Transition {
        between: BTreeSet::from([0]),
        at: None,
        kind: Some(TransitionKind::Blend),
        native_options: BTreeMap::new(),
    };
    assert_finding(
        &with(fillet(vec![edge_set(radius.clone())], vec![blend])),
        "transition 0 has 1 members",
    );
    let sphere = Transition {
        between: BTreeSet::from([0, 1]),
        at: None,
        kind: Some(TransitionKind::CornerSphere {
            radius: None,
            lengths: vec![Dimension::Literal(
                crate::scalar::NonNegativeLength::new(1.0).unwrap(),
            )],
        }),
        native_options: BTreeMap::new(),
    };
    assert_finding(
        &with(fillet(
            vec![edge_set(radius.clone()), edge_set(radius)],
            vec![sphere],
        )),
        "CornerSphere with 1 lengths",
    );
}

#[test]
fn a_draft_anchors_its_two_sided_angles() {
    let slope = || Dimension::Literal(crate::scalar::SlopeAngle::new(0.1).unwrap());
    let draft = |angle, steps| {
        FeatureOperation::Draft(Box::new(Draft {
            sets: vec![DraftSet {
                faces: Some(named(vec![entity(NameKind::Face, 1)])),
                fixed: None,
                angle: Some(angle),
            }],
            pull: None,
            neutral: None,
            parting: None,
            steps,
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }))
    };
    let two = DraftAngle::Two {
        first: slope(),
        second: slope(),
    };
    assert_finding(&with(draft(two, None)), "needs neutral or parting");
    let steps = Some(crate::design::ops::dressup::StepFaces::Tapered);
    assert_finding(
        &with(draft(DraftAngle::One(slope()), steps)),
        "Draft.steps occurs only with parting",
    );
    assert_warning(
        &with(draft(DraftAngle::One(slope()), None)),
        "states none of neutral",
    );
}

fn hole(head: Option<HoleHead>, series: bool) -> FeatureOperation {
    FeatureOperation::Hole(Box::new(Hole {
        positions: HolePositions::Points {
            points: named(vec![entity(NameKind::Vertex, 1)]),
            centers: None,
        },
        start: None,
        direction: None,
        shape: HoleShape::Bore(Bore {
            diameter: Some(Dimension::Literal(positive(5.0))),
            head,
            taper: None,
            bottom: None,
            near_chamfer: None,
            neck_chamfer: None,
            far_chamfer: None,
        }),
        extent: None,
        standard: None,
        thread: Some(ThreadSpec {
            standard: None,
            major_diameter: Some(Dimension::Literal(positive(6.0))),
            minor_diameter: Some(Dimension::Literal(positive(5.5))),
            pitch_diameter: None,
            pitch: None,
            hand: None,
            length: None,
            modeled: None,
        }),
        targets: None,
        series: if series {
            vec![crate::design::ops::dressup::SeriesStage {
                parts: named(Vec::new()),
                hole: crate::design::ops::dressup::StageHole::PartFeature(feature("fillet")),
            }]
        } else {
            Vec::new()
        },
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }))
}

#[test]
fn a_hole_has_a_wider_head_and_a_series_only_in_an_assembly_feature() {
    assert_no_design_finding(&with(hole(None, false)));
    let narrow = HoleHead::Counterbore {
        diameter: Some(Dimension::Literal(positive(4.0))),
        depth: None,
    };
    assert_finding(
        &with(hole(Some(narrow), false)),
        "does not exceed the bore diameter",
    );
    assert_finding(
        &with(hole(None, true)),
        "a Hole with a series is in a definition that states no assembly",
    );
    assert_no_finding_containing(
        &in_assembly(hole(None, true)),
        "a Hole with a series is in a definition",
    );
    assert_finding(&with(hole(None, true)), "which is no Hole feature");
}

// ----------------------------------------------------------------- patterns and body operations

fn linear(sides: Option<RunSides<Length>>, count: u32) -> PatternLayout {
    PatternLayout::Linear(linear_layout(sides, count))
}

fn linear_stage(count: u32) -> CompositeStage {
    CompositeStage::Linear(linear_layout(None, count))
}

fn linear_layout(sides: Option<RunSides<Length>>, count: u32) -> LinearLayout {
    LinearLayout {
        first: LinearRun {
            direction: Construction {
                resolved: Some(UnitVector3::X_AXIS),
                ..empty_construction()
            },
            run: PatternRun {
                spacing: Spacing::Pitch {
                    count: Dimension::Literal(count),
                    pitch: Dimension::Literal(length(1.0)),
                },
                sides,
                seed: None,
                increments: Vec::new(),
            },
        },
        second: None,
        seed_only: None,
    }
}

fn pattern(layout: PatternLayout, overrides: Vec<InstanceOverride>) -> FeatureOperation {
    pattern_of("fillet", layout, overrides)
}

fn pattern_of(
    seed: &str,
    layout: PatternLayout,
    overrides: Vec<InstanceOverride>,
) -> FeatureOperation {
    FeatureOperation::Pattern(Box::new(Pattern {
        seeds: PatternSeeds::Features {
            features: named(vec![pick(PickTarget::FeatureResult(feature(seed)))]),
            method: None,
        },
        layout,
        instances: PatternInstances::Implicit(overrides),
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }))
}

fn skipped(index: Vec<i32>) -> InstanceOverride {
    InstanceOverride {
        index: InstanceIndex(index),
        change: InstanceChange::Skipped,
    }
}

#[test]
fn pattern_members_belong_to_its_container_and_to_one_generator() {
    use crate::design::container::UnorderedKind;
    use crate::design::ops::body::PatternMember;
    let members_of = |seed: &str| {
        let FeatureOperation::Pattern(mut pattern) = pattern_of(seed, linear(None, 3), Vec::new())
        else {
            unreachable!()
        };
        pattern.instances = PatternInstances::Members(vec![PatternMember {
            index: InstanceIndex(vec![1]),
            features: vec![feature("copy")],
        }]);
        FeatureOperation::Pattern(pattern)
    };
    let mut ir = fixture::document();
    ir.design.containers[0] = Container::Unordered {
        id: id("container", "timeline"),
        kind: UnorderedKind::Group,
        members: [
            Member::Feature(feature("extrude")),
            Member::Feature(feature("fillet")),
        ]
        .into_iter()
        .collect(),
    };
    ir.design.features[0].definition.operation = members_of("fillet");
    ir.design
        .features
        .push(fixture::unresolved("copy", OperationFamily::Fillet));
    assert_finding(
        &ir,
        "pattern member test:design:feature#copy is no member of the pattern's container",
    );
    let Container::Unordered { members, .. } = &mut ir.design.containers[0] else {
        unreachable!()
    };
    members.insert(Member::Feature(feature("copy")));
    assert!(!design_findings(&ir).iter().any(|f| f
        .message
        .contains("no member of the pattern's container")
        || f.message.contains("several generators")));
    ir.design.features[1].definition.operation = members_of("extrude");
    assert_finding(&ir, "feature is a member of several generators");
}

#[test]
fn a_pattern_layout_and_its_indices_fit_its_runs() {
    let mut ir = fixture::document();
    ir.design.features[1].definition.operation =
        pattern_of("extrude", linear(None, 3), vec![skipped(vec![1])]);
    assert_no_design_finding(&ir);
    assert_finding(
        &with(pattern(linear(None, 0), Vec::new())),
        "a count is at least 1",
    );
    assert_finding(
        &with(pattern(linear(Some(RunSides::Against), 3), Vec::new())),
        "RunSides::Against occurs in a linear run",
    );
    assert_finding(
        &with(pattern(linear(None, 3), vec![skipped(vec![0])])),
        "is all zeros",
    );
    assert_finding(
        &with(pattern(linear(None, 3), vec![skipped(vec![1, 1])])),
        "other than one coordinate per run",
    );
    assert_finding(
        &with(pattern(
            PatternLayout::Composite(vec![linear_stage(2)]),
            Vec::new(),
        )),
        "fewer than two stages",
    );
    let follow = PatternLayout::Follow(named(vec![pick(PickTarget::Feature(feature("extrude")))]));
    assert_finding(
        &with(pattern(follow, Vec::new())),
        "reaches the pattern itself",
    );
}

fn combine(
    target: Option<Selection<crate::design::selection::Unordered>>,
    op: BooleanOp,
) -> Combine {
    Combine {
        target,
        tools: named(vec![entity(NameKind::Body, 2)]),
        op: Some(op),
        tools_after: None,
        new_component: None,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }
}

#[test]
fn a_combine_has_tools_apart_from_its_target() {
    let target = || Some(named(vec![entity(NameKind::Body, 1)]));
    let op = |combine| FeatureOperation::Combine(Box::new(combine));
    assert_no_design_finding(&with(op(combine(target(), BooleanOp::Cut))));
    assert_finding(
        &with(op(combine(None, BooleanOp::Cut))),
        "Combine.target is None",
    );
    assert_finding(
        &with(op(combine(target(), BooleanOp::Sew))),
        "Combine.op is Sew",
    );
    assert_finding(
        &with(op(combine(target(), BooleanOp::Compound))),
        "a Compound combine states a target",
    );
    let shared = Some(named(vec![entity(NameKind::Body, 2)]));
    assert_finding(
        &with(op(combine(shared, BooleanOp::Cut))),
        "in both Combine.target and Combine.tools",
    );
}

#[test]
fn a_transformation_has_steps_and_counts_of_one_or_more() {
    let transform = |steps, mode| {
        FeatureOperation::TransformBodies(Box::new(TransformBodies {
            bodies: named(vec![entity(NameKind::Body, 1)]),
            transformation: Transformation {
                steps,
                resolved: None,
            },
            mode,
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }))
    };
    assert_finding(
        &with(transform(Vec::new(), None)),
        "Transformation.steps is empty",
    );
    let step = TransformStep::Fixed(Transform::identity());
    let copy = TransformMode::Copy {
        count: Dimension::Literal(0),
        divisions: None,
    };
    assert_finding(&with(transform(vec![step], Some(copy))), "it is at least 1");
}

#[test]
fn a_template_input_pick_names_an_input_of_its_instance() {
    let instance = TemplateInstance {
        template: crate::design::ops::body::Template {
            name: None,
            library: None,
            variant: None,
        },
        inputs: vec![TemplateInput {
            role: None,
            reference: named(vec![entity(NameKind::Face, 1)]),
        }],
        parameters: Vec::new(),
        members: vec![feature("fillet")],
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    };
    let mut ir = with(FeatureOperation::TemplateInstance(Box::new(instance)));
    let input = |input| {
        let mut picking = extrude();
        picking.profile = named(vec![pick(PickTarget::TemplateInput {
            instance: feature("extrude"),
            input,
        })]);
        FeatureOperation::Extrude(Box::new(picking))
    };
    ir.design.features[1].definition.operation = input(0);
    assert!(!design_findings(&ir)
        .iter()
        .any(|f| f.message.contains("TemplateInput")));
    ir.design.features[1].definition.operation = input(1);
    assert_finding(&ir, "TemplateInput picks input 1");

    let plane = |input| {
        SketchPlane::Placed(Box::new(Construction {
            inputs: vec![crate::design::construction::ConstructionInput {
                role: None,
                reference: Some(crate::design::construction::InputReference::Picks(named(
                    vec![pick(PickTarget::TemplateInput {
                        instance: feature("extrude"),
                        input,
                    })],
                ))),
                value: None,
                reversed: None,
            }],
            after: Vec::new(),
            solution: None,
            mode: None,
            resolved: None,
            resolved_in: BTreeMap::new(),
        }))
    };
    ir.design.features[1].definition.operation = input(0);
    ir.design.sketches.push(Sketch {
        owner: ir.design.features[1].id.clone(),
        plane: plane(0),
        ..profile_sketch()
    });
    assert!(!design_findings(&ir)
        .iter()
        .any(|f| f.message.contains("TemplateInput")));
    ir.design.sketches[0].plane = plane(1);
    assert_finding(&ir, "TemplateInput picks input 1");
}

// ----------------------------------------------------------------- surfacing, sheet metal, meshes

#[test]
fn a_knit_joins_two_elements_and_heals_with_a_tolerance() {
    let knit = |elements, healing| {
        FeatureOperation::Knit(Box::new(Knit {
            elements,
            tolerance: None,
            simplify: None,
            healing,
            federation: None,
            output: SheetOutput {
                solid: None,
                boolean: None,
            },
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }))
    };
    assert_finding(
        &with(knit(named(vec![entity(NameKind::Face, 1)]), None)),
        "fewer than two picks",
    );
    let healing = crate::design::ops::surface::Healing {
        objective: None,
        tangency: None,
        tangency_objective: None,
        frozen: None,
    };
    assert_finding(&with(knit(unread(), Some(healing))), "without a tolerance");
}

#[test]
fn a_mesh_combine_and_section_name_what_they_use() {
    let mesh = |kind| {
        FeatureOperation::Mesh(Box::new(MeshFeature {
            source: named(vec![entity(NameKind::Body, 1)]),
            kind,
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }))
    };
    assert_finding(
        &with(mesh(MeshKind::Combine {
            tools: named(Vec::new()),
            op: Some(BooleanOp::Assemble),
        })),
        "other than Join, Cut, or Intersect",
    );
    assert_finding(
        &with(mesh(MeshKind::Section {
            sketch: sketch_id("absent"),
        })),
        "which is no sketch record",
    );
}

fn bend() -> BendSpec {
    BendSpec {
        radius: None,
        radius_side: None,
        relief: None,
    }
}

fn sheet(kind: SheetMetalKind) -> FeatureOperation {
    FeatureOperation::SheetMetal(Box::new(SheetMetal {
        kind,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }))
}

fn rule(key: &str, ratios: &[f64]) -> SheetMetalRule {
    SheetMetalRule {
        id: id("sheet-metal-rule", key),
        scope: id("definition", "part"),
        name: None,
        thickness: None,
        bend: bend(),
        allowance: Some(BendAllowance::KFactorTable(KFactorTable {
            rows: ratios
                .iter()
                .map(|&ratio| KFactorRow {
                    ratio: crate::scalar::NonNegativeReal::new(ratio).unwrap(),
                    k_factor: Dimension::Literal(crate::scalar::FiniteReal::new(0.4).unwrap()),
                })
                .collect(),
            lookup: None,
        })),
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
        native_ref: None,
    }
}

#[test]
fn sheet_metal_rules_are_unique_ordered_and_in_scope() {
    let apply = |key| {
        sheet(SheetMetalKind::ApplyRule {
            rule: id("sheet-metal-rule", key),
            bodies: None,
        })
    };
    let mut ir = with(apply("gauge"));
    ir.design.sheet_metal_rules.push(rule("gauge", &[0.5, 1.0]));
    assert_no_design_finding(&ir);
    ir.design.sheet_metal_rules.push(rule("gauge", &[1.0, 0.5]));
    assert_finding(&ir, "sheet-metal rule id repeats");
    assert_finding(&ir, "do not increase strictly");
    let ir = with(apply("absent"));
    assert_finding(&ir, "is no rule of the feature's scope");
}

#[test]
fn a_flange_states_one_width_per_edge() {
    let flange = Flange {
        edges: named(vec![entity(NameKind::Edge, 1), entity(NameKind::Edge, 2)]),
        height: None,
        angle: None,
        width: Some(FlangeWidth::CenteredPerEdge(vec![Dimension::Literal(
            positive(1.0),
        )])),
        bend: bend(),
        rule: None,
        height_datum: None,
        position: None,
        outline: None,
    };
    assert_finding(
        &with(sheet(SheetMetalKind::Flange(flange))),
        "has 1 widths for 2 edges",
    );
}

// ----------------------------------------------------------------- assembly operations

/// Adds occurrences of the part, owned by the part.
fn own_occurrences(ir: &mut CadIr, keys: &[&str]) {
    for key in keys {
        let occurrence: OccurrenceId = id("occurrence", key);
        ir.model.occurrences.push(Occurrence::new(
            occurrence.clone(),
            id("definition", "part"),
            None,
        ));
        ir.model
            .product_definitions
            .iter_mut()
            .find(|definition| definition.id.as_str() == "test:design:definition#part")
            .unwrap()
            .occurrences
            .push(occurrence);
    }
}

#[test]
fn a_place_set_lists_two_distinct_occurrences() {
    let set = |keys: &[&str]| {
        FeatureOperation::PlaceSet(Box::new(PlaceSet {
            occurrences: keys.iter().map(|key| id("occurrence", key)).collect(),
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }))
    };
    let mut ir = with(set(&["a", "a"]));
    own_occurrences(&mut ir, &["a"]);
    assert_finding(&ir, "lists an occurrence twice, or fewer than 2");
    assert_finding(
        &with(set(&["absent", "b"])),
        "which is no occurrence of the model",
    );
}

#[test]
fn a_place_states_its_base_with_its_placement() {
    let place = Place {
        occurrence: id("occurrence", "a"),
        placement: Some(empty_construction()),
        scale: None,
        base: None,
        constraints: Vec::new(),
        packaged: None,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    };
    let mut ir = with(FeatureOperation::Place(Box::new(place)));
    own_occurrences(&mut ir, &["a"]);
    assert_finding(&ir, "Place.base is present where placement is not");
}

/// §10: a `Place.scale` occurs only with a `placement`, and a `PerAxis` one
/// states no `frame`.
#[test]
fn a_place_scale_occurs_with_a_placement_and_in_the_placed_axes() {
    let factors =
        [2.0, 1.0, 1.0].map(|factor| Dimension::Literal(NonZeroReal::new(factor).unwrap()));
    let place = |placement: bool, frame: bool| {
        let placement = placement.then(empty_construction);
        let mut ir = with(FeatureOperation::Place(Box::new(Place {
            occurrence: id("occurrence", "a"),
            base: placement.as_ref().map(|_| PlacementBase::Parent),
            placement,
            scale: Some(ScaleFactors::PerAxis {
                factors: factors.clone(),
                frame: frame.then(|| Box::new(empty_construction())),
            }),
            constraints: Vec::new(),
            packaged: None,
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        })));
        own_occurrences(&mut ir, &["a"]);
        ir
    };
    assert_no_finding_containing(&place(true, false), "Place.scale");
    assert_finding(
        &place(false, false),
        "Place.scale is present where placement is not",
    );
    assert_finding(&place(true, true), "a PerAxis Place.scale states a frame");
}

fn joint(kind: JointKind) -> FeatureOperation {
    let frame = || JointConnector::Frame(empty_construction());
    FeatureOperation::Joint(Box::new(Joint {
        connectors: [frame(), frame()],
        kind,
        alignment: None,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }))
}

fn motion(motion: JointMotion, limits: Vec<AxisLimits>) -> JointKind {
    JointKind::Motion {
        motion,
        axes: None,
        offset: JointOffset {
            x: None,
            y: None,
            z: None,
            angle: None,
        },
        limits,
        drives: Vec::new(),
    }
}

#[test]
fn a_motion_joint_limits_the_freedoms_it_leaves() {
    let pin_slot = JointMotion::PinSlot {
        axis: FrameAxis::Z,
        slide: FrameAxis::Z,
    };
    assert_finding(
        &with(joint(motion(pin_slot, Vec::new()))),
        "a PinSlot's slide is its axis",
    );
    let slide = |axis| AxisLimits::Translation {
        axis,
        limits: Some(Bounds {
            min: Some(Dimension::Literal(length(2.0))),
            max: Some(Dimension::Literal(length(1.0))),
        }),
        rest: None,
    };
    let revolute = JointMotion::Revolute { axis: FrameAxis::Z };
    let ir = with(joint(motion(revolute, vec![slide(FrameAxis::Z)])));
    assert_finding(&ir, "a degree of freedom the motion does not leave");
    assert_finding(&ir, "Bounds has min above max");
    assert_finding(&ir, "no inputs and no steps has no resolved value");
    assert_finding(
        &with(joint(JointKind::Slot {
            position: None,
            lock_rotation: None,
        })),
        "of a variant its kind does not take",
    );
}

#[test]
fn component_instances_hold_one_occurrence_per_seed() {
    let component = || {
        let mut component = pick(PickTarget::Component);
        component.context = Some(PickContext::Occurrence(
            crate::design::selection::OccurrencePath(Vec::new()),
        ));
        component
    };
    let pattern = ComponentPattern {
        seeds: named(vec![component()]),
        layout: linear(None, 3),
        instances: vec![ComponentInstance {
            index: InstanceIndex(vec![1]),
            occurrences: vec![id("occurrence", "a"), id("occurrence", "b")],
        }],
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    };
    let mut ir = with(FeatureOperation::ComponentPattern(Box::new(pattern)));
    own_occurrences(&mut ir, &["a", "b"]);
    assert_finding(&ir, "lists 2 occurrences for 1 seeds");
    let mirror = MirrorComponents {
        seeds: named(vec![entity(NameKind::Body, 1)]),
        plane: empty_construction(),
        results: Vec::new(),
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    };
    let ir = with(FeatureOperation::MirrorComponents(Box::new(mirror)));
    assert_finding(&ir, "it picks only Component targets");
    assert_finding(&ir, "other than one result per seed");
}

#[test]
fn a_new_component_is_created_once() {
    let mut creating = combine(
        Some(named(vec![entity(NameKind::Body, 1)])),
        BooleanOp::Join,
    );
    creating.new_component = Some(NewComponent {
        occurrence: id("occurrence", "a"),
    });
    let mut ir = with(FeatureOperation::Combine(Box::new(creating)));
    let mirror = MirrorComponents {
        seeds: named(Vec::new()),
        plane: empty_construction(),
        results: vec![MirrorResult {
            occurrence: id("occurrence", "a"),
            orientation: None,
        }],
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    };
    ir.design.features[1].definition.operation =
        FeatureOperation::MirrorComponents(Box::new(mirror));
    own_occurrences(&mut ir, &["a"]);
    assert_finding(&ir, "which this combine's new_component creates");
}

#[test]
fn a_component_and_a_driver_name_other_scopes() {
    let mut ir = extruding(|_| {});
    ir.design.features[0].component = Some(id("definition", "part"));
    assert_finding(&ir, "no definition its scope spans beside its own");
    let mut ir = extruding(|_| {});
    ir.design.features[0].driven_by = Some(Driver::Feature(feature("fillet")));
    assert_finding(&ir, "a feature of its own scope");
}

#[test]
fn a_component_names_a_definition_its_scope_spans() {
    let names = |key: &str| {
        let mut ir = extruding(|_| {});
        for (key, storage) in [
            ("member", Storage::Embedded),
            ("external", Storage::External(ExternalDocument::default())),
        ] {
            ir.model
                .product_definitions
                .push(ProductDefinition::new(id("definition", key), storage));
            let occurrence: OccurrenceId = id("occurrence", key);
            ir.model.occurrences.push(Occurrence::new(
                occurrence.clone(),
                id("definition", key),
                None,
            ));
            ir.model
                .product_definitions
                .iter_mut()
                .find(|definition| definition.id.as_str() == "test:design:definition#part")
                .unwrap()
                .occurrences
                .push(occurrence);
        }
        ir.design.features[0].component = Some(id("definition", key));
        design_findings(&ir).iter().any(|finding| {
            finding
                .message
                .contains("no definition its scope spans beside its own")
        })
    };
    assert!(!names("member"));
    assert!(!names("part"));
    assert!(names("external"));
}

// ----------------------------------------------------------------- sketches

fn point(x: f64, y: f64) -> FinitePoint2 {
    FinitePoint2::new(Point2::new(x, y)).unwrap()
}

fn sketch_entity(
    key: &str,
    curve: PlanarCurve,
    roles: Vec<EntityRole<Planar>>,
) -> SketchEntity<Planar> {
    SketchEntity {
        id: id::<SketchEntityId>("sketch-entity", key),
        sketch: sketch_id("profile"),
        definition: EntityDefinition::Free(curve),
        roles,
        construction: None,
        centerline: None,
        native: None,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
        native_ref: None,
    }
}

fn role(of: &str, role: DefiningRole) -> EntityRole<Planar> {
    EntityRole {
        of: id("sketch-entity", of),
        role,
    }
}

fn constraint(key: &str, relation: SketchRelation<Planar>) -> SketchConstraint<Planar> {
    SketchConstraint {
        id: id::<SketchConstraintId>("sketch-constraint", key),
        sketch: sketch_id("profile"),
        relation,
        name: None,
        active: None,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
        native_ref: None,
    }
}

fn operand(key: &str) -> Operand<Planar> {
    Operand::Entity(id("sketch-entity", key))
}

/// The fixture with its extrude a sketch feature that owns a sketch of a
/// line and its two end points.
fn sketched() -> CadIr {
    let mut ir = with(FeatureOperation::Sketch(sketch_id("profile")));
    ir.design.sketches.push(profile_sketch());
    let line = PlanarCurve::Line {
        start: point(0.0, 0.0),
        end: point(1.0, 0.0),
    };
    ir.design.sketch_entities.extend([
        sketch_entity("line", line, Vec::new()),
        sketch_entity(
            "start",
            PlanarCurve::Point(point(0.0, 0.0)),
            vec![role("line", DefiningRole::Start)],
        ),
        sketch_entity(
            "end",
            PlanarCurve::Point(point(1.0, 0.0)),
            vec![role("line", DefiningRole::End)],
        ),
    ]);
    ir.design.sketch_constraints.push(constraint(
        "horizontal",
        SketchRelation::Horizontal {
            a: operand("line"),
            b: None,
        },
    ));
    ir
}

fn sketch_findings(ir: &CadIr) -> Vec<String> {
    design_findings(ir)
        .into_iter()
        .filter(|f| f.entity.as_deref().is_some_and(|e| e.contains("sketch")))
        .map(|f| f.message)
        .collect()
}

#[test]
fn a_sketch_of_a_line_and_its_ends_breaks_no_sketch_rule() {
    let findings = sketch_findings(&sketched());
    assert!(findings.is_empty(), "{findings:#?}");
}

#[test]
fn a_sketch_is_owned_by_the_feature_that_holds_it() {
    let mut ir = sketched();
    ir.design.sketches[0].owner = feature("absent");
    assert_finding(&ir, "is no feature of the design");
    let mut ir = sketched();
    ir.design.sketches[0].owner = feature("fillet");
    assert_finding(&ir, "neither holds it as its operation nor names it");
    assert_finding(&ir, "which is no sketch it owns");
}

#[test]
fn roles_fit_their_holders_and_targets() {
    let mut ir = sketched();
    ir.design.sketch_entities[1].roles = vec![role("end", DefiningRole::Center)];
    assert_finding(&ir, "which is no circle, arc, ellipse, or hyperbola");
    let mut ir = sketched();
    ir.design.sketch_entities[0].roles = vec![role("end", DefiningRole::Start)];
    assert_finding(&ir, "holds the point role");
    let mut ir = sketched();
    ir.design.sketch_entities[2].roles = vec![role("line", DefiningRole::Start)];
    assert_finding(&ir, "entities hold one role");
}

#[test]
fn relations_take_operands_of_their_sketch_and_kind() {
    let mut ir = sketched();
    ir.design.sketch_constraints.push(constraint(
        "equal",
        SketchRelation::Equal {
            members: vec![operand("line"), operand("start")],
        },
    ));
    assert_finding(&ir, "Equal takes at least two members of one kind class");
    let mut ir = sketched();
    ir.design.sketch_constraints.push(constraint(
        "vertical",
        SketchRelation::Vertical {
            a: operand("start"),
            b: Some(operand("absent")),
        },
    ));
    assert_finding(&ir, "which is no entity of its sketch");
    let mut ir = sketched();
    ir.design.sketch_constraints[0].sketch = sketch_id("absent");
    assert_finding(&ir, "which is no planar sketch");
}

#[test]
fn a_bound_sketch_dimension_is_named_by_its_parameter_of_the_owner_scope() {
    let mut ir = sketched();
    let parameter: ParameterId = id("parameter", "width");
    let mut record: DesignParameter = fixture::parameter(
        "width",
        ValueKind::Quantity(crate::design::units::Quantity::LENGTH),
    );
    record.scope = id("definition", "other");
    ir.design.parameters.push(record);
    let mut distance = constraint(
        "distance",
        SketchRelation::Distance {
            a: operand("start"),
            b: operand("end"),
            along: None,
            doubled: None,
            value: Dimension::Parameter(parameter),
        },
    );
    distance.name = Some("width".to_owned());
    ir.design.sketch_constraints.push(distance);
    assert_finding(&ir, "binds a parameter states a name");
    assert_finding(&ir, "a parameter of another scope than the sketch owner's");
}

#[test]
fn a_profile_region_has_an_outer_boundary_of_curves() {
    let mut ir = sketched();
    let region = crate::design::selection::SketchRegion {
        outer: vec![crate::design::selection::BoundaryUse {
            entity: id("sketch-entity", "start"),
            fragment: None,
        }],
        holes: Vec::new(),
    };
    let mut picking = extrude();
    picking.profile = named(vec![pick(PickTarget::SketchRegion {
        sketch: sketch_id("profile"),
        region,
    })]);
    ir.design.features[1].definition.operation = FeatureOperation::Extrude(Box::new(picking));
    assert_finding(&ir, "which is no non-construction curve of the sketch");
}

// ----------------------------------------------------------------- stated content only

#[track_caller]
fn assert_no_finding_containing(ir: &CadIr, fragment: &str) {
    let findings = design_findings(ir);
    assert!(
        !findings.iter().any(|f| f.message.contains(fragment)),
        "a design finding containing {fragment:?} in {findings:#?}"
    );
}

fn sweep(extent: PathExtent) -> FeatureOperation {
    FeatureOperation::Sweep(Box::new(Sweep {
        section: SweepSection::Profile(unread()),
        path: unread(),
        controls: SweepControls {
            orientation: None,
            twist: None,
            scaling: None,
        },
        guides: Vec::new(),
        extent: Some(extent),
        placement: None,
        limits: SpineLimits {
            start: None,
            end: None,
        },
        transition: None,
        output: solid(),
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }))
}

#[test]
fn an_unstated_path_extent_fraction_is_not_zero() {
    let unstated = PathExtent {
        along: None,
        against: None,
    };
    assert_no_finding_containing(&with(sweep(unstated)), "PathExtent");
    let zero = || Some(Dimension::Literal(Fraction::new(0.0).unwrap()));
    let literal = PathExtent {
        along: zero(),
        against: zero(),
    };
    assert_finding(&with(sweep(literal)), "along + against not above zero");
}

#[test]
fn a_connector_placed_by_a_step_has_no_resolved_value_to_state() {
    let stepped = || {
        JointConnector::Frame(Construction {
            after: vec![PostStep::Translate {
                axis: FrameAxis::Z,
                distance: Dimension::Literal(length(1.0)),
            }],
            ..empty_construction()
        })
    };
    let mut operation = joint(JointKind::Coincident { offset: None });
    if let FeatureOperation::Joint(joint) = &mut operation {
        joint.connectors = [stepped(), stepped()];
    }
    assert_no_finding_containing(&with(operation), "no resolved value");
    assert_finding(
        &with(joint(JointKind::Coincident { offset: None })),
        "no inputs and no steps has no resolved value",
    );
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
fn a_symmetric_contour_flange_extent_is_admitted() {
    let flange = |extent| {
        sheet(SheetMetalKind::ContourFlange {
            profile: unread(),
            edges: None,
            width: None,
            extent: Some(extent),
            thickness: None,
            bend: bend(),
            vertex_bends: Vec::new(),
            rule: None,
        })
    };
    let symmetric = Span::Symmetric {
        amount: Dimension::Literal(length(5.0)),
        measure: None,
    };
    assert_no_finding_containing(&with(flange(symmetric)), "ContourFlange.extent");
    let negative = Span::Between {
        start: Box::new(Limit::AtOrigin),
        end: Box::new(Limit::Amount(Dimension::Literal(length(-1.0)))),
    };
    assert_finding(&with(flange(negative)), "a ContourFlange.extent end");
}

#[test]
fn a_contour_flange_extent_follows_the_span_rules() {
    let flange = |extent| {
        sheet(SheetMetalKind::ContourFlange {
            profile: unread(),
            edges: None,
            width: None,
            extent: Some(extent),
            thickness: None,
            bend: bend(),
            vertex_bends: Vec::new(),
            rule: None,
        })
    };
    let zero = Span::Symmetric {
        amount: Dimension::Literal(length(0.0)),
        measure: None,
    };
    assert_finding(
        &with(flange(zero)),
        "ContourFlange.extent is a symmetric span of zero",
    );
    let origins = Span::Between {
        start: Box::new(Limit::AtOrigin),
        end: Box::new(Limit::AtOrigin),
    };
    assert_finding(
        &with(flange(origins)),
        "ContourFlange.extent is a Span::Between with AtOrigin at both start and end",
    );
}

/// The sketch fixture with a circle beside its line and points.
fn sketched_with_circle() -> CadIr {
    let mut ir = sketched();
    ir.design.sketch_entities.push(sketch_entity(
        "circle",
        PlanarCurve::Circle {
            center: point(0.0, 0.0),
            radius: positive(1.0),
        },
        Vec::new(),
    ));
    ir
}

fn sketch_entity_pick(key: &str) -> Picked {
    pick(PickTarget::SketchEntity(id("sketch-entity", key)))
}

#[test]
fn a_louver_open_edge_is_a_curve_of_its_profile_sketch() {
    use crate::design::ops::sheet_metal::StampForm;
    let louver = |edge: &str| {
        sheet(SheetMetalKind::Stamp {
            entry: None,
            form: StampForm::Louver {
                profile: named(vec![sketch_pick()]),
                open_edge: named(vec![sketch_entity_pick(edge)]),
                depth: Dimension::Literal(positive(1.0)),
                flap_angle: Dimension::Literal(crate::scalar::Angle::new(0.5).unwrap()),
                wall_angle: Dimension::Literal(crate::scalar::Angle::new(0.5).unwrap()),
                rim_radius: None,
            },
            rule: None,
        })
    };
    let open_edge = "Louver.open_edge picks";
    let mut ir = sketched();
    ir.design.sketches.push(Sketch {
        id: sketch_id("other"),
        ..profile_sketch()
    });
    ir.design.sketch_entities.push(SketchEntity {
        sketch: sketch_id("other"),
        ..sketch_entity(
            "elsewhere",
            PlanarCurve::Line {
                start: point(0.0, 1.0),
                end: point(1.0, 1.0),
            },
            Vec::new(),
        )
    });
    ir.design.features[1].definition.operation = louver("line");
    assert!(!design_findings(&ir)
        .iter()
        .any(|f| f.message.contains(open_edge)));
    ir.design.features[1].definition.operation = louver("start");
    assert_finding(&ir, open_edge);
    ir.design.features[1].definition.operation = louver("elsewhere");
    assert_finding(&ir, open_edge);
}

#[test]
fn a_helix_start_circle_picks_a_circle_or_arc() {
    let helical = |key| {
        FeatureOperation::HelicalSweep(Box::new(HelicalSweep {
            section: HelicalSection::Profile(unread()),
            helix: Helix {
                placement: empty_construction(),
                start: Some(HelixStart::Circle(named(vec![sketch_entity_pick(key)]))),
                shape: HelixShape::Helix {
                    law: None,
                    taper: None,
                },
                hand: None,
            },
            output: solid(),
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }))
    };
    let mut ir = sketched_with_circle();
    ir.design.features[1].definition.operation = helical("line");
    assert_finding(&ir, "whose curve is no circle or arc");
    ir.design.features[1].definition.operation = helical("circle");
    assert_no_finding_containing(&ir, "HelixStart::Circle");
}

#[test]
fn hole_points_pick_points_or_sketches() {
    let at = |picked| {
        let mut operation = hole(None, false);
        if let FeatureOperation::Hole(hole) = &mut operation {
            hole.thread = None;
            hole.positions = HolePositions::Points {
                points: named(vec![picked]),
                centers: None,
            };
        }
        operation
    };
    let mut ir = sketched_with_circle();
    ir.design.features[1].definition.operation = at(sketch_entity_pick("line"));
    assert_finding(&ir, "that is no point");
    ir.design.features[1].definition.operation = at(sketch_entity_pick("start"));
    assert_no_finding_containing(&ir, "HolePositions::Points");
    let mut plane = fixture::unresolved("plane", OperationFamily::Datum);
    plane.definition.operation = FeatureOperation::Datum(Box::new(Datum {
        element: DatumElement::Plane(empty_construction()),
        link: None,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }));
    ir.design.features.push(plane);
    ir.design.features[1].definition.operation = at(pick(PickTarget::Datum {
        feature: feature("plane"),
        part: None,
    }));
    assert_finding(&ir, "that is no point");
}

#[test]
fn an_index_has_the_followed_and_composite_coordinates() {
    let follow = PatternLayout::Follow(named(vec![pick(PickTarget::Feature(feature("fillet")))]));
    let mut ir = with(pattern(follow, vec![skipped(vec![1, 1, 1])]));
    ir.design.features[1].definition.operation = pattern(linear(None, 3), Vec::new());
    assert_finding(&ir, "other than one coordinate per run");
    let composite = || PatternLayout::Composite(vec![linear_stage(2), linear_stage(2)]);
    assert_finding(
        &with(pattern(composite(), vec![skipped(vec![1])])),
        "other than one coordinate per run",
    );
    assert_no_finding_containing(
        &with(pattern(composite(), vec![skipped(vec![1, 1])])),
        "one coordinate per run",
    );
}

mod assembly_rules;
mod bounds;
mod forms;
mod pattern_coordinates;
mod run_ranges;
mod split_tools;

#[test]
fn an_assembly_feature_operation_states_no_body_operand() {
    let cutting = |targets| {
        let mut operation = extrude();
        operation.output.boolean = Some(Boolean::Apply {
            op: BooleanOp::Cut,
            targets,
        });
        FeatureOperation::Extrude(Box::new(operation))
    };
    let bodies = Targets::Selected(named(vec![entity(NameKind::Body, 1)]));
    assert_finding(
        &in_assembly(cutting(Some(bodies))),
        "a body operand of its own",
    );
    assert_no_finding_containing(&in_assembly(cutting(None)), "body operand");
}

/// The fixture with `operation` in a definition whose assembly context
/// holds `participants` as its one entry.
fn in_context(operation: FeatureOperation, participants: SetSelection) -> CadIr {
    let mut ir = with(operation);
    ir.design.features[0].definition.assembly = Some(AssemblyContext {
        participants: BTreeMap::from([(Some(ContextEffect::Instances), participants)]),
    });
    ir
}

fn in_assembly(operation: FeatureOperation) -> CadIr {
    in_context(operation, unread())
}

fn split_by_tools(targets: Option<SetSelection>) -> FeatureOperation {
    FeatureOperation::Split(Box::new(Split {
        mode: SplitMode::ByTools {
            targets,
            tools: SplitTool::Geometry {
                tools: named(vec![entity(NameKind::Face, 2)]),
                extend: None,
            },
            method: None,
            kept: None,
        },
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }))
}

#[test]
fn a_by_tools_split_states_targets_exactly_where_its_definition_states_no_assembly() {
    let targets = || Some(named(vec![entity(NameKind::Body, 1)]));
    let unstated = "ByTools.targets is None in a definition that states no assembly";
    let stated = "a body operand of its own, SplitMode::ByTools.targets";
    assert_no_finding_containing(&with(split_by_tools(targets())), "ByTools.targets");
    assert_finding(&with(split_by_tools(None)), unstated);
    assert_finding(&in_assembly(split_by_tools(targets())), stated);
    assert_no_finding_containing(&in_assembly(split_by_tools(None)), "ByTools.targets");
}

/// A component pick through the occurrence `key`.
fn participant(key: &str) -> Picked {
    let mut component = pick(PickTarget::Component);
    component.context = Some(PickContext::Occurrence(OccurrencePath(vec![PathStep {
        occurrence: StepRef::Occurrence(id("occurrence", key)),
        configuration: None,
    }])));
    component
}

fn affected(seeds: Vec<Picked>, propagation: Option<Propagation>) -> RuleTerm<Unordered> {
    RuleTerm {
        rule: SelectionRule::Affected,
        seeds: Members::new(seeds).unwrap(),
        propagation,
    }
}

fn rule_selection(
    terms: Vec<RuleTerm<Unordered>>,
    excluded: Vec<RuleTerm<Unordered>>,
    resolved: Option<Vec<Picked>>,
) -> SetSelection {
    Selection {
        at: Anchor::Input,
        picks: Picks::Rule {
            terms,
            excluded,
            resolved: resolved.map(|picks| Members::new(picks).unwrap()),
        },
        native: None,
    }
}

fn cut() -> FeatureOperation {
    FeatureOperation::Combine(Box::new(combine(None, BooleanOp::Cut)))
}

/// The fixture with a cut whose assembly context holds `entries`.
fn with_participants(entries: Vec<(Option<ContextEffect>, SetSelection)>) -> CadIr {
    let mut ir = with(cut());
    ir.design.features[0].definition.assembly = Some(AssemblyContext {
        participants: entries.into_iter().collect(),
    });
    ir
}

#[test]
fn every_assembly_participant_pick_carries_an_occurrence_context() {
    let message = "an assembly participant pick carries no occurrence context";
    assert_finding(
        &in_context(cut(), named(vec![pick(PickTarget::Component)])),
        message,
    );
    let cached = rule_selection(
        vec![affected(Vec::new(), None)],
        Vec::new(),
        Some(vec![pick(PickTarget::Component)]),
    );
    assert_finding(&in_context(cut(), cached), message);
    assert_no_finding_containing(&in_context(cut(), named(vec![participant("a")])), message);
}

#[test]
fn an_assembly_context_lists_each_participant_once_in_one_state() {
    let instances = || Some(ContextEffect::Instances);
    let definitions = || Some(ContextEffect::Definitions);
    let twice = "a pick is in two selections of an assembly context";
    assert_finding(
        &with_participants(vec![(instances(), named(Vec::new()))]),
        "is Named with no pick",
    );
    assert_finding(
        &with_participants(vec![
            (instances(), unread()),
            (definitions(), named(vec![participant("a")])),
        ]),
        "with Unread picks is not its context's only entry",
    );
    assert_no_finding_containing(
        &with_participants(vec![(instances(), unread())]),
        "only entry",
    );
    assert_finding(
        &with_participants(vec![
            (instances(), named(vec![participant("a")])),
            (
                definitions(),
                named(vec![participant("a"), participant("b")]),
            ),
        ]),
        twice,
    );
    let cached = rule_selection(
        vec![affected(Vec::new(), None)],
        Vec::new(),
        Some(vec![participant("a")]),
    );
    assert_finding(
        &with_participants(vec![
            (instances(), cached),
            (definitions(), named(vec![participant("a")])),
        ]),
        twice,
    );
    let mut current = named(vec![participant("b")]);
    current.at = Anchor::Current;
    assert_finding(
        &with_participants(vec![
            (instances(), named(vec![participant("a")])),
            (definitions(), current),
        ]),
        "state more than one at",
    );
    let distinct = with_participants(vec![
        (instances(), named(vec![participant("a")])),
        (definitions(), named(vec![participant("b")])),
    ]);
    for message in ["two selections", "more than one at", "occurrence context"] {
        assert_no_finding_containing(&distinct, message);
    }
}

#[test]
fn an_affected_term_is_a_seedless_unpropagated_term_of_one_participant_selection() {
    let instances = || Some(ContextEffect::Instances);
    let only = |term| rule_selection(vec![term], Vec::new(), None);
    let bare = with_participants(vec![(instances(), only(affected(Vec::new(), None)))]);
    assert_no_finding_containing(&bare, "Affected");
    assert_finding(
        &with_participants(vec![(
            instances(),
            only(affected(vec![participant("a")], None)),
        )]),
        "an Affected rule term has seeds",
    );
    assert_finding(
        &with_participants(vec![(
            instances(),
            only(affected(Vec::new(), Some(Propagation::Off))),
        )]),
        "an Affected rule term states a propagation",
    );
    assert_finding(
        &with_participants(vec![
            (instances(), only(affected(Vec::new(), None))),
            (
                Some(ContextEffect::Definitions),
                only(affected(Vec::new(), None)),
            ),
        ]),
        "more than one selection of an assembly context holds an Affected rule term",
    );
    let explicit = RuleTerm {
        rule: SelectionRule::Explicit,
        seeds: Members::new(vec![participant("a")]).unwrap(),
        propagation: None,
    };
    assert_finding(
        &with_participants(vec![(
            instances(),
            rule_selection(vec![explicit], vec![affected(Vec::new(), None)], None),
        )]),
        "among the excluded terms",
    );
    assert_finding(
        &with(split_by_tools(Some(only(affected(Vec::new(), None))))),
        "an Affected rule term is outside an assembly context's participants",
    );
}

#[test]
fn an_assembly_combine_states_no_target() {
    let op = |combine| FeatureOperation::Combine(Box::new(combine));
    assert_no_design_finding(&in_assembly(op(combine(None, BooleanOp::Cut))));
    let target = Some(named(vec![entity(NameKind::Body, 1)]));
    assert_finding(
        &in_assembly(op(combine(target, BooleanOp::Cut))),
        "a body operand of its own, Combine.target",
    );
}

#[test]
fn an_assembly_feature_makes_no_new_body() {
    let mut operation = extrude();
    operation.output.boolean = Some(Boolean::NewBody);
    assert_finding(
        &in_assembly(FeatureOperation::Extrude(Box::new(operation))),
        "has a Boolean that is NewBody",
    );
}

#[test]
fn an_assembly_feature_runs_no_operation_that_states_its_bodies() {
    let excluded = "which an assembly feature does not run";
    let place_set = FeatureOperation::PlaceSet(Box::new(PlaceSet {
        occurrences: Vec::new(),
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }));
    assert_finding(&in_assembly(place_set), excluded);
    let delete = FeatureOperation::DeleteBodies(Box::new(DeleteBodies {
        bodies: named(vec![entity(NameKind::Body, 1)]),
        retain: None,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }));
    assert_finding(&in_assembly(delete), excluded);
    let mutual = FeatureOperation::Split(Box::new(Split {
        mode: SplitMode::Mutual {
            elements: named(vec![entity(NameKind::Body, 1), entity(NameKind::Body, 2)]),
            kept: None,
        },
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }));
    assert_finding(&in_assembly(mutual), excluded);
    let knit = FeatureOperation::Unresolved {
        family: OperationFamily::Knit,
    };
    assert_finding(&in_assembly(knit), excluded);
    let split = FeatureOperation::Unresolved {
        family: OperationFamily::Split,
    };
    assert_no_finding_containing(&in_assembly(split), excluded);
}

/// §10: a `ByTools` split whose tool is `Silhouette` has `method: None`, and
/// its `kept`, where stated, is `All` or `Native`.
#[test]
fn a_silhouette_split_states_no_method_and_keeps_all_or_native() {
    let silhouette = |method, kept| {
        FeatureOperation::Split(Box::new(Split {
            mode: SplitMode::ByTools {
                targets: Some(named(vec![entity(NameKind::Body, 1)])),
                tools: SplitTool::Silhouette(Box::new(Construction {
                    resolved: Some(UnitVector3::Z_AXIS),
                    ..empty_construction()
                })),
                method,
                kept,
            },
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }))
    };
    let all = || {
        Some(Kept::All {
            form: Some(PieceForm::Faces),
        })
    };
    let native = Some(Kept::Native(
        NonBlankString::new("f3d:silhouette_split_solid_body").unwrap(),
    ));
    for kept in [all(), native, None] {
        assert_no_finding_containing(&with(silhouette(None, kept)), "Silhouette split");
    }
    assert_finding(
        &with(silhouette(Some(SplitMethod::Intersect), all())),
        "a Silhouette split states a method",
    );
    let side = Some(Kept::Side {
        side: Dimension::Literal(true),
    });
    assert_finding(
        &with(silhouette(None, side)),
        "a Silhouette split keeps a side or pieces",
    );
}

#[test]
fn a_tapped_series_stage_states_no_standard() {
    let mut operation = hole(None, true);
    if let FeatureOperation::Hole(hole) = &mut operation {
        let FeatureOperation::Hole(tapped) = self::hole(None, false) else {
            unreachable!();
        };
        hole.series[0].hole = StageHole::Stated {
            shape: Box::new(tapped.shape),
            extent: None,
            standard: Some(StandardSize {
                standard: NonBlankString::new("ISO").unwrap(),
                kind: None,
                size: None,
                class: None,
            }),
            thread: tapped.thread.map(Box::new),
        };
    }
    assert_finding(&with(operation), "a tapped series stage states");
}
