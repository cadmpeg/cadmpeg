// SPDX-License-Identifier: Apache-2.0
//! Template instances and body operations.

use std::collections::BTreeMap;

use serde_json::json;

use super::joints::connectors;
use super::{
    angle, axis_construction, direction_construction, faces, frame_construction, id, length,
    literal, options, ordered, parameter, plane_construction, point_construction, refuses,
    settings, text, to_value, unread, Report,
};
use crate::design::dimension::Dimension;
use crate::design::ops::body::{
    BodyMate, Combine, DeleteBodies, DeleteFaces, ExternalDocument, Extract, Flex, FlexKind, Graft,
    Heal, Kept, MoveFaces, NewComponent, PieceForm, ReplaceFaces, Retention, ScaleFactors, Split,
    SplitMethod, SplitMode, SplitTool, Template, TemplateInput, TemplateInstance,
    TemplateParameter, ToolExtension, TransformBodies, TransformMode, TransformStep,
    Transformation,
};
use crate::design::ops::shared::{Alignment, JointKind, Locus, ToolDisposition};
use crate::design::ops::surface::ExtendMethod;
use crate::design::options::{BooleanOp, ProjectionDirection};
use crate::design::tests::arm_failures;
use crate::scalar::{FiniteReal, NonNegativeLength, NonZeroReal, PositiveLength};
use crate::transform::Transform;

fn non_zero(value: f64) -> NonZeroReal {
    NonZeroReal::new(value).unwrap()
}

fn documents() -> Vec<ExternalDocument> {
    vec![
        ExternalDocument {
            path: Some(text("lib/udf.gph")),
            document_id: Some(text("0f1e")),
            version: Some(text("3")),
            stamp: Some(text("2026-01-01")),
            object: Some(text("Groove")),
        },
        ExternalDocument {
            path: None,
            document_id: None,
            version: None,
            stamp: None,
            object: None,
        },
    ]
}

fn templates() -> Vec<Template> {
    vec![
        Template {
            name: Some(text("slot_udf")),
            library: documents().pop(),
            variant: Some(text("M6")),
        },
        Template {
            name: None,
            library: None,
            variant: None,
        },
    ]
}

fn template_inputs() -> Vec<TemplateInput> {
    vec![
        TemplateInput {
            role: Some(text("placement surface")),
            reference: ordered(30),
        },
        TemplateInput {
            role: None,
            reference: unread(),
        },
    ]
}

fn template_parameters() -> Vec<TemplateParameter> {
    vec![
        TemplateParameter {
            role: Some(text("depth")),
            parameter: id("parameter", 31),
        },
        TemplateParameter {
            role: None,
            parameter: id("parameter", 32),
        },
    ]
}

fn template_instances() -> Vec<TemplateInstance> {
    vec![
        TemplateInstance {
            template: templates().remove(0),
            inputs: template_inputs(),
            parameters: template_parameters(),
            members: vec![id("feature", 33), id("feature", 34)],
            native_options: options(),
            source_settings: settings(),
        },
        TemplateInstance {
            template: templates().remove(1),
            inputs: Vec::new(),
            parameters: Vec::new(),
            members: Vec::new(),
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        },
    ]
}

fn dispositions() -> Vec<ToolDisposition> {
    vec![
        ToolDisposition::Kept,
        ToolDisposition::Consumed,
        ToolDisposition::Absorbed,
    ]
}

fn new_components() -> Vec<NewComponent> {
    vec![NewComponent {
        occurrence: id("occurrence", 35),
    }]
}

fn combines() -> Vec<Combine> {
    vec![
        Combine {
            target: Some(faces(36)),
            tools: ordered(37),
            op: Some(BooleanOp::UnionTrim),
            tools_after: Some(ToolDisposition::Absorbed),
            new_component: new_components().pop(),
            native_options: options(),
            source_settings: settings(),
        },
        Combine {
            target: None,
            tools: ordered(38),
            op: None,
            tools_after: None,
            new_component: None,
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        },
    ]
}

fn piece_forms() -> Vec<PieceForm> {
    vec![PieceForm::Bodies, PieceForm::Regions, PieceForm::Faces]
}

