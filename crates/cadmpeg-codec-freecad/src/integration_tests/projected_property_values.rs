// SPDX-License-Identifier: Apache-2.0
//! Readable property values project to neutral attributes.

use super::*;
use cadmpeg_ir::attributes::AttributeValue;

fn attribute(property: &str, side: Option<&[u8]>) -> Vec<Option<AttributeValue>> {
    let document = format!(
        r#"<Document SchemaVersion="4"><Objects Count="1"><Object name="Feature" type="PartDesign::Feature"/></Objects><ObjectData Count="1"><Object name="Feature"><Properties Count="1">{property}</Properties></Object></ObjectData></Document>"#
    );
    let mut entries = vec![("Document.xml", document.as_bytes())];
    if let Some(side) = side {
        entries.push(("ListData", side));
    }
    let result = decode(archive_entries(&entries));
    let value = result
        .ir()
        .model
        .attributes
        .iter()
        .find(|a| a.name == "Extra")
        .expect("property has an attribute");
    assert!(value.native_ref.is_some());
    value.values.clone()
}

#[test]
fn path_projects_as_string() {
    assert_eq!(
        attribute(
            r#"<Property name="Extra" type="App::PropertyPath"><Path value="design/data"/></Property>"#,
            None
        ),
        vec![Some(AttributeValue::String("design/data".into()))]
    );
}

fn assert_vector(values: &[Option<AttributeValue>], expected: &[f64]) {
    let [Some(AttributeValue::Vector(actual))] = values else {
        panic!("expected vector")
    };
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert!((actual.get() - expected).abs() < 1e-12);
    }
}

#[test]
fn rotation_projects_axis_and_angle() {
    assert_vector(
        &attribute(
            r#"<Property name="Extra" type="App::PropertyRotation"><PropertyRotation A="0.5" Ox="0" Oy="1" Oz="0"/></Property>"#,
            None,
        ),
        &[0.5, 0.0, 1.0, 0.0],
    );
}

#[test]
fn matrix_projects_in_row_order() {
    assert_vector(
        &attribute(
            r#"<Property name="Extra" type="App::PropertyMatrix"><PropertyMatrix a11="1" a12="2" a13="3" a14="4" a21="5" a22="6" a23="7" a24="8" a31="9" a32="10" a33="11" a34="12" a41="13" a42="14" a43="15" a44="16"/></Property>"#,
            None,
        ),
        &[
            1., 2., 3., 4., 5., 6., 7., 8., 9., 10., 11., 12., 13., 14., 15., 16.,
        ],
    );
}

#[test]
fn float_list_projects_side_entry_in_order() {
    let mut side = 2_u32.to_le_bytes().to_vec();
    side.extend_from_slice(&1.25_f64.to_le_bytes());
    side.extend_from_slice(&(-2.5_f64).to_le_bytes());
    let values = attribute(
        r#"<Property name="Extra" type="App::PropertyFloatList"><FloatList file="ListData"/></Property>"#,
        Some(&side),
    );
    let [Some(AttributeValue::Float(first)), Some(AttributeValue::Float(second))] =
        values.as_slice()
    else {
        panic!("expected two floats")
    };
    assert!((first.get() - 1.25).abs() < 1e-12);
    assert!((second.get() + 2.5).abs() < 1e-12);
}
