// SPDX-License-Identifier: Apache-2.0
//! Revolves typed from the decoder's revolutions: the profile sketch, the
//! axis picked as a line of that sketch, the angle, and the boolean.

use cadmpeg_core::serde_erase::json;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::construction::{AxisValue, InputReference, InputRole, Relation, Subject};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ops::shared::{Limit, Span};
use cadmpeg_ir::design::ops::swept::Revolve;
use cadmpeg_ir::design::options::{BodyForm, Boolean, BooleanOp, SymmetricMeasure};
use cadmpeg_ir::design::selection::{PickTarget, Picks};
use cadmpeg_ir::design::units::{Quantity, UnitSymbol};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::Severity;
use serde_json::{json, Value};

use super::{assert_no_design_findings, part_with_history, timeline_feature, typed_timeline};
use crate::loss::SldprtLossCode;
use crate::native::SldprtNative;

const SKETCH: &str = "sldprt:model:sketch#spin";

fn entity(key: &str) -> String {
    format!("sldprt:model:sketch-entity#spin:{key}")
}

/// A sketch line from `start` to `end` in sketch coordinates.
struct Line {
    key: &'static str,
    start: (f64, f64),
    end: (f64, f64),
    construction: bool,
}

const fn line(key: &'static str, start: (f64, f64), end: (f64, f64)) -> Line {
    Line {
        key,
        start,
        end,
        construction: false,
    }
}

const fn centreline(key: &'static str, start: (f64, f64), end: (f64, f64)) -> Line {
    Line {
        key,
        start,
        end,
        construction: true,
    }
}

/// A rectangle 20 wide and 20 high whose left side lies at `u = left`.
fn rectangle(left: f64) -> Vec<Line> {
    let right = left + 20.0;
    vec![
        line("bottom", (left, 0.0), (right, 0.0)),
        line("right", (right, 0.0), (right, 20.0)),
        line("top", (right, 20.0), (left, 20.0)),
        line("left", (left, 20.0), (left, 0.0)),
    ]
}

/// A revolution of `Sketch1` about the axis through `origin` along
/// `direction`, with `extent` and `op`.
fn revolution(origin: [f64; 3], direction: [f64; 3], extent: &Value, op: &str) -> Value {
    json!({"kind": "resolved", "value": {
        "profile": {"kind": "sketch", "value": SKETCH},
        "axis": {
            "origin": {"x": origin[0], "y": origin[1], "z": origin[2]},
            "direction": {"x": direction[0], "y": direction[1], "z": direction[2]}},
        "extent": extent,
        "solid": true},
        "op": op})
}

fn one_sided(angle: f64) -> Value {
    json!({"kind": "one_sided", "termination": {"kind": "angle", "angle": angle}})
}

/// The full turn about the model y axis, which lies on the sketch's `u = 0`
/// line.
fn about_y() -> Value {
    revolution([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], &one_sided(TAU), "join")
}

/// A revolution whose axis no record stores: the decoder derived it.
fn derived(mut revolution: Value) -> Value {
    revolution["stored"] = json!(false);
    revolution
}

/// The `AxisOrigin` and `AxisDirection` properties the decoder gives a
/// revolution whose record stores its axis, and none where `"stored"` is
/// `false` or the construction holds no axis, with the `TurnSense` of a
/// revolution that states a `"sense"`.
fn axis_properties(revolution: &Value) -> Vec<(&'static str, String)> {
    let mut properties = revolution["sense"]
        .as_str()
        .map(|sense| ("TurnSense", sense.to_owned()))
        .into_iter()
        .collect::<Vec<_>>();
    let axis = &revolution["value"]["axis"];
    if revolution["stored"] == json!(false) || axis.is_null() {
        return properties;
    }
    let triple = |value: &Value, unit: &str| {
        ["x", "y", "z"]
            .iter()
            .map(|key| format!("{}{unit}", value[key]))
            .collect::<Vec<_>>()
            .join(",")
    };
    properties.extend([
        ("AxisOrigin", triple(&axis["origin"], "mm")),
        ("AxisDirection", triple(&axis["direction"], "")),
    ]);
    properties
}

