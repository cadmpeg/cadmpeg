// SPDX-License-Identifier: Apache-2.0
//! Projected attributes retain typed native values without outward links.

use super::*;

fn assert_inward_links(ir: &cadmpeg_ir::CadIr) {
    let namespace = ir.native.namespace("iges").expect("native namespace");
    for attribute in &ir.model.attributes {
        let id = attribute
            .native_ref
            .as_deref()
            .expect("projected attribute inward link");
        assert!(namespace
            .arenas()
            .values()
            .flatten()
            .any(|record| record.id() == id));
    }
    for record in namespace.arenas().values().flatten() {
        assert_eq!(record.field("neutral_values"), None);
        assert_eq!(record.field("neutral_label"), None);
    }
}

#[test]
fn native_attribute_values_and_directory_labels_survive_projection_without_backlinks() {
    let mut point = entity(116, 0, "116,1,2,3,0,0,1,3;");
    point.label = "SOURCE".into();
    let bytes = owned_test_file(&[
        point,
        entity(406, 5, "406,5,2.5,1,0,2,3.75;"),
        entity(406, 27, "406,6,4HNAME,2,1,7,2,8.25;"),
    ]);
    let decoded = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("typed property source");
    let ir = decoded.ir();
    assert_inward_links(ir);
    let properties = &ir
        .native
        .namespace("iges")
        .expect("native namespace")
        .arenas()["properties"];
    assert_eq!(properties.len(), 2);
    assert_eq!(properties[0].field("width"), Some(serde_json::json!(2.5)));
    assert_eq!(
        properties[0].field("extension"),
        Some(serde_json::json!(3.75))
    );
    assert_eq!(properties[0].field("cornering"), Some(serde_json::json!(1)));
    let values = properties[1].field("values").expect("typed generic values");
    assert_eq!(values[0]["value"]["value"], 7);
    assert_eq!(values[1]["value"]["value"], 8.25);
    let directory = &ir
        .native
        .namespace("iges")
        .expect("native namespace")
        .arenas()["entities"][0];
    assert_eq!(
        crate::test_support::native_bytes(&directory.field("label").expect("directory label")),
        b"  SOURCE"
    );
}

#[test]
fn defaults_and_rows_link_inward_without_losing_the_native_tokens() {
    let bytes = owned_test_file_with_structures(
        &[
            entity(322, 1, "322,4HATTR,0,1,1,3,1,3HVAL;"),
            entity(322, 0, "322,4HMETA,1,1,10,3,1;"),
            entity(422, 1, "422,1,4HITEM;"),
        ],
        &[(5, -3)],
    );
    let decoded = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("table source");
    let ir = decoded.ir();
    assert_inward_links(ir);
    let namespace = ir.native.namespace("iges").expect("native namespace");
    let defaults = namespace.arenas()["attribute_table_definitions"][0]
        .field("attributes")
        .expect("default descriptors");
    assert_eq!(
        crate::test_support::native_bytes(&defaults[0]["values"][0]["value"]["value"]),
        b"VAL"
    );
    let rows = namespace.arenas()["attribute_table_instances"][0]
        .field("rows")
        .expect("row tokens");
    assert_eq!(
        crate::test_support::native_bytes(&rows[0][0]["value"]),
        b"ITEM"
    );
}

#[test]
fn matching_name_and_directory_label_keep_one_neutral_value_and_both_native_facts() {
    let mut point = entity(116, 0, "116,1,2,3,0,0,1,3;");
    point.label = "SAME".into();
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[point, entity(406, 15, "406,1,4HSAME;")])),
            &DecodeOptions::default(),
        )
        .expect("matching independent source fields");
    let ir = decoded.ir();
    assert_inward_links(ir);
    let names: Vec<_> = ir
        .model
        .attributes
        .iter()
        .filter(|attribute| attribute.name == "name")
        .collect();
    assert_eq!(names.len(), 1);
    assert_eq!(
        names[0].values,
        [Some(AttributeValue::String("SAME".into()))]
    );
    let namespace = ir.native.namespace("iges").expect("native namespace");
    assert_eq!(
        crate::test_support::native_bytes(
            &namespace.arenas()["properties"][0]
                .field("value")
                .expect("native name")
        ),
        b"SAME"
    );
    assert_eq!(
        crate::test_support::native_bytes(
            &namespace.arenas()["entities"][0]
                .field("label")
                .expect("native label")
        ),
        b"    SAME"
    );
}
