// SPDX-License-Identifier: Apache-2.0
//! Boolean operation of the first solid feature of a `PartDesign` body.

use crate::loss::FreecadLossCode;
use crate::test_support::test_archive::{archive_entries, assert_valid_document};
use crate::FcstdCodec;
use cadmpeg_ir::features::operation_settings::BooleanOp;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::fmt::Write;
use std::io::Cursor;

const BREP: &[u8] = b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 0
Curves 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 0
Triangulations 0
TShapes 1
Ve 0.001 0 0 0 0 0 1001000 *
+1 0 *";

fn body(name: &str, group: &[&str], base: Option<&str>) -> String {
    let mut links = String::new();
    for member in group {
        write!(links, r#"<Link value="{member}"/>"#).unwrap();
    }
    let base = base.map_or_else(String::new, |base| {
        format!(r#"<Property name="BaseFeature" type="App::PropertyLink"><Link value="{base}"/></Property>"#)
    });
    let count = 4 + usize::from(!base.is_empty());
    let tip = group.last().copied().unwrap_or_default();
    format!(
        r#"<Object name="{name}"><Properties Count="{count}"><Property name="Group" type="App::PropertyLinkList"><LinkList count="{}">{links}</LinkList></Property>{base}<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property><Property name="Tip" type="App::PropertyLink"><Link value="{tip}"/></Property><Property name="Shape" type="Part::PropertyPartShape"><Part file="{name}.Shape.brp"/></Property></Properties></Object>"#,
        group.len()
    )
}

fn feature(name: &str, base: Option<&str>) -> String {
    let base = base.map_or_else(String::new, |base| {
        format!(r#"<Property name="BaseFeature" type="App::PropertyLink"><Link value="{base}"/></Property>"#)
    });
    let count = 5 + usize::from(!base.is_empty());
    format!(
        r#"<Object name="{name}"><Properties Count="{count}"><Property name="Sketch" type="App::PropertyLink"><Link value=""/></Property><Property name="Length" type="App::PropertyLength"><Float value="5"/></Property><Property name="Midplane" type="App::PropertyBool"><Bool value="false"/></Property><Property name="Reversed" type="App::PropertyBool"><Bool value="false"/></Property>{base}<Property name="Shape" type="Part::PropertyPartShape"><Part file="{name}.Shape.brp"/></Property></Properties></Object>"#
    )
}

fn decode(objects: &[(&str, &str)], data: &[String]) -> cadmpeg_ir::DecodeResult {
    let mut declared = String::new();
    for (index, (kind, name)) in objects.iter().enumerate() {
        write!(
            declared,
            r#"<Object type="{kind}" name="{name}" id="{}"/>"#,
            index + 1
        )
        .unwrap();
    }
    let document = format!(
        r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="{count}">{declared}</Objects><ObjectData Count="{count}">{}</ObjectData></Document>"#,
        data.concat(),
        count = objects.len()
    );
    let names = objects
        .iter()
        .map(|(_, name)| format!("{name}.Shape.brp"))
        .collect::<Vec<_>>();
    let mut entries = vec![("Document.xml", document.as_bytes())];
    entries.extend(names.iter().map(|name| (name.as_str(), BREP)));
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&entries)),
            &DecodeOptions::default(),
        )
        .expect("decode body fixture");
    assert_valid_document(result.ir());
    result
}

fn operation<'a>(result: &'a cadmpeg_ir::DecodeResult, name: &str) -> &'a FeatureDefinition {
    result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("feature {name}"))
        .evaluation
        .definition()
}

fn extrude_op(result: &cadmpeg_ir::DecodeResult, name: &str) -> BooleanOp {
    match operation(result, name) {
        FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. })
        | FeatureDefinition::PostProcess {
            operation: FeatureOperation::Extrude { op, .. },
            ..
        } => *op,
        other => panic!("{name} is not an extrusion: {other:?}"),
    }
}

