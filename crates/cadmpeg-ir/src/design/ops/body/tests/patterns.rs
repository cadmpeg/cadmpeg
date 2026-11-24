// SPDX-License-Identifier: Apache-2.0
//! Patterns, their layouts, runs, spacing, and instances.

use cadmpeg_core::decode::ResourceLimits;
use serde_json::json;
use std::collections::BTreeMap;

use super::{
    angle, axis_construction, direction_construction, faces, id, length, literal, options, ordered,
    parameter, plane_construction, point_construction, refuses, settings, text, to_value, unread,
    Report,
};
use crate::design::construction::{FrameAxis, ScalarDimension};
use crate::design::expr::{Expression, ExpressionDialect, ExpressionToken, Neutral, Node};
use crate::design::names::InstanceIndex;
use crate::design::ops::body::{
    AdjustAgainst, InstanceChange, InstanceOverride, Pattern, PatternInstances, PatternMember,
    PatternMethod, PatternSeeds,
};
use crate::design::ops::shared::{
    ChainLayout, ChainLink, ChainLinks, CircularLayout, CompositeStage, DimensionRun,
    DimensionTable, Increment, InstanceOrientation, LinearLayout, LinearRun, Locus, MirrorLayout,
    PathReference, PatternLayout, PatternRun, RunSides, ScaleLayout, Spacing, TableRow, UpToFill,
};
use crate::design::options::SymmetricMeasure;
use crate::design::read;
use crate::design::tests::arm_failures;
use crate::features::ParameterId;
use crate::scalar::{Angle, Length, PositiveReal};

fn increments() -> Vec<Increment> {
    vec![
        Increment {
            parameter: id("parameter", 4),
            step: ScalarDimension::Length(literal(length(2.0))),
        },
        Increment {
            parameter: id("parameter", 5),
            step: ScalarDimension::Angle(parameter(6)),
        },
    ]
}

fn length_fills() -> Vec<UpToFill<Length>> {
    vec![
        UpToFill::Count(literal(4)),
        UpToFill::Pitch(literal(length(2.5))),
    ]
}

fn angle_fills() -> Vec<UpToFill<Angle>> {
    vec![
        UpToFill::Count(parameter(1)),
        UpToFill::Pitch(literal(angle(0.5))),
    ]
}

fn loci() -> Vec<Locus> {
    vec![
        Locus::Centroid,
        Locus::Point(Box::new(point_construction())),
    ]
}

fn length_spacings() -> Vec<Spacing<Length>> {
    vec![
        Spacing::Pitch {
            count: literal(4),
            pitch: literal(length(2.5)),
        },
        Spacing::Span {
            count: parameter(1),
            span: parameter(2),
        },
        Spacing::PitchSpan {
            pitch: literal(length(2.0)),
            span: literal(length(10.0)),
        },
        Spacing::Fit { count: literal(6) },
        Spacing::UpTo {
            reference: Box::new(ordered(5)),
            fill: UpToFill::Pitch(literal(length(3.0))),
            offset: Some(literal(length(1.0))),
            from: Some(Locus::Centroid),
        },
        Spacing::UpTo {
            reference: Box::new(unread()),
            fill: UpToFill::Count(literal(3)),
            offset: None,
            from: None,
        },
        Spacing::List(vec![literal(length(1.0)), literal(length(2.0))]),
        Spacing::Native(text("f3d:path_spacing")),
    ]
}

fn angle_spacings() -> Vec<Spacing<Angle>> {
    vec![
        Spacing::Pitch {
            count: literal(6),
            pitch: literal(angle(1.0)),
        },
        Spacing::Span {
            count: literal(4),
            span: literal(angle(3.0)),
        },
        Spacing::Fit { count: literal(8) },
        Spacing::List(Vec::new()),
    ]
}

