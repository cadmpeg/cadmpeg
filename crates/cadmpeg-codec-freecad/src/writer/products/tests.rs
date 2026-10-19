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
    serde_json::from_value(json!({"id":format!("authored:model:occurrence#{name}"),"prototype":{"scope":"local","definition":definition.id},"parent":parent.map_or(json!({"kind":"root"}),|parent|json!({"kind":"occurrence","occurrence":parent.id})),"ordinal":0,"transform":cadmpeg_ir::transform::Transform::identity(),"scale":[1,1,1],"name":name})).expect("occurrence")
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
    write(&ir, &mut objects, &mut names, &HashMap::new(), &mut losses).expect("products");
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
    write(&ir, &mut objects, &mut names, &HashMap::new(), &mut losses).expect("products");
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
    write(&ir, &mut objects, &mut names, &HashMap::new(), &mut losses).expect("products");
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
    write(&ir, &mut objects, &mut names, &HashMap::new(), &mut losses).expect("product graph");
    let document = xml::document(&objects, &BTreeMap::new(), "4");
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