fn flag_kept() -> Vec<Kept<Dimension<bool>>> {
    vec![
        Kept::All {
            form: Some(PieceForm::Faces),
        },
        Kept::All { form: None },
        Kept::Side {
            side: literal(true),
        },
        Kept::Pieces {
            keep: Some(Box::new(faces(39))),
            remove: Some(Box::new(faces(40))),
        },
        Kept::Pieces {
            keep: None,
            remove: None,
        },
        Kept::Native(text("sldprt:trim_all")),
    ]
}

fn list_kept() -> Vec<Kept<Vec<Option<Dimension<bool>>>>> {
    vec![
        Kept::Side {
            side: vec![Some(literal(true)), None],
        },
        Kept::Side { side: Vec::new() },
        Kept::All {
            form: Some(PieceForm::Bodies),
        },
    ]
}

fn split_tools() -> Vec<SplitTool> {
    vec![
        SplitTool::Plane(Box::new(plane_construction())),
        SplitTool::Geometry {
            tools: faces(41),
            extend: None,
        },
        SplitTool::Geometry {
            tools: faces(41),
            extend: tool_extensions().pop(),
        },
        SplitTool::Silhouette(Box::new(direction_construction())),
    ]
}

fn tool_extensions() -> Vec<ToolExtension> {
    vec![
        ToolExtension::Off,
        ToolExtension::Extended { method: None },
        ToolExtension::Extended {
            method: Some(ExtendMethod::Tangent),
        },
    ]
}

fn split_methods() -> Vec<SplitMethod> {
    vec![
        SplitMethod::Intersect,
        SplitMethod::Project(Box::new(ProjectionDirection::SurfaceNormal)),
        SplitMethod::Project(Box::new(ProjectionDirection::Along(
            direction_construction(),
        ))),
    ]
}

fn split_modes() -> Vec<SplitMode> {
    vec![
        SplitMode::ByTools {
            targets: Some(faces(42)),
            tools: split_tools().remove(0),
            method: Some(SplitMethod::Intersect),
            kept: Some(Kept::Side {
                side: literal(false),
            }),
        },
        SplitMode::ByTools {
            targets: None,
            tools: split_tools().remove(1),
            method: None,
            kept: None,
        },
        SplitMode::ByTools {
            targets: Some(faces(42)),
            tools: split_tools().remove(3),
            method: None,
            kept: Some(Kept::Native(text("f3d:silhouette_split_solid_body"))),
        },
        SplitMode::Mutual {
            elements: ordered(43),
            kept: list_kept().pop(),
        },
        SplitMode::Mutual {
            elements: ordered(43),
            kept: None,
        },
    ]
}

fn splits() -> Vec<Split> {
    vec![
        Split {
            mode: split_modes().remove(0),
            native_options: options(),
            source_settings: settings(),
        },
        Split {
            mode: split_modes().remove(4),
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        },
    ]
}

fn delete_bodies() -> Vec<DeleteBodies> {
    vec![
        DeleteBodies {
            bodies: faces(44),
            retain: Some(Retention::KeepSelected),
            native_options: options(),
            source_settings: settings(),
        },
        DeleteBodies {
            bodies: unread(),
            retain: None,
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        },
    ]
}

fn extracts() -> Vec<Extract> {
    vec![
        Extract {
            source: faces(45),
            reversed: Some(true),
            native_options: options(),
            source_settings: settings(),
        },
        Extract {
            source: faces(45),
            reversed: None,
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        },
    ]
}

fn scale_factors() -> Vec<ScaleFactors> {
    vec![
        ScaleFactors::Uniform(literal(non_zero(2.0))),
        ScaleFactors::PerAxis {
            factors: [
                literal(non_zero(1.0)),
                parameter(46),
                literal(non_zero(-0.5)),
            ],
            frame: Some(Box::new(frame_construction())),
        },
        ScaleFactors::PerAxis {
            factors: [
                literal(non_zero(1.0)),
                literal(non_zero(2.0)),
                literal(non_zero(3.0)),
            ],
            frame: None,
        },
    ]
}

pub(super) fn body_mates() -> Vec<BodyMate> {
    let [first, second, ..] = <[_; 5]>::try_from(connectors()).unwrap();
    vec![
        BodyMate {
            connectors: [first.clone(), second.clone()],
            kind: JointKind::Coincident {
                offset: Some(literal(length(1.0))),
            },
            alignment: Some(Alignment::Opposed),
        },
        BodyMate {
            connectors: [second, first],
            kind: JointKind::Parallel,
            alignment: None,
        },
    ]
}