fn length_sides() -> Vec<RunSides<Length>> {
    vec![
        RunSides::One,
        RunSides::Against,
        RunSides::Symmetric {
            count: Some(SymmetricMeasure::Total),
            extent: Some(SymmetricMeasure::PerSide),
        },
        RunSides::Symmetric {
            count: None,
            extent: None,
        },
        RunSides::Reverse(Box::new(Spacing::Pitch {
            count: literal(2),
            pitch: literal(length(5.0)),
        })),
    ]
}

fn angle_sides() -> Vec<RunSides<Angle>> {
    vec![
        RunSides::One,
        RunSides::Symmetric {
            count: Some(SymmetricMeasure::PerSide),
            extent: None,
        },
        RunSides::Reverse(Box::new(Spacing::Fit { count: literal(3) })),
    ]
}

fn length_runs() -> Vec<PatternRun<Length>> {
    vec![
        PatternRun {
            spacing: Spacing::Pitch {
                count: literal(3),
                pitch: literal(length(4.0)),
            },
            sides: Some(RunSides::Against),
            seed: Some(literal(1)),
            increments: increments(),
        },
        PatternRun {
            spacing: Spacing::Fit { count: literal(5) },
            sides: None,
            seed: None,
            increments: Vec::new(),
        },
    ]
}

fn angle_runs() -> Vec<PatternRun<Angle>> {
    vec![
        PatternRun {
            spacing: Spacing::Span {
                count: literal(6),
                span: literal(angle(std::f64::consts::TAU)),
            },
            sides: Some(RunSides::Symmetric {
                count: Some(SymmetricMeasure::PerSide),
                extent: None,
            }),
            seed: Some(parameter(3)),
            increments: Vec::new(),
        },
        PatternRun {
            spacing: Spacing::Pitch {
                count: literal(2),
                pitch: literal(angle(0.5)),
            },
            sides: None,
            seed: None,
            increments: Vec::new(),
        },
    ]
}

fn linear_runs() -> Vec<LinearRun> {
    length_runs()
        .into_iter()
        .map(|run| LinearRun {
            direction: direction_construction(),
            run,
        })
        .collect()
}

fn path_references() -> Vec<PathReference> {
    vec![
        PathReference::PlaneNormal,
        PathReference::Direction(Box::new(direction_construction())),
    ]
}

fn orientations() -> Vec<InstanceOrientation> {
    vec![
        InstanceOrientation::Fixed,
        InstanceOrientation::Follow,
        InstanceOrientation::Frenet,
        InstanceOrientation::Rolled {
            reference: PathReference::PlaneNormal,
            reference_axis: FrameAxis::Y,
            seed_axis: None,
        },
        InstanceOrientation::Rolled {
            reference: PathReference::Direction(Box::new(direction_construction())),
            reference_axis: FrameAxis::Z,
            seed_axis: Some(Box::new(direction_construction())),
        },
        InstanceOrientation::Native(text("nx:pattern_orientation")),
    ]
}

fn dimension_runs() -> Vec<DimensionRun> {
    vec![
        DimensionRun {
            count: literal(4),
            increments: increments(),
        },
        DimensionRun {
            count: parameter(7),
            increments: Vec::new(),
        },
    ]
}

fn table_rows() -> Vec<TableRow> {
    vec![
        TableRow {
            instance: 1,
            values: vec![Some(ScalarDimension::Length(literal(length(12.0)))), None],
        },
        TableRow {
            instance: 2,
            values: Vec::new(),
        },
    ]
}

fn tables() -> Vec<DimensionTable> {
    vec![
        DimensionTable {
            parameters: vec![id("parameter", 4), id("parameter", 5)],
            rows: table_rows(),
        },
        DimensionTable {
            parameters: Vec::new(),
            rows: Vec::new(),
        },
    ]
}

fn chain_links() -> Vec<ChainLinks> {
    vec![
        ChainLinks::Connected(vec![ChainLink {
            pins: [axis_construction(), axis_construction()],
        }]),
        ChainLinks::Connected(Vec::new()),
        ChainLinks::Native {
            kind: text("sldprt:distance_chain"),
            picks: ordered(8),
        },
    ]
}