/// `revolution`, whose record stores the turn sense `sense`.
fn sensed(mut revolution: Value, sense: &str) -> Value {
    revolution["sense"] = json!(sense);
    revolution
}

/// A part whose history holds the built-in planes, the origin, a sketch
/// `Sketch1` on the XY plane holding `lines`, and one revolve per entry of
/// `revolves` (`Revolve1`, `Revolve2`), each with the dimensions `dimensions`
/// and the decoded construction and operation of its entry (`{"kind":
/// "resolved", "value": …, "op": …}`). A `feature` profile names
/// `Sketch1`'s feature.
fn part(dimensions: &str, lines: &[Line], revolves: &[Value]) -> (CadIr, SldprtNative) {
    part_in_order(dimensions, lines, revolves, true)
}

/// [`part`], with `Sketch1` after the revolves where `sketch_first` is
/// `false`.
fn part_in_order(
    dimensions: &str,
    lines: &[Line],
    revolves: &[Value],
    sketch_first: bool,
) -> (CadIr, SldprtNative) {
    let revolve_records = (0..revolves.len())
        .map(|index| {
            format!(
                r#"<Revolve Name="Revolve{}" Type="Revolve" id="{}">{dimensions}</Revolve>"#,
                index + 1,
                20 + index
            )
        })
        .collect::<Vec<_>>()
        .concat();
    let sketch_record = r#"<Sketch Name="Sketch1" Type="ProfileFeature" id="8"/>"#;
    let (before, after) = if sketch_first {
        (sketch_record, "")
    } else {
        ("", sketch_record)
    };
    let keywords = format!(
        r#"<Keywords Name="Spin">
            <Feature Name="Front" id="2"/>
            <Feature Name="Top" id="3"/>
            <Feature Name="Right" id="4"/>
            <Sketch Name="Origin" Type="OriginProfileFeature" id="106"/>
            {before}
            {revolve_records}
            {after}
        </Keywords>"#
    );
    let names = ["Revolve1", "Revolve2"];
    let mut classes = vec![
        ("moRefPlane_c", "Front", 2),
        ("moRefPlane_c", "Top", 3),
        ("moRefPlane_c", "Right", 4),
        ("moOriginProfileFeature_c", "Origin", 106),
    ];
    if sketch_first {
        classes.push(("moProfileFeature_c", "Sketch1", 8));
    }
    for (index, name) in names.iter().take(revolves.len()).enumerate() {
        classes.push(("moRevolution_c", *name, 20 + u32::try_from(index).unwrap()));
    }
    if !sketch_first {
        classes.push(("moProfileFeature_c", "Sketch1", 8));
    }
    let mut ir = part_with_history(keywords.as_bytes(), &classes);
    let native =
        SldprtNative::load(ir.native.namespace("sldprt").unwrap(), ir.tolerances.linear).unwrap();
    let id = |name: &str| {
        ir.model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap()
            .id
            .clone()
    };
    let sketch_feature = id("Sketch1");
    let revolve_ids = names
        .iter()
        .take(revolves.len())
        .map(|name| id(name))
        .collect::<Vec<_>>();
    ir.model.sketch_entities = json::from_value(Value::Array(
        lines
            .iter()
            .map(|line| {
                json!({"id": entity(line.key), "sketch": SKETCH,
                    "construction": line.construction, "endpoint_refs": [],
                    "geometry": {"kind": "line",
                        "start": {"u": line.start.0, "v": line.start.1},
                        "end": {"u": line.end.0, "v": line.end.1}}})
            })
            .collect(),
    ))
    .unwrap();
    ir.model.sketches = json::from_value(json!([{
        "id": SKETCH, "name": "Sketch1",
        "placement": {"kind": "resolved", "frame": {"origin": {"x": 0.0, "y": 0.0, "z": 0.0},
            "normal": {"x": 0.0, "y": 0.0, "z": 1.0}, "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0}}},
        "profiles": []
    }]))
    .unwrap();
    for feature in &mut ir.model.features {
        let definition = if feature.id == sketch_feature {
            json!({"kind": "operation", "value": {"kind": "sketch",
                "value": {"sketch": {"kind": "planar", "sketch": SKETCH}}}})
        } else if let Some(index) = revolve_ids.iter().position(|id| *id == feature.id) {
            let mut revolve = revolves[index].clone();
            if revolve["value"]["profile"]["kind"] == json!("feature") {
                revolve["value"]["profile"]["value"] = json!(sketch_feature.as_str());
            }
            for (key, value) in axis_properties(&revolve) {
                feature
                    .source_properties
                    .insert(NonBlankString::new(key).unwrap(), value);
            }
            json!({"kind": "operation", "value": {"kind": "revolve", "value": {
                "construction": {"kind": revolve["kind"], "value": revolve["value"]},
                "op": revolve["op"]}}})
        } else {
            continue;
        };
        let definition = json::from_value(definition).unwrap();
        feature.evaluation.edit(|current, _| *current = definition);
    }
    (ir, native)
}

/// The typed revolve `name`, or the reason the design leaves it
/// `Unresolved`, from the note naming it.
fn typed(ir: &CadIr, native: &SldprtNative, name: &str) -> Result<Revolve, String> {
    let (timeline, _, typed) = typed_timeline(ir, native);
    let feature = timeline_feature(&timeline, name);
    match &feature.definition.operation {
        FeatureOperation::Revolve(revolve) => Ok(revolve.as_ref().clone()),
        FeatureOperation::Unresolved {
            family: OperationFamily::Revolve,
        } => Err(typed.loss.map_or_else(String::new, |note| note.message)),
        other => panic!("{name} is {other:?}"),
    }
}

/// The sketch entity the revolve's axis picks, and whether the axis runs
/// against it.
fn axis_pick(revolve: &Revolve) -> (Option<String>, Option<bool>) {
    let axis = revolve.axis.as_ref().unwrap();
    assert!(axis.after.is_empty(), "{:?}", axis.after);
    match axis.inputs.as_slice() {
        [] => (None, None),
        [input] => {
            assert!(matches!(
                input.role,
                Some(InputRole {
                    subject: Subject::Whole,
                    relation: Relation::Coincident
                })
            ));
            let Some(InputReference::Picks(selection)) = &input.reference else {
                panic!("the axis input picks");
            };
            let Picks::Named(picks) = &selection.picks else {
                panic!("the axis input names its pick");
            };
            let [pick] = picks.as_slice() else {
                panic!("one pick");
            };
            let PickTarget::SketchEntity(line) = &pick.target else {
                panic!("the axis picks a sketch entity: {:?}", pick.target);
            };
            (Some(line.as_str().to_owned()), input.reversed)
        }
        inputs => panic!("one input at most: {inputs:?}"),
    }
}

fn with_centreline() -> Vec<Line> {
    let mut lines = rectangle(10.0);
    lines.push(centreline("axis", (0.0, -5.0), (0.0, 25.0)));
    lines
}

#[test]
fn a_full_turn_about_a_centreline_picks_it_and_binds_its_angle() {
    let (mut ir, native) = part(
        r#"<Dimension Name="D1">360deg</Dimension>"#,
        &with_centreline(),
        &[about_y()],
    );
    let (timeline, entries, typed) = typed_timeline(&ir, &native);
    let feature = timeline_feature(&timeline, "Revolve1");
    let FeatureOperation::Revolve(revolve) = &feature.definition.operation else {
        panic!(
            "the revolve is typed: {:?} {:?}",
            feature.definition.operation, typed.loss
        );
    };
    let Picks::Named(picks) = &revolve.profile.picks else {
        panic!("the profile names its pick");
    };
    assert!(matches!(picks.as_slice(),
        [pick] if pick.target == PickTarget::Sketch(json::from_value(json!(SKETCH)).unwrap())));
    assert_eq!(axis_pick(revolve), (Some(entity("axis")), None));
    assert_eq!(
        revolve.axis.as_ref().unwrap().resolved,
        Some(
            json::from_value::<AxisValue>(json!({
            "origin": {"x": 0.0, "y": 0.0, "z": 0.0}, "direction": {"x": 0.0, "y": 1.0, "z": 0.0}}))
            .unwrap()
        )
    );
    let angle = entries.owned(&feature.id, "D1").unwrap().clone();
    assert_eq!(
        revolve.span,
        Some(Span::Between {
            start: Box::new(Limit::AtOrigin),
            end: Box::new(Limit::Amount(Dimension::Parameter(angle.clone()))),
        })
    );
    assert_eq!(typed.bound.get(&angle), Some(&Quantity::ANGLE));
    assert_eq!(revolve.output.boolean, Some(Boolean::NewBody));
    assert_eq!(revolve.output.body, Some(BodyForm::Solid));
    assert!(typed.loss.is_none(), "{:?}", typed.loss);
    let note = typed.revolve_axis_lines.expect("the picked line is named");
    assert_eq!(
        note.code,
        SldprtLossCode::DesignRevolveAxisLineInferred.kind()
    );
    assert!(
        note.message.starts_with("1 revolves")
            && note.message.contains(feature.id.as_str())
            && note.message.contains(&entity("axis"))
            && !note.message.contains("one of"),
        "{}",
        note.message
    );
    let unchecked = typed
        .revolve_profile_unchecked
        .expect("no stored loop to compare");
    assert_eq!(
        unchecked.code,
        SldprtLossCode::DesignRevolveProfileUnchecked.kind()
    );

    let definition = ir.design.scopes[0].definition.clone();
    let (parameters, _, _) = entries.parameters(
        &definition,
        &super::super::parameters::Readings {
            decoded: super::super::parameters::Decoded::new(
                &ir.model.parameters,
                &[],
                &crate::resolved_features::assembly::ConfigurationMaps::new(),
            ),
            length_unit: Some(UnitSymbol::Mm),
            length_places: None,
            bound: typed.bound,
            measured: typed.measured,
            driven: typed.references,
            manager: std::collections::HashMap::new(),
            confirmed: std::collections::HashSet::new(),
        },
    );
    ir.design.features = timeline.features;
    ir.design.parameters = parameters;
    ir.design.sketches = typed.sketches.records;
    ir.design.sketch_entities = typed.sketches.entities;
    ir.design.sketch_constraints = typed.sketches.constraints;
    assert_no_design_findings(&ir);
}

#[test]
fn an_axis_that_runs_against_its_line_reverses_the_input() {
    let (ir, native) = part(
        r#"<Dimension Name="D1">360deg</Dimension>"#,
        &with_centreline(),
        &[revolution(
            [0.0, 4.0, 0.0],
            [0.0, -1.0, 0.0],
            &one_sided(TAU),
            "join",
        )],
    );
    let revolve = typed(&ir, &native, "Revolve1").unwrap();
    assert_eq!(axis_pick(&revolve), (Some(entity("axis")), Some(true)));
}

#[test]
fn a_profile_line_on_the_axis_is_picked_where_no_centreline_lies_on_it() {
    let (ir, native) = part("", &rectangle(0.0), &[about_y()]);
    let revolve = typed(&ir, &native, "Revolve1").unwrap();
    assert_eq!(axis_pick(&revolve), (Some(entity("left")), Some(true)));

    let mut lines = rectangle(0.0);
    lines.push(centreline("axis", (0.0, -5.0), (0.0, 25.0)));
    let (ir, native) = part("", &lines, &[about_y()]);
    let (_, _, typed) = typed_timeline(&ir, &native);
    let message = typed.revolve_axis_lines.unwrap().message;
    assert!(
        message.contains(&entity("axis")) && message.contains("one of 2 lines on the axis"),
        "{message}"
    );
}

#[test]
fn an_axis_on_no_line_of_the_sketch_states_only_its_value() {
    let (ir, native) = part(
        "",
        &rectangle(10.0),
        &[revolution(
            [-5.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            &one_sided(TAU),
            "join",
        )],
    );
    let revolve = typed(&ir, &native, "Revolve1").unwrap();
    assert_eq!(axis_pick(&revolve), (None, None));
    assert!(revolve.axis.unwrap().resolved.is_some());
    let (_, _, typed) = typed_timeline(&ir, &native);
    assert!(typed.revolve_axis_lines.is_none());
}

#[test]
fn an_axis_outside_the_sketch_plane_leaves_the_revolve_unresolved() {
    let (ir, native) = part(
        "",
        &with_centreline(),
        &[revolution(
            [0.0, 0.0, 5.0],
            [0.0, 1.0, 0.0],
            &one_sided(TAU),
            "join",
        )],
    );
    let reason = typed(&ir, &native, "Revolve1").unwrap_err();
    assert!(
        reason.starts_with("0 sketch, 0 extrude and 1 revolve features"),
        "{reason}"
    );
    assert!(
        reason.contains("outside its profile sketch's plane (DI-85)"),
        "{reason}"
    );
}

#[test]
fn a_turn_whose_sense_matters_leaves_the_revolve_unresolved() {
    for (extent, reason) in [
        (one_sided(PI), "less than a full turn on one side"),
        (
            json!({"kind": "two_sided",
                "first": {"kind": "angle", "angle": PI}, "second": {"kind": "angle", "angle": 1.0}}),
            "two-sided",
        ),
        (
            json!({"kind": "one_sided", "termination": {"kind": "through_all"}}),
            "limit is no angle",
        ),
    ] {
        let (ir, native) = part(
            "",
            &with_centreline(),
            &[revolution(
                [0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                &extent,
                "join",
            )],
        );
        let message = typed(&ir, &native, "Revolve1").unwrap_err();
        assert!(message.contains(reason), "{message}");
    }
}

#[test]
fn a_mid_plane_angle_is_a_symmetric_total() {
    let (ir, native) = part(
        r#"<Dimension Name="Angle">90deg</Dimension>"#,
        &with_centreline(),
        &[revolution(
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            &json!({"kind": "symmetric", "termination": {"kind": "angle", "angle": PI / 2.0}}),
            "join",
        )],
    );
    let (timeline, entries, _) = typed_timeline(&ir, &native);
    let feature = timeline_feature(&timeline, "Revolve1");
    let FeatureOperation::Revolve(revolve) = &feature.definition.operation else {
        panic!("typed: {:?}", feature.definition.operation);
    };
    assert_eq!(
        revolve.span,
        Some(Span::Symmetric {
            amount: Dimension::Parameter(entries.owned(&feature.id, "Angle").unwrap().clone()),
            measure: Some(SymmetricMeasure::Total),
        })
    );
}

#[test]
fn an_angle_no_dimension_holds_is_a_literal() {
    for dimensions in ["", r#"<Dimension Name="D1">90deg</Dimension>"#] {
        let (ir, native) = part(dimensions, &with_centreline(), &[about_y()]);
        let revolve = typed(&ir, &native, "Revolve1").unwrap();
        let Some(Span::Between { end, .. }) = revolve.span else {
            panic!("one-sided");
        };
        let Limit::Amount(Dimension::Literal(angle)) = *end else {
            panic!("a literal angle: {end:?}");
        };
        assert!((angle.get() - TAU).abs() < 1e-12);
        let (_, _, typed) = typed_timeline(&ir, &native);
        assert!(typed.bound.is_empty(), "{dimensions}: {:?}", typed.bound);
    }
}

#[test]
fn a_join_after_a_body_joins_and_a_cut_cuts() {
    let (ir, native) = part(
        "",
        &with_centreline(),
        &[
            about_y(),
            revolution([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], &one_sided(TAU), "join"),
        ],
    );
    assert_eq!(
        typed(&ir, &native, "Revolve1").unwrap().output.boolean,
        Some(Boolean::NewBody)
    );
    assert_eq!(
        typed(&ir, &native, "Revolve2").unwrap().output.boolean,
        Some(Boolean::Apply {
            op: BooleanOp::Join,
            targets: None
        })
    );
    let (ir, native) = part(
        "",
        &with_centreline(),
        &[revolution(
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            &one_sided(TAU),
            "cut",
        )],
    );
    assert_eq!(
        typed(&ir, &native, "Revolve1").unwrap().output.boolean,
        Some(Boolean::Apply {
            op: BooleanOp::Cut,
            targets: None
        })
    );
}

#[test]
fn an_unread_operand_or_an_empty_sketch_leaves_the_revolve_unresolved() {
    let unread = json!({"kind": "unresolved", "value": {"kind": "axis",
        "profile": {"kind": "sketch", "value": SKETCH}, "solid": true}, "op": "join"});
    let (ir, native) = part("", &with_centreline(), &[unread]);
    let message = typed(&ir, &native, "Revolve1").unwrap_err();
    assert!(
        message.contains("the revolve's axis is not read"),
        "{message}"
    );

    let (ir, native) = part(
        "",
        &[centreline("axis", (0.0, -5.0), (0.0, 25.0))],
        &[about_y()],
    );
    let message = typed(&ir, &native, "Revolve1").unwrap_err();
    assert!(message.contains("holds no profile curve"), "{message}");

    let native_profile = json!({"kind": "resolved", "value": {
        "profile": {"kind": "native", "value": "sldprt:history:feature#0:8"},
        "axis": {"origin": {"x": 0.0, "y": 0.0, "z": 0.0}, "direction": {"x": 0.0, "y": 1.0, "z": 0.0}},
        "extent": one_sided(TAU), "solid": true}, "op": "join"});
    let (ir, native) = part("", &with_centreline(), &[native_profile]);
    let message = typed(&ir, &native, "Revolve1").unwrap_err();
    assert!(message.contains("profile is no whole sketch"), "{message}");
}

#[test]
fn a_revolve_that_adds_material_after_a_body_states_no_boolean() {
    let unread = || {
        revolution(
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            &one_sided(TAU),
            "unresolved",
        )
    };
    let (ir, native) = part("", &with_centreline(), &[unread(), unread()]);
    assert_eq!(
        typed(&ir, &native, "Revolve1").unwrap().output.boolean,
        Some(Boolean::NewBody)
    );
    assert_eq!(
        typed(&ir, &native, "Revolve2").unwrap().output.boolean,
        None
    );
    let (timeline, _, projected) = typed_timeline(&ir, &native);
    let note = projected
        .revolve_boolean_unstated
        .expect("the unstated boolean is named");
    assert_eq!(
        note.code,
        SldprtLossCode::DesignRevolveBooleanUnstated.kind()
    );
    let second = timeline_feature(&timeline, "Revolve2");
    assert!(
        note.message.starts_with("1 revolves")
            && note.message.contains(second.id.as_str())
            && note.message.contains("tells a new body from a join"),
        "{}",
        note.message
    );

    let (ir, native) = part(
        "",
        &with_centreline(),
        &[
            unread(),
            revolution(
                [0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                &one_sided(TAU),
                "new_body",
            ),
        ],
    );
    assert_eq!(
        typed(&ir, &native, "Revolve2").unwrap().output.boolean,
        Some(Boolean::NewBody)
    );
}

#[test]
fn a_derived_axis_on_a_line_of_the_sketch_states_that_line_with_a_warning() {
    let (ir, native) = part("", &with_centreline(), &[derived(about_y())]);
    let revolve = typed(&ir, &native, "Revolve1").unwrap();
    assert_eq!(axis_pick(&revolve), (Some(entity("axis")), None));
    let (timeline, _, typed) = typed_timeline(&ir, &native);
    assert!(typed.revolve_axis_lines.is_none());
    let note = typed
        .revolve_axis_derived
        .expect("the derived axis is named");
    assert_eq!(note.code, SldprtLossCode::DesignRevolveAxisDerived.kind());
    assert_eq!(note.severity, Severity::Warning);
    let feature = timeline_feature(&timeline, "Revolve1");
    assert!(
        note.message.contains(feature.id.as_str())
            && note.message.contains(&entity("axis"))
            && note.message.contains("no record stores the axis"),
        "{}",
        note.message
    );
}

#[test]
fn a_derived_axis_on_no_line_of_the_sketch_leaves_the_revolve_unresolved() {
    let (ir, native) = part(
        "",
        &rectangle(10.0),
        &[derived(revolution(
            [-5.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            &one_sided(TAU),
            "join",
        ))],
    );
    let message = typed(&ir, &native, "Revolve1").unwrap_err();
    assert!(
        message.contains("no record stores the revolve's axis, and no line of its sketch"),
        "{message}"
    );
}

#[test]
fn an_angle_without_angle_or_d1_binds_the_first_positive_angle_dimension() {
    let (ir, native) = part(
        r#"<Dimension Name="D3">360deg</Dimension>"#,
        &with_centreline(),
        &[about_y()],
    );
    let (timeline, entries, typed) = typed_timeline(&ir, &native);
    let feature = timeline_feature(&timeline, "Revolve1");
    let FeatureOperation::Revolve(revolve) = &feature.definition.operation else {
        panic!("typed: {:?}", feature.definition.operation);
    };
    let angle = entries.owned(&feature.id, "D3").unwrap().clone();
    assert_eq!(
        revolve.span,
        Some(Span::Between {
            start: Box::new(Limit::AtOrigin),
            end: Box::new(Limit::Amount(Dimension::Parameter(angle.clone()))),
        })
    );
    assert_eq!(typed.bound.get(&angle), Some(&Quantity::ANGLE));
}

#[test]
fn a_feature_profile_picks_the_sketch_of_that_feature() {
    let mut revolution = about_y();
    revolution["value"]["profile"] = json!({"kind": "feature"});
    let (ir, native) = part("", &with_centreline(), &[revolution]);
    let revolve = typed(&ir, &native, "Revolve1").unwrap();
    let Picks::Named(picks) = &revolve.profile.picks else {
        panic!("the profile names its pick");
    };
    assert!(matches!(picks.as_slice(),
        [pick] if pick.target == PickTarget::Sketch(json::from_value(json!(SKETCH)).unwrap())));
}

#[test]
fn a_sketch_the_revolve_cannot_see_leaves_it_unresolved() {
    let (ir, native) = part_in_order("", &with_centreline(), &[about_y()], false);
    let message = typed(&ir, &native, "Revolve1").unwrap_err();
    assert!(
        message.contains("sketch follows it in the timeline"),
        "{message}"
    );

    let mut elsewhere = about_y();
    elsewhere["value"]["profile"]["value"] = json!("sldprt:model:sketch#elsewhere");
    let (ir, native) = part("", &with_centreline(), &[elsewhere]);
    let message = typed(&ir, &native, "Revolve1").unwrap_err();
    assert!(message.contains("sketch is not stated"), "{message}");
}

#[test]
fn an_option_of_another_format_leaves_the_revolve_unresolved() {
    let mut revolution = about_y();
    revolution["value"]["allow_multi_profile_faces"] = json!(true);
    let (ir, native) = part("", &with_centreline(), &[revolution]);
    let message = typed(&ir, &native, "Revolve1").unwrap_err();
    assert!(message.contains("option of another format"), "{message}");
}

#[test]
fn a_partial_turn_whose_record_stores_its_sense_is_typed_with_a_warning() {
    for sense in ["Stored", "Reversed"] {
        let (ir, native) = part(
            r#"<Dimension Name="D1">90deg</Dimension>"#,
            &with_centreline(),
            &[sensed(
                revolution(
                    [0.0, 0.0, 0.0],
                    [0.0, -1.0, 0.0],
                    &one_sided(FRAC_PI_2),
                    "join",
                ),
                sense,
            )],
        );
        let revolve = typed(&ir, &native, "Revolve1").unwrap();
        assert_eq!(axis_pick(&revolve), (Some(entity("axis")), Some(true)));
        let Some(Span::Between { start, end }) = &revolve.span else {
            panic!("a one-sided span: {:?}", revolve.span);
        };
        assert!(matches!(**start, Limit::AtOrigin));
        assert!(matches!(**end, Limit::Amount(Dimension::Parameter(_))));
        let (timeline, _, typed) = typed_timeline(&ir, &native);
        let note = typed
            .revolve_sense_assumed
            .expect("the partial turn is named");
        assert_eq!(note.code, SldprtLossCode::DesignRevolveSenseAssumed.kind());
        assert_eq!(note.severity, Severity::Warning);
        let feature = timeline_feature(&timeline, "Revolve1");
        assert!(
            note.message.contains(feature.id.as_str()) && note.message.contains("EV-25"),
            "{}",
            note.message
        );
    }
}

#[test]
fn a_reversed_full_turn_is_typed_about_the_reversed_axis_without_a_warning() {
    for (sense, direction, reversed) in [
        ("Stored", [0.0, 1.0, 0.0], None),
        ("Reversed", [0.0, -1.0, 0.0], Some(true)),
    ] {
        let (ir, native) = part(
            r#"<Dimension Name="D1">360deg</Dimension>"#,
            &with_centreline(),
            &[sensed(
                revolution([0.0, 0.0, 0.0], direction, &one_sided(TAU), "join"),
                sense,
            )],
        );
        let revolve = typed(&ir, &native, "Revolve1").unwrap();
        assert_eq!(axis_pick(&revolve), (Some(entity("axis")), reversed));
        assert_eq!(
            revolve.axis.as_ref().unwrap().resolved,
            Some(
                json::from_value::<AxisValue>(json!({
                "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                "direction": {"x": direction[0], "y": direction[1], "z": direction[2]}}))
                .unwrap()
            )
        );
        assert!(matches!(&revolve.span, Some(Span::Between { start, .. })
            if matches!(**start, Limit::AtOrigin)));
        let (_, _, projected) = typed_timeline(&ir, &native);
        assert!(projected.revolve_sense_assumed.is_none());
    }
}

#[test]
fn only_a_sense_read_with_the_axis_that_the_solid_depends_on_is_named() {
    let two_sided = json!({"kind": "resolved", "value": {
        "profile": {"kind": "sketch", "value": SKETCH},
        "axis": {"origin": {"x": 0.0, "y": 0.0, "z": 0.0},
            "direction": {"x": 0.0, "y": 1.0, "z": 0.0}},
        "extent": {"kind": "two_sided",
            "first": {"kind": "angle", "angle": PI},
            "second": {"kind": "angle", "angle": 1.0}},
        "solid": true},
        "op": "join"});
    let partial = revolution([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], &one_sided(PI), "join");
    for (revolve, named, reason) in [
        (sensed(about_y(), "Reversed"), false, None),
        (
            partial.clone(),
            false,
            Some("less than a full turn on one side"),
        ),
        (
            derived(sensed(partial, "Stored")),
            false,
            Some("less than a full turn on one side"),
        ),
        (sensed(two_sided, "Stored"), true, Some("two-sided")),
    ] {
        let (ir, native) = part("", &with_centreline(), &[revolve]);
        match (typed(&ir, &native, "Revolve1"), reason) {
            (Ok(_), None) => {}
            (Err(message), Some(reason)) => assert!(message.contains(reason), "{message}"),
            (result, reason) => panic!("{reason:?}: {result:?}"),
        }
        let (_, _, projected) = typed_timeline(&ir, &native);
        assert_eq!(projected.revolve_sense_assumed.is_some(), named);
    }
}