fn steps() -> Vec<TransformStep> {
    vec![
        TransformStep::Translate {
            direction: direction_construction(),
            distance: literal(length(5.0)),
        },
        TransformStep::Components {
            frame: Some(frame_construction()),
            delta: [literal(length(1.0)), parameter(47), literal(length(0.0))],
        },
        TransformStep::Components {
            frame: None,
            delta: [
                literal(length(1.0)),
                literal(length(2.0)),
                literal(length(3.0)),
            ],
        },
        TransformStep::PointToPoint {
            from: point_construction(),
            to: point_construction(),
        },
        TransformStep::Rotate {
            axis: axis_construction(),
            angle: literal(angle(0.25)),
        },
        TransformStep::FrameToFrame {
            from: frame_construction(),
            to: frame_construction(),
        },
        TransformStep::Scale {
            center: Some(Locus::Centroid),
            factors: ScaleFactors::Uniform(literal(non_zero(0.5))),
        },
        TransformStep::Scale {
            center: None,
            factors: scale_factors().remove(1),
        },
        TransformStep::Fixed(Transform::identity()),
        TransformStep::Constrained {
            mates: body_mates(),
        },
        TransformStep::Native(text("nx:csys_to_csys")),
    ]
}

fn transformations() -> Vec<Transformation> {
    vec![
        Transformation {
            steps: steps(),
            resolved: Some(Transform::identity()),
        },
        Transformation {
            steps: Vec::new(),
            resolved: None,
        },
    ]
}

fn transform_modes() -> Vec<TransformMode> {
    vec![
        TransformMode::Move,
        TransformMode::Copy {
            count: literal(4),
            divisions: Some(literal(4)),
        },
        TransformMode::Copy {
            count: parameter(48),
            divisions: None,
        },
    ]
}

fn transform_bodies() -> Vec<TransformBodies> {
    vec![
        TransformBodies {
            bodies: faces(49),
            transformation: transformations().remove(0),
            mode: Some(TransformMode::Move),
            native_options: options(),
            source_settings: settings(),
        },
        TransformBodies {
            bodies: faces(49),
            transformation: transformations().remove(1),
            mode: None,
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        },
    ]
}

fn move_faces() -> Vec<MoveFaces> {
    vec![
        MoveFaces {
            faces: faces(50),
            transformation: transformations().remove(0),
            native_options: options(),
            source_settings: settings(),
        },
        MoveFaces {
            faces: faces(50),
            transformation: transformations().remove(1),
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        },
    ]
}

fn heals() -> Vec<Heal> {
    vec![
        Heal::Open,
        Heal::Extend,
        Heal::Fill {
            tangent: Some(literal(true)),
        },
        Heal::Fill { tangent: None },
        Heal::Native(text("catia:remove_lump")),
    ]
}

fn delete_faces() -> Vec<DeleteFaces> {
    vec![
        DeleteFaces {
            faces: faces(51),
            heal: Some(Heal::Extend),
            native_options: options(),
            source_settings: settings(),
        },
        DeleteFaces {
            faces: faces(51),
            heal: None,
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        },
    ]
}

fn replace_faces() -> Vec<ReplaceFaces> {
    vec![
        ReplaceFaces {
            faces: faces(52),
            replacement: faces(53),
            native_options: options(),
            source_settings: settings(),
        },
        ReplaceFaces {
            faces: unread(),
            replacement: unread(),
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        },
    ]
}

fn grafts() -> Vec<Graft> {
    vec![
        Graft {
            body: faces(54),
            sheet: faces(55),
            flip: Some(literal(true)),
            simplify: Some(parameter(56)),
            native_options: options(),
            source_settings: settings(),
        },
        Graft {
            body: faces(54),
            sheet: faces(55),
            flip: None,
            simplify: None,
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        },
    ]
}

fn flex_kinds() -> Vec<FlexKind> {
    vec![
        FlexKind::Bend {
            angle: literal(angle(1.5)),
            radius: Some(literal(PositiveLength::new(20.0).unwrap())),
        },
        FlexKind::Bend {
            angle: parameter(57),
            radius: None,
        },
        FlexKind::Twist {
            angle: literal(angle(0.5)),
        },
        FlexKind::Taper {
            factor: literal(FiniteReal::new(1.5).unwrap()),
        },
        FlexKind::Stretch {
            distance: literal(length(-3.0)),
        },
        FlexKind::Native(text("sldprt:flex")),
    ]
}