fn chain_layouts() -> Vec<ChainLayout> {
    chain_links()
        .into_iter()
        .map(|links| ChainLayout {
            path: ordered(9),
            links,
        })
        .collect()
}

fn linear_layouts() -> Vec<LinearLayout> {
    let mut linear = linear_runs();
    let second = linear.pop();
    let first = linear.pop().unwrap();
    vec![
        LinearLayout {
            first: first.clone(),
            second,
            seed_only: Some(literal(true)),
        },
        LinearLayout {
            first,
            second: None,
            seed_only: None,
        },
    ]
}

fn circular_layouts() -> Vec<CircularLayout> {
    vec![
        CircularLayout {
            axis: axis_construction(),
            angular: angle_runs().remove(0),
            radial: length_runs().pop(),
            orientation: Some(InstanceOrientation::Follow),
        },
        CircularLayout {
            axis: axis_construction(),
            angular: angle_runs().remove(1),
            radial: None,
            orientation: None,
        },
    ]
}

fn mirror_layouts() -> Vec<MirrorLayout> {
    vec![
        MirrorLayout {
            plane: plane_construction(),
            stitch_tolerance: Some(literal(length(0.01))),
        },
        MirrorLayout {
            plane: plane_construction(),
            stitch_tolerance: None,
        },
    ]
}

fn scale_layouts() -> Vec<ScaleLayout> {
    vec![
        ScaleLayout {
            factor: literal(PositiveReal::new(2.0).unwrap()),
            count: literal(2),
            center: Locus::Centroid,
        },
        ScaleLayout {
            factor: parameter(12),
            count: literal(3),
            center: Locus::Point(Box::new(point_construction())),
        },
    ]
}

fn stages() -> Vec<CompositeStage> {
    vec![
        CompositeStage::Linear(linear_layouts().remove(0)),
        CompositeStage::Circular(circular_layouts().remove(1)),
        CompositeStage::Mirror(mirror_layouts().remove(1)),
        CompositeStage::Scale(scale_layouts().remove(0)),
        CompositeStage::Native(text("fcstd:Transformed")),
    ]
}

fn layouts() -> Vec<PatternLayout> {
    let mut layouts: Vec<PatternLayout> = linear_layouts()
        .into_iter()
        .map(PatternLayout::Linear)
        .collect();
    layouts.extend(circular_layouts().into_iter().map(PatternLayout::Circular));
    layouts.extend([
        PatternLayout::Path {
            path: ordered(10),
            run: length_runs().remove(0),
            orientation: orientations().pop(),
        },
        PatternLayout::Path {
            path: unread(),
            run: length_runs().remove(1),
            orientation: None,
        },
        PatternLayout::Points {
            points: ordered(11),
            reference: Some(Locus::Point(Box::new(point_construction()))),
        },
        PatternLayout::Points {
            points: ordered(11),
            reference: None,
        },
        PatternLayout::Dimensions(dimension_runs()),
        PatternLayout::Table(tables().remove(0)),
    ]);
    layouts.extend(mirror_layouts().into_iter().map(PatternLayout::Mirror));
    layouts.push(PatternLayout::Composite(stages()));
    layouts.extend(scale_layouts().into_iter().map(PatternLayout::Scale));
    layouts.extend([
        PatternLayout::Follow(ordered(13)),
        PatternLayout::Chain(chain_layouts().remove(0)),
        PatternLayout::Native(text("nx:along_path")),
    ]);
    layouts
}

fn indices() -> Vec<InstanceIndex> {
    vec![
        InstanceIndex(vec![1, 0]),
        InstanceIndex(vec![-2]),
        InstanceIndex(Vec::new()),
    ]
}

