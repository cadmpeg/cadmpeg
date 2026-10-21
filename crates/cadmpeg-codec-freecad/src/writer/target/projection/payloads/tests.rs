// SPDX-License-Identifier: Apache-2.0
//! Current typed payload ownership, independent of retained source evidence.

use super::*;
use crate::native::{LinkArray, ProductNode};
use crate::test_support::test_archive::CORE_DESIGN_PRODUCT;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

struct Graph {
    properties: Vec<PropertyRecord>,
    shapes: Vec<ShapePayloadRecord>,
    maps: Vec<ElementMapRecord>,
    tables: Vec<StringTableRecord>,
    products: Vec<ProductNodeRecord>,
    entries: Vec<crate::native::EntryRecord>,
}

impl Graph {
    fn load() -> Self {
        let decoded = crate::FcstdCodec
            .decode(
                &mut Cursor::new(CORE_DESIGN_PRODUCT),
                &DecodeOptions::default(),
            )
            .unwrap();
        let namespace = decoded.ir().native.namespace("fcstd").unwrap();
        Self {
            properties: namespace.arena_as("properties").unwrap(),
            shapes: namespace.arena_as("shape_payloads").unwrap(),
            maps: namespace.arena_as("element_maps").unwrap(),
            tables: namespace.arena_as("string_tables").unwrap(),
            products: namespace.arena_as("product_nodes").unwrap(),
            entries: namespace.arena_as("entries").unwrap(),
        }
    }

    fn xml(&self, property: &PropertyRecord) -> Option<String> {
        let entries = entries(
            property,
            &self.shapes,
            &self.maps,
            &self.tables,
            &self.products,
            &self.entries,
        );
        crate::writer::xml::native_projection(property, &entries).unwrap()
    }
}

#[test]
fn typed_shape_map_and_array_references_project_without_source_xml() {
    let graph = Graph::load();
    let mut families = BTreeSet::new();
    for property in graph
        .properties
        .iter()
        .filter(|property| !property.side_entries().is_empty())
    {
        families.insert(property.type_name.as_str());
        assert!(
            crate::writer::xml::native(property).unwrap().is_none(),
            "synthesis must still regenerate {}",
            property.id
        );
        let expected = graph
            .xml(property)
            .unwrap_or_else(|| panic!("typed payload for {}", property.id));
        let mut current = property.clone();
        current.xml =
            crate::native::RetainedXml::from_text("deliberately unusable source XML".into(), 0)
                .unwrap();
        assert_eq!(graph.xml(&current).as_deref(), Some(expected.as_str()));
    }
    assert_eq!(
        families,
        BTreeSet::from([
            "Part::PropertyPartShape",
            "App::PropertyPlacementList",
            "App::PropertyVectorList"
        ])
    );
}

#[test]
fn a_filename_does_not_replace_the_exact_typed_owner_or_payload() {
    let mut graph = Graph::load();
    let shape = graph
        .properties
        .iter()
        .find(|property| property.id.ends_with("#Cut:Shape"))
        .unwrap()
        .clone();
    assert!(graph.xml(&shape).is_some());
    let archive_entries = graph.entries.clone();
    graph.entries.clear();
    assert!(
        graph.xml(&shape).is_none(),
        "typed payloads must also own their current archive entry"
    );
    graph.entries = archive_entries;
    let index = graph
        .shapes
        .iter()
        .position(|payload| payload.property == shape.id)
        .unwrap();
    graph.shapes[index].property = "fcstd:native:property#Other:Shape".into();
    assert!(
        graph.xml(&shape).is_none(),
        "a different shape property cannot own the payload"
    );
    graph.shapes[index].property = shape.id.clone();
    graph.maps.clear();
    assert!(
        graph.xml(&shape).is_none(),
        "the shape payload cannot stand in for its element-map payload"
    );

    let mut graph = Graph::load();
    let array = graph
        .properties
        .iter()
        .find(|property| property.name == "PlacementList")
        .unwrap()
        .clone();
    assert!(graph.xml(&array).is_some());
    let index = graph
        .products
        .iter()
        .position(|product| product.object == array.owner)
        .unwrap();
    graph.products[index].object = "fcstd:native:object#Other".into();
    assert!(
        graph.xml(&array).is_none(),
        "a different occurrence cannot own this array"
    );
    graph.products[index].object = array.owner.clone();
    if let ProductNode::Occurrence(node) = &mut graph.products[index].node {
        node.array =
            LinkArray::try_new(Some(3), Vec::new(), Vec::new(), Vec::new(), Vec::new()).unwrap();
    } else {
        panic!("array owner is an occurrence");
    }
    assert!(
        graph.xml(&array).is_none(),
        "the filename cannot stand in for absent typed transforms"
    );
}

#[test]
fn every_current_payload_reference_must_match_and_python_stays_guarded() {
    let graph = Graph::load();
    for name in ["Shape", "PlacementList", "ScaleList"] {
        let mut property = graph
            .properties
            .iter()
            .find(|property| property.name == name && !property.side_entries().is_empty())
            .unwrap()
            .clone();
        property
            .values_mut()
            .unwrap()
            .iter_mut()
            .find(|value| value.attributes.contains_key("file"))
            .unwrap()
            .attributes
            .insert("file".into(), "unowned.bin".into());
        assert!(
            graph.xml(&property).is_none(),
            "{name}: a root filename without its typed payload owner must decline"
        );
    }
    let mut opaque = graph
        .properties
        .iter()
        .find(|property| !property.side_entries().is_empty())
        .unwrap()
        .clone();
    opaque.type_name = "App::PropertyPythonObject".into();
    let filenames = opaque.side_entries().iter().cloned().collect();
    assert!(crate::writer::xml::native_projection(&opaque, &filenames)
        .unwrap()
        .is_none());
}

#[test]
fn string_table_references_require_readable_entries_and_the_exact_property() {
    let mut graph = Graph::load();
    let mut property = graph
        .properties
        .iter()
        .find(|property| property.id.ends_with("#Box:Shape"))
        .unwrap()
        .clone();
    if let crate::native::PropertyBody::Persisted {
        values,
        side_entries,
        ..
    } = &mut property.body
    {
        values.push(crate::native::ValueRecord {
            tag: "StringHasher".into(),
            order: values.len(),
            parent: None,
            attributes: std::collections::BTreeMap::from([("file".into(), "table.txt".into())]),
            text: None,
            decoded_json: None,
            raw_xml: String::new(),
        });
        side_entries.push("table.txt".into());
    }
    let table = |owner, entries| {
        StringTableRecord::try_new(
            1,
            Some(owner),
            Some(true),
            Some(0),
            Some("table.txt".into()),
            entries,
        )
        .unwrap()
    };
    graph.tables = vec![table(property.id.clone(), Some(Vec::new()))];
    graph.entries.push(
        crate::native::EntryRecord::new(
            crate::native::native_id("entry", "table.txt"),
            "table.txt".into(),
            cadmpeg_core::container::ContainerRole::Metadata,
            vec![property.id.clone()],
            Vec::new(),
        )
        .unwrap(),
    );
    assert!(graph.xml(&property).is_some());
    graph.tables = vec![table(
        "fcstd:native:property#Other:Shape".into(),
        Some(Vec::new()),
    )];
    assert!(graph.xml(&property).is_none());
    graph.tables = vec![table(property.id.clone(), None)];
    assert!(
        graph.xml(&property).is_none(),
        "a source-only string table is not a typed payload"
    );
}
