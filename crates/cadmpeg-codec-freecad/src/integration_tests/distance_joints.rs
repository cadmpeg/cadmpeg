// SPDX-License-Identifier: Apache-2.0
//! Distance joints between elements of the decoded shapes of an authored
//! assembly, restated in the form the assembly solver gives them.

use cadmpeg_ir::design::construction::{FrameAxis, InputReference};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::names::NativeNameValue;
use cadmpeg_ir::design::ops::shared::{Alignment, Joint, JointConnector, JointKind, JointMotion};
use cadmpeg_ir::design::options::NativeOption;
use cadmpeg_ir::design::selection::{PickContext, Picks, StepRef};

use super::*;
use crate::loss::FreecadLossCode;
use crate::test_support::test_archive::rewrite_entry;

const PRODUCT_ASSEMBLY: &[u8] =
    include_bytes!("../../../../corpus/freecad_fcstd/fixtures/product_assembly.FCStd");

/// The authored assembly with its joint changed to a Distance joint of
/// distance `distance` between the references `first` and `second`, each the
/// `XLink` element of the reference. The joint detaches its first connector,
/// whose pick names the occurrence and no element.
fn distance_joint(first: &str, second: &str, distance: f64) -> Vec<u8> {
    rewrite_entry(PRODUCT_ASSEMBLY, "Document.xml", |data| {
        let text = std::str::from_utf8(data).expect("Document.xml is UTF-8");
        let start = text
            .find("<Object name=\"RevoluteJoint\">")
            .expect("joint record");
        let end = start + text[start..].find("</Object>").expect("joint end");
        let mut joint = text[start..end]
            .replacen(
                "<Enum value=\"Revolute\"/>",
                "<Enum value=\"Distance\"/>",
                1,
            )
            .replacen("Properties Count=\"19\"", "Properties Count=\"20\"", 1);
        for (index, reference) in [("1", first), ("2", second)] {
            let property = format!("<Property name=\"Reference{index}\"");
            let at = joint.find(&property).expect("reference property");
            let open = at + joint[at..].find("<XLink").expect("reference link");
            let close = open + joint[open..].find("</XLink>").expect("link end") + "</XLink>".len();
            joint.replace_range(open..close, reference);
        }
        let at = joint
            .find("<Property name=\"Reference1\"")
            .expect("reference property");
        joint.insert_str(
            at,
            &format!(
                "<Property name=\"Distance\" type=\"App::PropertyLength\">\
                 <Float value=\"{distance}\"/></Property>"
            ),
        );
        format!("{}{joint}{}", &text[..start], &text[end..]).into_bytes()
    })
}

fn link(object: &str, sub: &str) -> String {
    format!("<XLink file=\"\" name=\"{object}\" sub=\"{sub}\"/>")
}

struct Decoded {
    joint: Joint,
    losses: Vec<cadmpeg_ir::report::loss::LossNote>,
}