fn flexes() -> Vec<Flex> {
    vec![
        Flex {
            bodies: faces(58),
            frame: frame_construction(),
            trims: [
                Some(literal(NonNegativeLength::new(0.0).unwrap())),
                Some(parameter(59)),
            ],
            kind: flex_kinds().remove(0),
            native_options: options(),
            source_settings: settings(),
        },
        Flex {
            bodies: faces(58),
            frame: frame_construction(),
            trims: [None, None],
            kind: flex_kinds().remove(2),
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        },
    ]
}

#[test]
fn every_template_and_body_operation_type_round_trips() {
    let mut report = Report::default();
    report.check("TemplateInstance", template_instances());
    report.check("Template", templates());
    report.check("TemplateInput", template_inputs());
    report.check("TemplateParameter", template_parameters());
    report.check("ExternalDocument", documents());
    report.check("Combine", combines());
    report.check("ToolDisposition", dispositions());
    report.check("NewComponent", new_components());
    report.check("Split", splits());
    report.check("SplitMode", split_modes());
    report.check("SplitTool", split_tools());
    report.check("ToolExtension", tool_extensions());
    report.check("SplitMethod", split_methods());
    report.check("Kept<Dimension<bool>>", flag_kept());
    report.check("Kept<Vec<Option<Dimension<bool>>>>", list_kept());
    report.check("PieceForm", piece_forms());
    report.check("DeleteBodies", delete_bodies());
    report.check(
        "Retention",
        vec![Retention::DeleteSelected, Retention::KeepSelected],
    );
    report.check("Extract", extracts());
    report.check("TransformBodies", transform_bodies());
    report.check("TransformMode", transform_modes());
    report.check("Transformation", transformations());
    report.check("TransformStep", steps());
    report.check("BodyMate", body_mates());
    report.check("ScaleFactors", scale_factors());
    report.check("MoveFaces", move_faces());
    report.check("DeleteFaces", delete_faces());
    report.check("Heal", heals());
    report.check("ReplaceFaces", replace_faces());
    report.check("Graft", grafts());
    report.check("Flex", flexes());
    report.check("FlexKind", flex_kinds());
    report.finish();
}

/// §5.10: each template and body operation is the `FeatureOperation` arm its
/// family names, holding the operation as its value.
#[test]
fn every_template_and_body_operation_is_a_feature_operation_arm() {
    use crate::design::feature::{FeatureOperation as Op, OperationFamily as Family};
    let failures = [
        arm_failures(
            Family::TemplateInstance,
            template_instances(),
            Op::TemplateInstance,
        ),
        arm_failures(Family::Combine, combines(), Op::Combine),
        arm_failures(Family::Split, splits(), Op::Split),
        arm_failures(Family::DeleteBodies, delete_bodies(), Op::DeleteBodies),
        arm_failures(Family::Extract, extracts(), Op::Extract),
        arm_failures(
            Family::TransformBodies,
            transform_bodies(),
            Op::TransformBodies,
        ),
        arm_failures(Family::Flex, flexes(), Op::Flex),
        arm_failures(Family::MoveFaces, move_faces(), Op::MoveFaces),
        arm_failures(Family::DeleteFaces, delete_faces(), Op::DeleteFaces),
        arm_failures(Family::ReplaceFaces, replace_faces(), Op::ReplaceFaces),
        arm_failures(Family::Graft, grafts(), Op::Graft),
    ]
    .concat();
    assert!(failures.is_empty(), "{failures:#?}");
}

