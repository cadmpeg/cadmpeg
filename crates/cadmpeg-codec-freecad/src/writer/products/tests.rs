// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_ir::products::{Occurrence, ProductDefinition};
use serde_json::json;

fn definition(name: &str) -> ProductDefinition {
    serde_json::from_value(json!({"id":format!("authored:model:product_definition#{name}"),"kind":"part","source_name":name,"native_ref":format!("fcstd:native:object#{name}")})).expect("definition")
}

fn occurrence(
    name: &str,
    definition: &ProductDefinition,
    parent: Option<&Occurrence>,
) -> Occurrence {
    serde_json::from_value(json!({"id":format!("authored:model:occurrence#{name}"),"prototype":{"scope":"local","definition":definition.id},"parent":parent.map_or(json!({"kind":"root"}),|parent|json!({"kind":"occurrence","occurrence":parent.id})),"ordinal":0,"transform":cadmpeg_ir::transform::Transform::identity(),"name":name})).expect("occurrence")
}

#[test]
fn missing_link_component_keeps_other_link_state() {
    let component =
        cadmpeg_ir::ids::ProductDefinitionId::mint("authored:model:product#missing").unwrap();
    let state =
        cadmpeg_ir::products::LinkState::new(Vec::new(), Some(component.clone()), Some(true), None)
            .unwrap();
    let mut object = Object::new("Link".into(), "App::Link", "Link");
    let mut losses = Vec::new();
    link_state(&mut object, &state, &HashMap::new(), &mut losses);
    assert!(object.properties["LinkClaimChild"].contains("value=\"1\""));
    assert!(object.properties["ElementList"].contains("count=\"0\""));
    assert!(losses
        .iter()
        .any(|loss| loss.message.contains(component.as_str())));
}

#[test]
fn nested_membership_accumulates_siblings_in_neutral_order() {
    let mut ir = CadIr::empty();
    let definition = definition("Prototype");
    let parent = occurrence("Parent", &definition, None);
    let mut first = occurrence("First", &definition, Some(&parent));
    first.ordinal = 1;
    let mut second = occurrence("Second", &definition, Some(&parent));
    second.ordinal = 0;
    ir.model.product_definitions.push(definition.clone());
    ir.model
        .occurrences
        .extend([parent.clone(), first.clone(), second.clone()]);
    let mut names = HashMap::from([
        (definition.id.as_str().to_owned(), "Prototype".to_owned()),
        (parent.id.as_str().to_owned(), "Parent".to_owned()),
        (first.id.as_str().to_owned(), "First".to_owned()),
        (second.id.as_str().to_owned(), "Second".to_owned()),
    ]);
    let mut objects = Vec::new();
    let mut losses = Vec::new();
    crate::test_support::with_service_context(&[], |ctx| {
        write(
            ctx,
            &ir,
            &mut objects,
            &mut names,
            &HashMap::new(),
            &mut losses,
        )
    })
    .expect("products");
    let group = &objects
        .iter()
        .find(|object| object.name == "Parent")
        .expect("parent")
        .properties["Group"];
    let xml = roxmltree::Document::parse(group).expect("group XML");
    let members: Vec<_> = xml
        .descendants()
        .filter(|node| node.has_tag_name("Link"))
        .filter_map(|node| node.attribute("value"))
        .collect();
    assert_eq!(members, ["Second", "First"]);
    assert_eq!(
        objects
            .iter()
            .filter(|object| object.name == "Parent")
            .count(),
        1
    );
}

#[test]
fn container_occurrence_reuses_its_definition_object() {
    let mut ir = CadIr::empty();
    let definition = definition("Assembly");
    let mut instance = occurrence("Assembly", &definition, None);
    instance.native_ref = definition.native_ref.clone();
    ir.model.product_definitions.push(definition.clone());
    ir.model.occurrences.push(instance.clone());
    let mut names = HashMap::from([
        (definition.id.as_str().to_owned(), "Assembly".to_owned()),
        (instance.id.as_str().to_owned(), "Assembly".to_owned()),
    ]);
    let mut objects = Vec::new();
    let mut losses = Vec::new();
    crate::test_support::with_service_context(&[], |ctx| {
        write(
            ctx,
            &ir,
            &mut objects,
            &mut names,
            &HashMap::new(),
            &mut losses,
        )
    })
    .expect("products");
    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0].kind, "App::Part");
    assert!(!objects[0].properties.contains_key("LinkedObject"));
}

#[test]
fn unresolved_external_prototype_is_a_persisted_link_state() {
    let mut ir = CadIr::empty();
    let definition = definition("Prototype");
    let mut instance = occurrence("Missing", &definition, None);
    instance.prototype = PrototypeReference::External {
        document: ExternalDocument::path("missing.FCStd"),
        object: Some("Part".into()),
    };
    ir.model.occurrences.push(instance.clone());
    let mut names = HashMap::from([(instance.id.as_str().to_owned(), "Missing".to_owned())]);
    let mut objects = Vec::new();
    let mut losses = Vec::new();
    crate::test_support::with_service_context(&[], |ctx| {
        write(
            ctx,
            &ir,
            &mut objects,
            &mut names,
            &HashMap::new(),
            &mut losses,
        )
    })
    .expect("products");
    let document =
        roxmltree::Document::parse(&objects[0].properties["LinkedObject"]).expect("link XML");
    let link = document
        .descendants()
        .find(|node| node.has_tag_name("XLink"))
        .expect("XLink");
    assert_eq!(link.attribute("file"), Some("missing.FCStd"));
    assert_eq!(link.attribute("name"), Some("Part"));
    assert!(losses.is_empty());
}