/// The bodies of the current state of the `PartDesign::Body` `name`: the
/// state at its `Tip`, which its own `Shape` holds (§5.5).
fn outputs(result: &cadmpeg_ir::DecodeResult, name: &str) -> Vec<String> {
    let object = format!("fcstd:native:object#{name}");
    result
        .ir()
        .design
        .scopes
        .iter()
        .flat_map(|scope| &scope.current)
        .find(|state| state.native_ref.as_deref() == Some(object.as_str()))
        .expect("current state")
        .bodies
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect()
}

#[test]
fn first_pads_of_independent_bodies_create_new_bodies() {
    let result = decode(
        &[
            ("PartDesign::Body", "Part"),
            ("PartDesign::Pad", "Pad"),
            ("PartDesign::Body", "Extra"),
            ("PartDesign::Pad", "ExtraPad"),
        ],
        &[
            body("Part", &["Pad"], None),
            feature("Pad", None),
            body("Extra", &["ExtraPad"], None),
            feature("ExtraPad", None),
        ],
    );
    assert_eq!(extrude_op(&result, "Pad"), BooleanOp::NewBody);
    assert_eq!(extrude_op(&result, "ExtraPad"), BooleanOp::NewBody);
    let first = outputs(&result, "Part");
    let second = outputs(&result, "Extra");
    assert_eq!(first.len(), 1);
    assert_eq!(second.len(), 1);
    assert!(first.iter().all(|body| !second.contains(body)));
    assert_body_members(&result, "Part", &["Pad"]);
    assert_body_members(&result, "Extra", &["ExtraPad"]);
}

#[test]
fn later_pad_joins_the_body_that_owns_it() {
    let result = decode(
        &[
            ("PartDesign::Body", "Part"),
            ("PartDesign::Pad", "Pad"),
            ("PartDesign::Body", "Extra"),
            ("PartDesign::Pad", "ExtraPad"),
            ("PartDesign::Pad", "Pad2"),
        ],
        &[
            body("Part", &["Pad", "Pad2"], None),
            feature("Pad", None),
            body("Extra", &["ExtraPad"], None),
            feature("ExtraPad", None),
            feature("Pad2", Some("Pad")),
        ],
    );
    assert_eq!(extrude_op(&result, "Pad"), BooleanOp::NewBody);
    assert_eq!(extrude_op(&result, "Pad2"), BooleanOp::Join);
    assert_eq!(extrude_op(&result, "ExtraPad"), BooleanOp::NewBody);
    assert_base_dependency(&result, "Pad2", "Pad");
    assert_body_members(&result, "Part", &["Pad", "Pad2"]);
    assert_body_members(&result, "Extra", &["ExtraPad"]);
    assert_ne!(outputs(&result, "Part"), outputs(&result, "Extra"));
}

#[test]
fn pad_of_a_body_with_a_base_feature_joins_it() {
    let result = decode(
        &[
            ("PartDesign::Body", "Part"),
            ("PartDesign::Pad", "Pad"),
            ("PartDesign::Body", "Extra"),
            ("PartDesign::Pad", "ExtraPad"),
        ],
        &[
            body("Part", &["Pad"], None),
            feature("Pad", None),
            body("Extra", &["ExtraPad"], Some("Pad")),
            feature("ExtraPad", Some("Pad")),
        ],
    );
    assert_eq!(extrude_op(&result, "Pad"), BooleanOp::NewBody);
    assert_eq!(extrude_op(&result, "ExtraPad"), BooleanOp::Join);
    assert_base_dependency(&result, "ExtraPad", "Pad");
}

#[test]
fn first_pocket_of_an_empty_body_keeps_its_cut_with_a_loss() {
    let result = decode(
        &[
            ("PartDesign::Body", "Part"),
            ("PartDesign::Pocket", "Pocket"),
        ],
        &[body("Part", &["Pocket"], None), feature("Pocket", None)],
    );
    assert_eq!(extrude_op(&result, "Pocket"), BooleanOp::Cut);
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "feature.removal-without-base"));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == FreecadLossCode::FeatureRemovalWithoutBase.note("").code));
}