fn expression_map() -> BTreeMap<ParameterId, Expression> {
    BTreeMap::from([
        (
            id("parameter", 1),
            Expression::Neutral(Neutral(Node::Boolean(true))),
        ),
        (
            id("parameter", 2),
            Expression::Source {
                dialect: ExpressionDialect::SolidWorks,
                tokens: vec![ExpressionToken::Text("12mm".to_owned())],
            },
        ),
    ])
}

fn changes() -> Vec<InstanceChange> {
    vec![
        InstanceChange::Skipped,
        InstanceChange::Varied {
            parameters: expression_map(),
        },
        InstanceChange::Varied {
            parameters: BTreeMap::new(),
        },
    ]
}

fn overrides() -> Vec<InstanceOverride> {
    changes()
        .into_iter()
        .zip(indices())
        .map(|(change, index)| InstanceOverride { index, change })
        .collect()
}

fn members() -> Vec<PatternMember> {
    vec![
        PatternMember {
            index: InstanceIndex(vec![1]),
            features: vec![id("feature", 20), id("feature", 21)],
        },
        PatternMember {
            index: InstanceIndex(vec![2]),
            features: Vec::new(),
        },
    ]
}

fn instances() -> Vec<PatternInstances> {
    vec![
        PatternInstances::Implicit(overrides()),
        PatternInstances::Implicit(Vec::new()),
        PatternInstances::Members(members()),
    ]
}

fn methods() -> Vec<PatternMethod> {
    vec![
        PatternMethod::Identical,
        PatternMethod::Adjust {
            against: Some(AdjustAgainst::ModelSoFar),
        },
        PatternMethod::Adjust { against: None },
        PatternMethod::Native(text("inventor:pattern_compute")),
    ]
}

fn seeds() -> Vec<PatternSeeds> {
    vec![
        PatternSeeds::Features {
            features: ordered(14),
            method: Some(PatternMethod::Adjust {
                against: Some(AdjustAgainst::ModelBefore),
            }),
        },
        PatternSeeds::Features {
            features: ordered(14),
            method: None,
        },
        PatternSeeds::Faces(faces(15)),
        PatternSeeds::Bodies {
            bodies: faces(16),
            join: Some(true),
        },
        PatternSeeds::Bodies {
            bodies: faces(16),
            join: None,
        },
    ]
}

fn patterns() -> Vec<Pattern> {
    vec![
        Pattern {
            seeds: seeds().remove(0),
            layout: layouts().remove(0),
            instances: instances().remove(0),
            native_options: options(),
            source_settings: settings(),
        },
        Pattern {
            seeds: seeds().remove(3),
            layout: PatternLayout::Mirror(mirror_layouts().remove(1)),
            instances: PatternInstances::Members(members()),
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        },
    ]
}

#[test]
fn every_pattern_type_round_trips() {
    let mut report = Report::default();
    report.check("Pattern", patterns());
    report.check("PatternSeeds", seeds());
    report.check("PatternMethod", methods());
    report.check(
        "AdjustAgainst",
        vec![AdjustAgainst::ModelSoFar, AdjustAgainst::ModelBefore],
    );
    report.check("PatternLayout", layouts());
    report.check("CompositeStage", stages());
    report.check("LinearLayout", linear_layouts());
    report.check("CircularLayout", circular_layouts());
    report.check("MirrorLayout", mirror_layouts());
    report.check("ScaleLayout", scale_layouts());
    report.check("ChainLayout", chain_layouts());
    report.check("ChainLinks", chain_links());
    report.check(
        "ChainLink",
        vec![ChainLink {
            pins: [axis_construction(), axis_construction()],
        }],
    );
    report.check("LinearRun", linear_runs());
    report.check("PatternRun<Length>", length_runs());
    report.check("PatternRun<Angle>", angle_runs());
    report.check("RunSides<Length>", length_sides());
    report.check("RunSides<Angle>", angle_sides());
    report.check("Spacing<Length>", length_spacings());
    report.check("Spacing<Angle>", angle_spacings());
    report.check("UpToFill<Length>", length_fills());
    report.check("UpToFill<Angle>", angle_fills());
    report.check("Locus", loci());
    report.check("InstanceOrientation", orientations());
    report.check("PathReference", path_references());
    report.check("Increment", increments());
    report.check("DimensionRun", dimension_runs());
    report.check("DimensionTable", tables());
    report.check("TableRow", table_rows());
    report.check("PatternInstances", instances());
    report.check("InstanceIndex", indices());
    report.check("InstanceOverride", overrides());
    report.check("InstanceChange", changes());
    report.check("PatternMember", members());
    report.finish();
}