/// §2.5's example: an internally tagged arm writes its fields beside its
/// tag, and a unit arm is its tag alone.
#[test]
fn the_examples_of_section_2_5() {
    let copy = TransformMode::Copy {
        count: literal(4),
        divisions: Some(literal(4)),
    };
    assert_eq!(
        to_value(&copy),
        json!({"kind": "copy", "count": {"kind": "literal", "value": 4}, "divisions": {"kind": "literal", "value": 4}})
    );
    assert_eq!(to_value(&TransformMode::Move), json!({"kind": "move"}));
    refuses::<TransformMode>(r#"{"kind": "move", "count": 1}"#);
    assert_eq!(to_value(&ToolDisposition::Absorbed), json!("absorbed"));
    assert_eq!(to_value(&Retention::KeepSelected), json!("keep_selected"));
    assert_eq!(to_value(&Heal::Open), json!({"kind": "open"}));
}

/// §2.5: every `Option` field, a variant's included, is omitted for `None`
/// and refuses `null`, and every list, map, and set field is always written
/// and refuses omission.
#[test]
fn an_option_field_is_omitted_and_a_list_field_is_always_written() {
    assert_eq!(
        to_value(&Kept::<Dimension<bool>>::All { form: None }),
        json!({"kind": "all", "value": {}})
    );
    refuses::<Kept<Dimension<bool>>>(r#"{"kind": "all", "value": {"form": null}}"#);
    refuses::<Kept<Dimension<bool>>>(r#"{"kind": "all"}"#);
    let side = Kept::<Dimension<bool>>::Side {
        side: literal(true),
    };
    assert_eq!(
        to_value(&side),
        json!({"kind": "side", "value": {"side": {"kind": "literal", "value": true}}})
    );
    let empty = Transformation {
        steps: Vec::new(),
        resolved: None,
    };
    assert_eq!(serde_json::to_string(&empty).unwrap(), r#"{"steps":[]}"#);
    refuses::<Transformation>("{}");
    let message = refuses::<Transformation>(r#"{"steps": [], "resolved": null}"#);
    assert!(message.contains("resolved"), "{message}");
}

/// §6.5: a `Geometry` split tool holds its selection under `tools` and its
/// extension under `extend`, and a `Silhouette` tool is its direction.
#[test]
fn a_split_tool_states_its_extension_and_its_silhouette() {
    let geometry = to_value(&split_tools().remove(2));
    assert_eq!(geometry["kind"], json!("geometry"));
    assert_eq!(geometry["tools"], to_value(&faces(41)));
    assert_eq!(
        geometry["extend"],
        json!({"kind": "extended", "method": {"kind": "tangent"}})
    );
    assert_eq!(
        to_value(&ToolExtension::Extended { method: None }),
        json!({"kind": "extended"})
    );
    assert_eq!(to_value(&ToolExtension::Off), json!({"kind": "off"}));
    refuses::<ToolExtension>(r#"{"kind": "off", "method": {"kind": "tangent"}}"#);
    let bare = to_value(&faces(41));
    let mut old = bare.as_object().unwrap().clone();
    old.insert("kind".to_owned(), json!("geometry"));
    refuses::<SplitTool>(&serde_json::Value::Object(old).to_string());
    let silhouette = to_value(&split_tools().remove(3));
    let mut direction = to_value(&direction_construction());
    direction
        .as_object_mut()
        .unwrap()
        .insert("kind".to_owned(), json!("silhouette"));
    assert_eq!(silhouette, direction);
}

/// A combine's new component is omitted for `None`, refuses `null`, and
/// names an occurrence when stated.
#[test]
fn a_combine_states_its_new_component_only_when_it_makes_one() {
    let mut combine = combines().pop().unwrap();
    let value = to_value(&combine);
    assert!(value.get("new_component").is_none(), "{value}");
    combine.new_component = new_components().pop();
    let value = to_value(&combine);
    assert_eq!(
        value["new_component"],
        json!({"occurrence": "test:design:occurrence#35"})
    );
    let mut text = value.clone();
    text["new_component"] = serde_json::Value::Null;
    let message = refuses::<Combine>(&text.to_string());
    assert!(message.contains("new_component"), "{message}");
    let mut text = value;
    text["new_component"]["component"] = json!(1);
    refuses::<Combine>(&text.to_string());
}

/// A fixed-length array writes `null` for an unstated element and refuses
/// another length.
#[test]
fn fixed_length_arrays_hold_their_length() {
    let flex = flexes().pop().unwrap();
    let value = to_value(&flex);
    assert_eq!(value["trims"], json!([null, null]));
    let mut short = value;
    short["trims"] = json!([null]);
    refuses::<Flex>(&short.to_string());
    let step = TransformStep::Components {
        frame: None,
        delta: [
            literal(length(1.0)),
            literal(length(2.0)),
            literal(length(3.0)),
        ],
    };
    let mut value = to_value(&step);
    value["value"]["delta"].as_array_mut().unwrap().pop();
    refuses::<TransformStep>(&value.to_string());
}