#[test]
fn feature_outside_any_body_keeps_its_join() {
    let result = decode(&[("PartDesign::Pad", "Pad")], &[feature("Pad", None)]);
    assert_eq!(extrude_op(&result, "Pad"), BooleanOp::Join);
}

fn assert_base_dependency(result: &cadmpeg_ir::DecodeResult, name: &str, base: &str) {
    let features = &result.ir().model.features;
    let feature = features
        .iter()
        .find(|feature| feature.name.as_deref() == Some(name))
        .unwrap();
    let base = features
        .iter()
        .find(|feature| feature.name.as_deref() == Some(base))
        .unwrap();
    assert_eq!(
        feature.dependencies.iter().collect::<Vec<_>>(),
        vec![&base.id]
    );
}

fn assert_body_members(result: &cadmpeg_ir::DecodeResult, body: &str, members: &[&str]) {
    let FeatureOperation::TreeNode { children, .. } = operation(result, body).operation() else {
        panic!("body tree node");
    };
    let expected = members
        .iter()
        .map(|name| {
            &result
                .ir()
                .model
                .features
                .iter()
                .find(|feature| feature.name.as_deref() == Some(name))
                .unwrap()
                .id
        })
        .collect::<Vec<_>>();
    assert_eq!(children.iter().collect::<Vec<_>>(), expected);
    assert_eq!(children.active_child().as_ref(), expected.last().copied());
}

#[test]
fn empty_base_links_start_a_new_body() {
    let result = decode(
        &[("PartDesign::Body", "Part"), ("PartDesign::Pad", "Pad")],
        &[body("Part", &["Pad"], Some("")), feature("Pad", Some(""))],
    );
    assert_eq!(extrude_op(&result, "Pad"), BooleanOp::NewBody);
}

#[test]
fn a_features_base_link_keeps_its_join() {
    let result = decode(
        &[
            ("Part::Feature", "Base"),
            ("PartDesign::Body", "Part"),
            ("PartDesign::Pad", "Pad"),
        ],
        &[
            feature("Base", None),
            body("Part", &["Pad"], None),
            feature("Pad", Some("Base")),
        ],
    );
    assert_eq!(extrude_op(&result, "Pad"), BooleanOp::Join);
    assert_base_dependency(&result, "Pad", "Base");
}

#[test]
fn a_body_base_link_keeps_join_with_an_empty_feature_base_link() {
    let result = decode(
        &[
            ("Part::Feature", "Base"),
            ("PartDesign::Body", "Part"),
            ("PartDesign::Pad", "Pad"),
        ],
        &[
            feature("Base", None),
            body("Part", &["Pad"], Some("Base")),
            feature("Pad", Some("")),
        ],
    );
    assert_eq!(extrude_op(&result, "Pad"), BooleanOp::Join);
}

#[test]
fn a_stored_base_precedes_the_later_pad() {
    let result = decode(
        &[
            ("PartDesign::Body", "Part"),
            ("PartDesign::Feature", "Base"),
            ("PartDesign::Pad", "Pad"),
        ],
        &[
            body("Part", &["Base", "Pad"], None),
            feature("Base", None),
            feature("Pad", None),
        ],
    );
    assert_eq!(extrude_op(&result, "Pad"), BooleanOp::Join);
    assert_body_members(&result, "Part", &["Base", "Pad"]);
}

#[test]
fn a_later_pocket_cuts_its_own_body() {
    let result = decode(
        &[
            ("PartDesign::Body", "Part"),
            ("PartDesign::Pad", "Pad"),
            ("PartDesign::Body", "Extra"),
            ("PartDesign::Pad", "ExtraPad"),
            ("PartDesign::Pocket", "Pocket"),
        ],
        &[
            body("Part", &["Pad", "Pocket"], None),
            feature("Pad", None),
            body("Extra", &["ExtraPad"], None),
            feature("ExtraPad", None),
            feature("Pocket", Some("Pad")),
        ],
    );
    assert_eq!(extrude_op(&result, "Pocket"), BooleanOp::Cut);
    assert_base_dependency(&result, "Pocket", "Pad");
    assert_body_members(&result, "Part", &["Pad", "Pocket"]);
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| loss.code.local_code() != "feature.removal-without-base"));
}