fn decode(first: &str, second: &str, distance: f64) -> Decoded {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(distance_joint(first, second, distance)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_valid_document(decoded.ir());
    let joint = decoded
        .ir()
        .design
        .features
        .iter()
        .find_map(|feature| match &feature.definition.operation {
            FeatureOperation::Joint(joint) if feature.id.as_str().contains("RevoluteJoint") => {
                Some(joint.as_ref().clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("{:?}", decoded.report().losses));
    Decoded {
        joint,
        losses: decoded.report().losses.clone(),
    }
}

/// The shape of connector `connector`, the element its pick names, and the
/// last occurrence of the pick's path.
fn connector(connector: &JointConnector) -> (&'static str, Option<String>, Option<String>) {
    let (shape, inputs) = match connector {
        JointConnector::Frame(construction) => ("frame", &construction.inputs),
        JointConnector::Plane(construction) => ("plane", &construction.inputs),
        JointConnector::Axis(construction) => ("axis", &construction.inputs),
        JointConnector::Point(construction) => ("point", &construction.inputs),
        JointConnector::Entity(selection) => panic!("{selection:?}"),
    };
    let [input] = inputs.as_slice() else {
        panic!("{inputs:?}");
    };
    let Some(InputReference::Picks(selection)) = &input.reference else {
        panic!("{input:?}");
    };
    let Picks::Named(members) = &selection.picks else {
        panic!("{selection:?}");
    };
    let [pick] = members.as_slice() else {
        panic!("{members:?}");
    };
    let element = pick.native.as_ref().map(|name| match &name.value {
        NativeNameValue::Text(text) => text.as_str().to_owned(),
        other => panic!("{other:?}"),
    });
    let occurrence = match &pick.context {
        Some(PickContext::Occurrence(path)) => path.0.last().map(|step| match &step.occurrence {
            StepRef::Occurrence(id) => id.as_str().to_owned(),
            StepRef::Native(name) => panic!("{name:?}"),
        }),
        None => None,
        Some(other) => panic!("{other:?}"),
    };
    (shape, element, occurrence)
}

fn instance(name: &str) -> String {
    format!("fcstd:model:occurrence#{name}:instance")
}

fn length(dimension: Option<&Dimension<cadmpeg_ir::scalar::Length>>) -> Option<f64> {
    match dimension {
        Some(Dimension::Literal(length)) => Some(length.get()),
        _ => None,
    }
}

fn native(joint: &Joint, name: &str) -> Option<NativeOption> {
    joint
        .native_options
        .iter()
        .find(|(key, _)| key.as_str() == format!("fcstd:{name}"))
        .map(|(_, option)| option.clone())
}

fn charged(decoded: &Decoded, code: FreecadLossCode) -> bool {
    let code = code.note("").code;
    decoded.losses.iter().any(|loss| loss.code == code)
}

#[test]
fn two_planar_faces_are_a_planar_joint_offset_by_the_distance() {
    let decoded = decode(
        &link("BracketOccurrence", "Face1"),
        &link("PinOccurrence", "Face2"),
        4.0,
    );
    let JointKind::Motion {
        motion: JointMotion::Planar { axis: FrameAxis::Z },
        offset,
        ..
    } = &decoded.joint.kind
    else {
        panic!("{:?}", decoded.joint.kind);
    };
    assert_eq!(length(offset.z.as_ref()), Some(4.0));
    assert_eq!(decoded.joint.alignment, Some(Alignment::Nearest));
    assert_eq!(
        decoded.joint.connectors.each_ref().map(connector),
        [
            ("frame", None, Some(instance("BracketOccurrence"))),
            (
                "frame",
                Some("Face2".to_owned()),
                Some(instance("PinOccurrence"))
            ),
        ]
    );
    assert_eq!(native(&decoded.joint, "Distance"), None);
}

#[test]
fn a_line_and_a_circle_hold_parallel_axes_apart_by_the_distance_and_radius() {
    // The pin's first edge is a circle of radius 3.
    let decoded = decode(
        &link("BracketOccurrence", "Edge1"),
        &link("PinOccurrence", "Edge1"),
        2.0,
    );
    let JointKind::Distance { distance, .. } = &decoded.joint.kind else {
        panic!("{:?}", decoded.joint.kind);
    };
    assert_eq!(length(distance.as_ref()), Some(5.0));
    assert_eq!(decoded.joint.alignment, Some(Alignment::Nearest));
    assert_eq!(
        decoded.joint.connectors.each_ref().map(connector),
        [
            ("axis", None, Some(instance("BracketOccurrence"))),
            (
                "axis",
                Some("Edge1".to_owned()),
                Some(instance("PinOccurrence"))
            ),
        ]
    );
    assert_eq!(
        native(&decoded.joint, "parallelAxes"),
        Some(NativeOption::Bool(Dimension::Literal(true)))
    );
}

#[test]
fn a_cylinder_and_a_plane_put_the_plane_first_and_keep_the_axis_in_it() {
    // The pin's first face is a cylinder of radius 3. The swap exchanges the
    // references; the first slot stays detached.
    let decoded = decode(
        &link("PinOccurrence", "Face1"),
        &link("BracketOccurrence", "Face1"),
        1.0,
    );
    let JointKind::Coincident { offset } = &decoded.joint.kind else {
        panic!("{:?}", decoded.joint.kind);
    };
    assert_eq!(length(offset.as_ref()), Some(4.0));
    assert_eq!(
        decoded.joint.connectors.each_ref().map(connector),
        [
            ("plane", None, Some(instance("BracketOccurrence"))),
            (
                "axis",
                Some("Face1".to_owned()),
                Some(instance("PinOccurrence"))
            ),
        ]
    );
}

#[test]
fn two_vertices_at_no_distance_coincide() {
    let decoded = decode(
        &link("BracketOccurrence", "Vertex1"),
        &link("PinOccurrence", "Vertex1"),
        0.0,
    );
    assert_eq!(decoded.joint.kind, JointKind::Coincident { offset: None });
    assert_eq!(
        decoded
            .joint
            .connectors
            .each_ref()
            .map(|item| connector(item).0),
        ["point", "point"]
    );
}

#[test]
fn an_axis_separation_below_the_radius_is_its_magnitude() {
    // The pin's first edge is a circle of radius 3; the solver squares the
    // separation of the axes.
    let decoded = decode(
        &link("BracketOccurrence", "Edge1"),
        &link("PinOccurrence", "Edge1"),
        -5.0,
    );
    let JointKind::Distance { distance, .. } = &decoded.joint.kind else {
        panic!("{:?}", decoded.joint.kind);
    };
    assert_eq!(length(distance.as_ref()), Some(2.0));
    assert_eq!(native(&decoded.joint, "Distance"), None);
    // The stored signed distance differs from the stated one: it is a
    // source setting.
    assert!(matches!(
        decoded
            .joint
            .source_settings
            .iter()
            .find(|(key, _)| key.as_str() == "fcstd:Distance")
            .map(|(_, option)| option),
        Some(NativeOption::Length(Dimension::Literal(length))) if length.get() == -5.0
    ));
    assert!(!charged(
        &decoded,
        FreecadLossCode::FeatureJointDistanceUnsatisfiable
    ));
}

#[test]
fn a_swapped_attached_slot_takes_the_frame_of_its_own_offset() {
    // The joint stores the first reference's frame (20, 6, 3) built with
    // offset +0.5 along z; the swap moves that reference to the attached
    // second slot, whose offset is -0.5 along z.
    let decoded = decode(
        &link("PinOccurrence", "Face1"),
        &link("BracketOccurrence", "Face1"),
        1.0,
    );
    let JointConnector::Axis(second) = &decoded.joint.connectors[1] else {
        panic!("{:?}", decoded.joint.connectors[1]);
    };
    let origin = second.resolved.as_ref().unwrap().origin.get();
    assert_eq!([origin.x, origin.y, origin.z], [20.0, 6.0, 2.0]);
}

#[test]
fn an_element_of_another_document_stays_native() {
    let decoded = decode(
        "<XLink file=\"Other.FCStd\" name=\"Part\" sub=\"Face1\"/>",
        &link("PinOccurrence", "Face2"),
        3.0,
    );
    assert_eq!(
        decoded.joint.kind,
        JointKind::Distance {
            distance: None,
            limits: None
        }
    );
    assert!(matches!(
        native(&decoded.joint, "Distance"),
        Some(NativeOption::Length(Dimension::Literal(length))) if length.get() == 3.0
    ));
    assert!(charged(
        &decoded,
        FreecadLossCode::FeatureJointDistanceUnresolved
    ));
}
