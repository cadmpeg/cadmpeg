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
fn property_values_have_one_neutral_home_and_native_records_keep_only_links() {
    let bytes = owned_test_file(&[entity(406, 27, "406,4,4HNAME,1,3,3HVAL;")]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let ir = result.ir();
    assert!(ir
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "NAME"
            && attribute.values == vec![AttributeValue::String("VAL".into())]));
    let record = &ir.native.namespace("iges").unwrap().arenas()["properties"][0];
    assert!(record.field("name").is_none());
    assert!(record.field("values").is_none());
    let links = record.field("neutral_values").unwrap();
    for reference in links
        .as_object()
        .unwrap()
        .values()
        .flat_map(|value| value.as_array().unwrap())
    {
        assert!(ir
            .model
            .attributes
            .iter()
            .any(|attribute| attribute.id.as_str() == reference.as_str().unwrap()));
    }
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
        .unwrap();
    let ir = result.ir();
    assert_eq!(ir.model.points.len(), 1);
    let property = ir
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "iges.property.reference_designator.value")
        .unwrap();
    assert_eq!(
        property.target,
        AttributeTarget::Point(ir.model.points[0].id.clone())
    );
    assert_eq!(property.values, vec![AttributeValue::String("P1".into())]);
    let product_property = &ir.native.namespace("iges").unwrap().arenas()["product_properties"][0];
    assert!(product_property.field("value").is_none());
    assert_eq!(
        product_property.field("neutral_values"),
        ir.native.namespace("iges").unwrap().arenas()["properties"][0].field("neutral_values")
    );
}

#[test]
fn table_defaults_and_rows_project_without_retaining_second_value_copies() {
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
        .unwrap();
    let ir = result.ir();
    assert!(ir.model.attributes.iter().any(|attribute| attribute.name
        == "iges.attribute_table.default.attributes.0.values.0"
        && attribute.values == vec![AttributeValue::String("VAL".into())]));
    assert!(ir.model.attributes.iter().any(|attribute| attribute.name
        == "iges.attribute_table.value.rows.0.0"
        && attribute.values == vec![AttributeValue::String("ITEM".into())]));
    let namespace = ir.native.namespace("iges").unwrap();
    let schema = namespace.arenas()["attribute_table_definitions"][0]
        .field("attributes")
        .unwrap();
    assert!(schema[0].get("values").is_none());
    assert_eq!(schema[0]["value_data_type"], 3);
    let instance = &namespace.arenas()["attribute_table_instances"][0];
    assert!(instance.field("rows").is_none());
    assert_eq!(
        instance.field("row_widths").unwrap(),
        serde_json::json!([1])
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
        .unwrap();
    let ir = result.ir();
    let name = ir
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "name")
        .unwrap();
    assert_eq!(
        name.target,
        AttributeTarget::Point(ir.model.points[0].id.clone())
    );
    assert_eq!(name.values, [AttributeValue::String("HELLO".into())]);
    let native = &ir.native.namespace("iges").unwrap().arenas()["entities"][0];
    assert!(native.field("label").is_none());
    assert_eq!(native.field("neutral_label").unwrap()[0], name.id.as_str());
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
        .unwrap();
    let ir = result.ir();
    for (name, value) in [("name", "longer label"), ("iges.directory_label", "SHORT")] {
        assert!(ir
            .model
            .attributes
            .iter()
            .any(|attribute| attribute.name == name
                && attribute.values == [AttributeValue::String(value.into())]));
    }
    let native = &ir.native.namespace("iges").unwrap().arenas()["properties"][0];
    assert!(native.field("value").is_none());
    assert!(native.field("neutral_values").is_some());
}
