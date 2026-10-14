// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::test_owned::{
    owned_test_file, owned_test_file_with_structures, OwnedTestEntity,
};
use crate::IgesCodec;

fn native_entity(entity_type: i64, form: i64, parameters: &str) -> OwnedTestEntity {
    OwnedTestEntity {
        entity_type,
        form,
        label: "NATIVE".into(),
        status: "00000000",
        parameters: parameters.into(),
    }
}

fn assert_native_arena(bytes: &[u8], arena: &str) {
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        result.ir().native.namespace("iges").unwrap().arenas()[arena].len(),
        1,
    );
}

#[test]
fn native_attribute_definition_and_instance_keep_nested_values() {
    let definition = owned_test_file(&[native_entity(322, 1, "322,4HATTR,0,1,1,1,1,3HVAL;")]);
    assert_native_arena(&definition, "attribute_table_definitions");
    let decoded = IgesCodec
        .decode(&mut Cursor::new(&definition), &DecodeOptions::default())
        .unwrap();
    let definitions =
        &decoded.ir().native.namespace("iges").unwrap().arenas()["attribute_table_definitions"];
    assert_eq!(
        definitions[0].fields()["attributes"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let instance = owned_test_file_with_structures(
        &[
            native_entity(322, 0, "322,4HMETA,1,1,10,1,1;"),
            native_entity(422, 1, "422,1,4HITEM;"),
        ],
        &[(3, -1)],
    );
    assert_native_arena(&instance, "attribute_table_instances");
    let decoded = IgesCodec
        .decode(&mut Cursor::new(&instance), &DecodeOptions::default())
        .unwrap();
    let instances =
        &decoded.ir().native.namespace("iges").unwrap().arenas()["attribute_table_instances"];
    assert_eq!(instances[0].fields()["rows"].as_array().unwrap().len(), 1);
}

#[test]
fn native_property_forms_keep_one_record() {
    for (form, parameters) in [
        (11, "406,7,0,1,1,1,1,2,3;"),
        (24, "406,5,1,7,2HID,8,4HFUNC;"),
        (27, "406,3,4HNAME,1,1,3HVAL;"),
        (12, "406,1,4HFILE;"),
        (25, "406,3,4HNAME,1,7;"),
        (30, "406,0,0,0,1,1HL,0,0,0,0,0,0,0,1,0,0,0;"),
        (31, "406,0,1,2,3,4,5,6,7,8,9;"),
        (34, "406,0,1,0,1,2;"),
    ] {
        let bytes = owned_test_file(&[native_entity(406, form, parameters)]);
        let decoded = IgesCodec
            .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
            .unwrap();
        let properties = &decoded.ir().native.namespace("iges").unwrap().arenas()["properties"];
        assert_eq!(properties.len(), 1, "form {form}");
    }
}