/// §5.10: a pattern is the `FeatureOperation` arm `pattern`, holding the
/// pattern as its value.
#[test]
fn every_pattern_is_a_feature_operation_arm() {
    use crate::design::feature::{FeatureOperation as Op, OperationFamily as Family};
    let failures = arm_failures(Family::Pattern, patterns(), Op::Pattern);
    assert!(failures.is_empty(), "{failures:#?}");
}

/// §2.5: an adjacently tagged spacing writes its fields under `value`, a unit
/// arm of an enum with data is its tag alone, and an instance index is a bare
/// array.
#[test]
fn the_examples_of_section_2_5() {
    let pitch = Spacing::<Length>::Pitch {
        count: literal(4),
        pitch: literal(length(2.5)),
    };
    assert_eq!(
        to_value(&pitch),
        json!({"kind": "pitch", "value": {"count": {"kind": "literal", "value": 4}, "pitch": {"kind": "literal", "value": 2.5}}})
    );
    assert_eq!(
        to_value(&Spacing::<Length>::Native(text("nx:x")))["kind"],
        json!("native")
    );
    assert_eq!(
        to_value(&RunSides::<Length>::Against),
        json!({"kind": "against"})
    );
    assert_eq!(
        to_value(&PatternMethod::Identical),
        json!({"kind": "identical"})
    );
    assert_eq!(to_value(&AdjustAgainst::ModelSoFar), json!("model_so_far"));
    assert_eq!(to_value(&InstanceIndex(vec![1, -2])), json!([1, -2]));
    assert_eq!(
        to_value(&PatternSeeds::Faces(faces(1)))["kind"],
        json!("faces")
    );
}