#[test]
fn nested_link_group_children_decode_with_their_instance_parent() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::{Cursor, Write};
    let mut ir = CadIr::empty();
    let definition = definition("Prototype");
    let parent = occurrence("Parent", &definition, None);
    let first = occurrence("First", &definition, Some(&parent));
    let second = occurrence("Second", &definition, Some(&parent));
    ir.model.product_definitions.push(definition);
    ir.model.occurrences.extend([parent, first, second]);
    let mut names = crate::writer::names::prepare(&ir, &[], &[]);
    let mut objects = Vec::new();
    let mut losses = Vec::new();
    crate::test_support::with_service_context(&[], |ctx| {
        write(
            ctx,
            &ir,
            &mut objects,
            &mut names,
            &HashMap::new(),
            &mut losses,
        )
    })
    .expect("product graph");
    let document = xml::document(&objects, &indexmap::IndexMap::new(), "4", None, "1");
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    archive
        .start_file("Document.xml", zip::write::SimpleFileOptions::default())
        .expect("entry");
    archive.write_all(document.as_bytes()).expect("XML");
    let bytes = archive.finish().expect("archive").into_inner();
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("authored product decode");
    let parent_name = names[ir.model.occurrences[0].id.as_str()].clone();
    let parent = decoded
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| {
            occurrence
                .native_ref
                .as_ref()
                .is_some_and(|native| native.ends_with(&format!("#{parent_name}")))
        })
        .expect("parent occurrence");
    let children:Vec<_>=decoded.ir().model.occurrences.iter().filter(|occurrence|matches!(&occurrence.parent,OccurrenceParent::Occurrence {occurrence:owner} if *owner==parent.id)).collect();
    assert_eq!(
        children.len(),
        2,
        "every nested member has the actual App::Link instance as parent"
    );
}

#[test]
fn missing_occurrence_pose_keeps_the_object_and_joint_without_placement() {
    let mut ir = CadIr::empty();
    let definition = definition("Prototype");
    let mut unknown = occurrence("Unknown", &definition, None);
    unknown.transform = None;
    let known = occurrence("Known", &definition, None);
    let joint_id = cadmpeg_ir::products::JointId::mint("authored:model:joint#Unknown").unwrap();
    ir.model
        .assembly_joints
        .push(cadmpeg_ir::products::AssemblyJoint::grounded(
            joint_id.clone(),
            cadmpeg_ir::products::JointConnector {
                operand: cadmpeg_ir::products::JointOperand::occurrence(
                    unknown.id.clone(),
                    "Body",
                    vec![],
                ),
                frame: cadmpeg_ir::transform::Transform::identity(),
                detached: false,
            },
            None,
        ));
    ir.model.product_definitions.push(definition.clone());
    ir.model
        .occurrences
        .extend([unknown.clone(), known.clone()]);
    let mut objects = Vec::new();
    let mut names = HashMap::from([(definition.id.as_str().to_owned(), "Prototype".to_owned())]);
    let mut losses = Vec::new();
    crate::test_support::with_service_context(&[], |ctx| {
        write(
            ctx,
            &ir,
            &mut objects,
            &mut names,
            &HashMap::new(),
            &mut losses,
        )
    })
    .unwrap();
    let unknown_name = names.get(unknown.id.as_str()).expect("unknown-pose object");
    let unknown_object = objects
        .iter()
        .find(|object| object.name == *unknown_name)
        .unwrap();
    assert!(!unknown_object.properties.contains_key("Placement"));
    assert!(names.contains_key(joint_id.as_str()));
    let known_name = names.get(known.id.as_str()).expect("known sibling object");
    let object = objects
        .iter()
        .find(|object| object.name == *known_name)
        .unwrap();
    assert!(object.properties.contains_key("Placement"));
    assert!(losses
        .iter()
        .all(|loss| loss.code.local_code() != "occurrence.not-transferred"));
    assert!(losses
        .iter()
        .all(|loss| loss.code.local_code() != "joint.operand-not-transferred"));
    assert_eq!(ir.model.occurrences[0].transform, None);
    assert_eq!(
        ir.model.occurrences[1].transform,
        Some(cadmpeg_ir::transform::Transform::identity())
    );
}