fn box_feature(name: &str) -> String {
    format!(
        r#"<Object name="{name}"><Properties Count="5"><Property name="Length" type="App::PropertyLength"><Float value="5"/></Property><Property name="Width" type="App::PropertyLength"><Float value="5"/></Property><Property name="Height" type="App::PropertyLength"><Float value="5"/></Property><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property><Property name="Shape" type="Part::PropertyPartShape"><Part file="{name}.Shape.brp"/></Property></Properties></Object>"#
    )
}

#[test]
fn an_additive_primitive_starts_a_body_and_the_next_pad_joins() {
    for kind in ["PartDesign::AdditiveBox", "PartDesign::Box"] {
        let result = decode(
            &[
                ("PartDesign::Body", "Part"),
                (kind, "Box"),
                ("PartDesign::Pad", "Pad"),
            ],
            &[
                body("Part", &["Box", "Pad"], None),
                box_feature("Box"),
                feature("Pad", None),
            ],
        );
        assert!(matches!(
            operation(&result, "Box").operation(),
            FeatureOperation::Primitive {
                op: BooleanOp::NewBody,
                ..
            }
        ));
        assert_eq!(extrude_op(&result, "Pad"), BooleanOp::Join);
        assert_body_members(&result, "Part", &["Box", "Pad"]);
    }
}

#[test]
fn a_later_primitive_joins_its_body() {
    let result = decode(
        &[
            ("PartDesign::Body", "Part"),
            ("PartDesign::Pad", "Pad"),
            ("PartDesign::Box", "Box"),
        ],
        &[
            body("Part", &["Pad", "Box"], None),
            feature("Pad", None),
            box_feature("Box"),
        ],
    );
    assert!(matches!(
        operation(&result, "Box").operation(),
        FeatureOperation::Primitive {
            op: BooleanOp::Join,
            ..
        }
    ));
}

#[test]
fn first_subtractive_primitive_keeps_its_cut_with_a_loss() {
    let result = decode(
        &[
            ("PartDesign::Body", "Part"),
            ("PartDesign::SubtractiveBox", "Box"),
        ],
        &[body("Part", &["Box"], None), box_feature("Box")],
    );
    assert!(matches!(
        operation(&result, "Box").operation(),
        FeatureOperation::Primitive {
            op: BooleanOp::Cut,
            ..
        }
    ));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == FreecadLossCode::FeatureRemovalWithoutBase.note("").code));
}

#[test]
fn first_hole_keeps_its_hole_operation_with_a_loss() {
    let hole = r#"<Object name="Hole"><Properties Count="4"><Property name="Profile" type="App::PropertyLink"><Link value="Locations"/></Property><Property name="Diameter" type="App::PropertyLength"><Float value="6"/></Property><Property name="Depth" type="App::PropertyLength"><Float value="5"/></Property><Property name="DrillPointAngle" type="App::PropertyAngle"><Float value="118"/></Property></Properties></Object>"#;
    let result = decode(
        &[
            ("PartDesign::Body", "Part"),
            ("Sketcher::SketchObject", "Locations"),
            ("PartDesign::Hole", "Hole"),
        ],
        &[
            body("Part", &["Locations", "Hole"], None),
            String::from(r#"<Object name="Locations"><Properties Count="0"/></Object>"#),
            hole.to_owned(),
        ],
    );
    assert!(matches!(
        operation(&result, "Hole").operation(),
        FeatureOperation::Hole { .. }
    ));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == FreecadLossCode::FeatureRemovalWithoutBase.note("").code));
}