/// A unit arm of an internally tagged enum writes only its tag and refuses
/// another key.
#[test]
fn an_empty_arm_is_its_tag_alone() {
    assert_eq!(to_value(&Locus::Centroid), json!({"kind": "centroid"}));
    assert_eq!(
        to_value(&PathReference::PlaneNormal),
        json!({"kind": "plane_normal"})
    );
    assert_eq!(
        to_value(&InstanceChange::Skipped),
        json!({"kind": "skipped"})
    );
    refuses::<PathReference>(r#"{"kind": "plane_normal", "extra": 1}"#);
    refuses::<InstanceChange>(r#"{"kind": "skipped", "extra": 1}"#);
}

/// §2.5: an `Option` field is omitted for `None` and refuses `null`; a list
/// field is always written and refuses omission; an `Option` in a list writes
/// `null`.
#[test]
fn options_are_omitted_and_lists_are_written() {
    let run = PatternRun::<Length> {
        spacing: Spacing::Fit { count: literal(2) },
        sides: None,
        seed: None,
        increments: Vec::new(),
    };
    assert_eq!(
        to_value(&run),
        json!({"spacing": {"kind": "fit", "value": {"count": {"kind": "literal", "value": 2}}}, "increments": []})
    );
    let fit = r#"{"kind": "fit", "value": {"count": {"kind": "literal", "value": 2}}}"#;
    let message = refuses::<PatternRun<Length>>(&format!(
        r#"{{"spacing": {fit}, "sides": null, "increments": []}}"#
    ));
    assert!(message.contains("sides"), "{message}");
    refuses::<PatternRun<Length>>(&format!(r#"{{"spacing": {fit}}}"#));
    refuses::<PatternRun<Length>>(&format!(
        r#"{{"spacing": {fit}, "increments": [], "count": 1}}"#
    ));
    let row = TableRow {
        instance: 3,
        values: vec![None],
    };
    assert_eq!(to_value(&row), json!({"instance": 3, "values": [null]}));
    let message = refuses::<PatternSeeds>(
        r#"{"kind": "bodies", "bodies": {"at": {"kind": "current"}, "picks": {"kind": "unread"}}, "join": null}"#,
    );
    assert!(message.contains("join"), "{message}");
}

/// A varied instance's parameters are an object keyed by parameter id that
/// refuses a repeated key.
#[test]
fn varied_parameters_are_an_object_with_distinct_keys() {
    let varied = InstanceChange::Varied {
        parameters: expression_map(),
    };
    let value = to_value(&varied);
    assert_eq!(value["kind"], json!("varied"));
    assert!(value["parameters"].is_object(), "{value}");
    assert!(value["parameters"]["test:design:parameter#1"].is_object());
    let message = refuses::<InstanceChange>(
        r#"{"kind": "varied", "parameters": {"test:design:parameter#1": {"kind": "neutral", "value": {"text": "true", "refs": [], "measures": []}}, "test:design:parameter#1": {"kind": "neutral", "value": {"text": "false", "refs": [], "measures": []}}}}"#,
    );
    assert!(message.contains("test:design:parameter#1"), "{message}");
}

/// A chain link holds exactly two pins.
#[test]
fn a_chain_link_holds_two_pins() {
    let link = ChainLink {
        pins: [axis_construction(), axis_construction()],
    };
    let mut value = to_value(&link);
    let pins = value["pins"].as_array_mut().unwrap();
    assert_eq!(pins.len(), 2);
    pins.pop();
    refuses::<ChainLink>(&value.to_string());
}

/// §6.5: a stage is a linear, circular, mirror, or scale layout or a native
/// stage, holding the struct of the layout of that name and written as that
/// layout's arm.
#[test]
fn a_composite_stage_is_written_as_the_layout_of_its_name() {
    let stage = |stage: &CompositeStage| to_value(stage);
    let layout = |layout: PatternLayout| to_value(&layout);
    let stages = stages();
    assert_eq!(
        stage(&stages[0]),
        layout(PatternLayout::Linear(linear_layouts().remove(0)))
    );
    assert_eq!(
        stage(&stages[1]),
        layout(PatternLayout::Circular(circular_layouts().remove(1)))
    );
    assert_eq!(
        stage(&stages[2]),
        layout(PatternLayout::Mirror(mirror_layouts().remove(1)))
    );
    assert_eq!(
        stage(&stages[3]),
        layout(PatternLayout::Scale(scale_layouts().remove(0)))
    );
    assert_eq!(
        stage(&stages[4]),
        layout(PatternLayout::Native(text("fcstd:Transformed")))
    );
}

/// §6.5: a `Composite` does not nest a `Composite`, and no stage is a path,
/// points, dimensions, table, follow, or chain layout.
#[test]
fn a_composite_stage_is_no_other_layout() {
    let native = r#"{"kind":"native","value":"leaf"}"#;
    let composite = |stage: &str| format!(r#"{{"kind":"composite","value":[{native},{stage}]}}"#);
    assert!(
        read::from_str::<PatternLayout>(&composite(native), &ResourceLimits::desktop()).is_ok()
    );
    let nested = composite(&composite(native));
    refuses::<PatternLayout>(&nested);
    for layout in layouts().into_iter().filter(|layout| {
        !matches!(
            layout,
            PatternLayout::Linear(_)
                | PatternLayout::Circular(_)
                | PatternLayout::Mirror(_)
                | PatternLayout::Scale(_)
                | PatternLayout::Native(_)
        )
    }) {
        refuses::<PatternLayout>(&composite(&to_value(&layout).to_string()));
    }
}