#[test]
fn unknown_ancestor_keeps_descendants_and_local_joint_frames() {
    use cadmpeg_ir::products::{AssemblyJoint, JointConnector, JointId, JointOperand};
    use cadmpeg_ir::transform::Transform;
    let mut ir = CadIr::empty();
    let leaf = definition("Leaf");
    let definition = definition("Prototype");
    let translated = |x| {
        Transform::affine([
            [1.0, 0.0, 0.0, x],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .unwrap()
    };
    let mut unknown = occurrence("Unknown", &definition, None);
    unknown.transform = None;
    let child = occurrence("Child", &leaf, Some(&unknown));
    let grandchild = occurrence("Grandchild", &leaf, Some(&child));
    let mut known = occurrence("Known", &definition, None);
    known.ordinal = 1;
    known.transform = Some(translated(10.0));
    let mut known_child = occurrence("KnownChild", &leaf, Some(&known));
    known_child.transform = Some(translated(3.0));
    ir.model
        .product_definitions
        .extend([definition.clone(), leaf.clone()]);
    ir.model.occurrences.extend([
        unknown.clone(),
        child.clone(),
        grandchild.clone(),
        known.clone(),
        known_child.clone(),
    ]);
    let joint_id = JointId::mint("authored:model:joint#Child").unwrap();
    ir.model.assembly_joints.push(AssemblyJoint::grounded(
        joint_id.clone(),
        JointConnector {
            operand: JointOperand::occurrence(child.id.clone(), "Body", vec![]),
            frame: Transform::identity(),
            detached: false,
        },
        None,
    ));
    let mut objects = Vec::new();
    let mut names = HashMap::from([
        (definition.id.as_str().to_owned(), "Prototype".to_owned()),
        (leaf.id.as_str().to_owned(), "Leaf".to_owned()),
    ]);
    let mut losses = Vec::new();
    crate::test_support::with_service_context(&[], |ctx| {
        write(
            ctx,
            &ir,
            &mut objects,
            &mut names,
            &HashMap::new(),
            &mut losses,
        )
    })
    .unwrap();
    for row in [&unknown, &child, &grandchild] {
        assert!(names.contains_key(row.id.as_str()));
    }
    let root = objects
        .iter()
        .find(|object| Some(&object.name) == names.get(unknown.id.as_str()))
        .unwrap();
    assert!(!root.properties.contains_key("Placement"));
    assert!(root.properties["Group"].contains(names.get(child.id.as_str()).unwrap()));
    let child_object = objects
        .iter()
        .find(|object| Some(&object.name) == names.get(child.id.as_str()))
        .unwrap();
    assert!(child_object.properties["Group"].contains(names.get(grandchild.id.as_str()).unwrap()));
    assert!(losses
        .iter()
        .all(|loss| loss.code.local_code() != "occurrence.not-transferred"));
    for (row, x) in [(&known, "10"), (&known_child, "3")] {
        let object = objects
            .iter()
            .find(|object| Some(&object.name) == names.get(row.id.as_str()))
            .unwrap();
        let document = roxmltree::Document::parse(&object.properties["Placement"]).unwrap();
        assert_eq!(
            document
                .descendants()
                .find(|node| node.has_tag_name("PropertyPlacement"))
                .unwrap()
                .attribute("Px"),
            Some(x)
        );
    }
    assert!(names.contains_key(joint_id.as_str()));
    assert!(losses
        .iter()
        .all(|loss| loss.code.local_code() != "joint.operand-not-transferred"));
    assert_eq!(
        ir.model.occurrences[1].transform,
        Some(Transform::identity())
    );
}

mod object_dependencies;

#[test]
fn occurrence_with_missing_product_links_does_not_refuse_other_occurrences() {
    use crate::FcstdCodec;
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    use cadmpeg_ir::{Codec, DecodeOptions};
    use std::io::Cursor;
    let mut ir = cadmpeg_ir::CadIr::empty();
    let definition: cadmpeg_ir::products::ProductDefinition =
        serde_json::from_value(serde_json::json!({
            "id": "authored:model:product#prototype", "kind": "part", "source_name": "Prototype"
        }))
        .unwrap();
    for (name, parent, prototype) in [
        (
            "MissingPrototype",
            serde_json::json!({"kind":"root"}),
            serde_json::json!({"scope":"local","definition":"authored:model:product#missing"}),
        ),
        (
            "MissingParent",
            serde_json::json!({"kind":"occurrence","occurrence":"authored:model:occurrence#missing"}),
            serde_json::json!({"scope":"local","definition":definition.id}),
        ),
    ] {
        let mut occurrence: cadmpeg_ir::products::Occurrence = serde_json::from_value(serde_json::json!({
            "id":format!("authored:model:occurrence#{name}"),"name":name,"ordinal":0,"parent":parent,"prototype":prototype
        })).unwrap();
        if name == "MissingPrototype" {
            occurrence.linked_prototype = Some(cadmpeg_ir::transform::Transform::identity());
        }
        ir.model.occurrences.push(occurrence);
    }
    ir.model.product_definitions.push(definition);
    let plan = FcstdCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    for id in [
        "authored:model:occurrence#MissingPrototype",
        "authored:model:occurrence#MissingParent",
    ] {
        assert!(plan
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains(id)));
    }
    assert!(plan.report().losses.iter().any(|loss| loss.code
        == crate::loss::FreecadLossCode::ProductLinkedPlacementFolded
            .note("")
            .code));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.occurrences.len(), 2);
}
