// SPDX-License-Identifier: Apache-2.0

use crate::test_support::test_owned::{
    owned_test_file, owned_test_file_with_structures, OwnedTestEntity,
};
use crate::IgesCodec;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

fn entity(entity_type: i64, form: i64, parameters: &str) -> OwnedTestEntity {
    OwnedTestEntity {
        entity_type,
        form,
        label: "ATTR".into(),
        status: "00000000",
        parameters: parameters.into(),
    }
}

#[test]
fn property_values_have_one_neutral_home_and_native_records_keep_typed_values() {
    let bytes = owned_test_file(&[entity(406, 27, "406,4,4HNAME,1,3,3HVAL;")]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("generic property fixture decodes");
    let ir = result.ir();
    assert!(ir
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "NAME"
            && attribute.values == vec![AttributeValue::String("VAL".into())]));
    let record = &ir
        .native
        .namespace("iges")
        .expect("decoded IGES native namespace")
        .arenas()["properties"][0];
    assert_eq!(
        crate::test_support::native_bytes(&record.field("name").expect("typed native name field")),
        b"NAME"
    );
    assert_eq!(
        crate::test_support::native_bytes(
            &record.field("values").expect("typed native values field")[0]["value"]["value"]
        ),
        b"VAL"
    );
}

#[test]
fn property_owner_targets_the_projected_point_instead_of_the_native_property() {
    // The point's trailing groups contain no associativities and one property D3.
    let bytes = owned_test_file(&[
        entity(116, 0, "116,1,2,3,0,0,1,3;"),
        entity(406, 7, "406,1,2HP1;"),
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("owned reference-designator fixture decodes");
    let ir = result.ir();
    assert_eq!(ir.model.points.len(), 1);
    let property = ir
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "iges.property.reference_designator.value")
        .expect("projected reference-designator attribute exists");
    assert_eq!(
        property.target,
        AttributeTarget::Point(ir.model.points[0].id.clone())
    );
    assert_eq!(property.values, vec![AttributeValue::String("P1".into())]);
    let product_property = &ir
        .native
        .namespace("iges")
        .expect("decoded IGES native namespace")
        .arenas()["product_properties"][0];
    assert_eq!(
        crate::test_support::native_bytes(
            &product_property
                .field("value")
                .expect("typed native value field")
        ),
        b"P1"
    );
    assert_eq!(
        crate::test_support::native_bytes(
            &ir.native
                .namespace("iges")
                .expect("decoded IGES native namespace")
                .arenas()["properties"][0]
                .field("value")
                .expect("typed native value field")
        ),
        b"P1"
    );
}

#[test]
fn table_defaults_and_rows_keep_typed_native_values_after_projection() {
    let bytes = owned_test_file_with_structures(
        &[
            entity(322, 1, "322,4HATTR,0,1,1,3,1,3HVAL;"),
            entity(322, 0, "322,4HMETA,1,1,10,3,1;"),
            entity(422, 1, "422,1,4HITEM;"),
        ],
        &[(5, -3)],
    );
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("typed attribute table defaults and row fixture decodes");
    let ir = result.ir();
    assert!(ir.model.attributes.iter().any(|attribute| attribute.name
        == "iges.attribute_table.default.attributes.0.values.0"
        && attribute.values == vec![AttributeValue::String("VAL".into())]));
    assert!(ir.model.attributes.iter().any(|attribute| attribute.name
        == "iges.attribute_table.value.rows.0.0"
        && attribute.values == vec![AttributeValue::String("ITEM".into())]));
    let namespace = ir
        .native
        .namespace("iges")
        .expect("decoded IGES native namespace");
    let schema = namespace.arenas()["attribute_table_definitions"][0]
        .field("attributes")
        .expect("typed native attributes field");
    assert_eq!(
        crate::test_support::native_bytes(&schema[0]["values"][0]["value"]["value"]),
        b"VAL"
    );
    assert_eq!(schema[0]["value_data_type"], 3);
    let instance = &namespace.arenas()["attribute_table_instances"][0];
    let rows = instance.field("rows").expect("typed native rows field");
    assert_eq!(
        rows.as_array()
            .expect("native table rows are an array")
            .iter()
            .map(|row| row
                .as_array()
                .expect("native table value is an array")
                .len())
            .collect::<Vec<_>>(),
        [1]
    );
    assert_eq!(
        crate::test_support::native_bytes(&rows[0][0]["value"]),
        b"ITEM"
    );
}

#[test]
fn directory_carrier_label_has_one_editable_neutral_name() {
    let mut point = entity(116, 0, "116,1,2,3,0;");
    point.label = "HELLO".into();
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[point])),
            &DecodeOptions::default(),
        )
        .expect("directory-label point fixture decodes");
    let ir = result.ir();
    let name = ir
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "name")
        .expect("projected point name attribute exists");
    assert_eq!(
        name.target,
        AttributeTarget::Point(ir.model.points[0].id.clone())
    );
    assert_eq!(name.values, [AttributeValue::String("HELLO".into())]);
    let native = &ir
        .native
        .namespace("iges")
        .expect("decoded IGES native namespace")
        .arenas()["entities"][0];
    assert_eq!(
        crate::test_support::native_bytes(
            &native.field("label").expect("typed native label field")
        ),
        b"   HELLO"
    );
}

#[test]
fn full_name_property_and_distinct_directory_label_remain_distinct_facts() {
    let mut point = entity(116, 0, "116,1,2,3,0,0,1,3;");
    point.label = "SHORT".into();
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                point,
                entity(406, 15, "406,1,12Hlonger label;"),
            ])),
            &DecodeOptions::default(),
        )
        .expect("full-name and directory-label fixture decodes");
    let ir = result.ir();
    for (name, value) in [("name", "longer label"), ("iges.directory_label", "SHORT")] {
        assert!(ir
            .model
            .attributes
            .iter()
            .any(|attribute| attribute.name == name
                && attribute.values == [AttributeValue::String(value.into())]));
    }
    let native = &ir
        .native
        .namespace("iges")
        .expect("decoded IGES native namespace")
        .arenas()["properties"][0];
    assert_eq!(
        crate::test_support::native_bytes(
            &native.field("value").expect("typed native value field")
        ),
        b"longer label"
    );
}

mod linkage;
