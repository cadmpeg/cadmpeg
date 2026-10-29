// SPDX-License-Identifier: Apache-2.0
//! Public decode keeps vector facts while unrelated bytes stay unrepresented.

use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn current_vector_fields_retain_exact_source_reads_without_crediting_opaque_extras() {
    let property = "<Property name='ExtraVector' type='App::PropertyVector'><PropertyVector valueX='1.25' valueY='-2' valueZ='3' Vendor='opaque-extra'/></Property>";
    let document = format!("<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object name='Owner' type='App::Feature'/></Objects><ObjectData Count='1'><Object name='Owner'><Properties Count='1'>{property}</Properties></Object></ObjectData></Document>");
    let decoded = crate::FcstdCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_archive::archive(&document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let properties = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let [native] = properties.as_slice() else {
        panic!("one property: {properties:?}")
    };
    assert_eq!(native.name, "ExtraVector");
    assert_eq!(native.values()[0].attributes["Vendor"], "opaque-extra");
    let projected = decoded
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.native_ref.as_deref() == Some(native.id.as_str()))
        .expect("the current decoded vector attribute");
    assert_eq!(projected.name, "ExtraVector");
    let [Some(cadmpeg_ir::attributes::AttributeValue::Vector(coordinates))] =
        projected.values.as_slice()
    else {
        panic!("finite vector: {projected:?}")
    };
    assert_eq!(
        coordinates
            .iter()
            .map(|coordinate| coordinate.get())
            .collect::<Vec<_>>(),
        [1.25, -2.0, 3.0]
    );
    let changed_document = document.replace("Vendor='opaque-extra'", "Vendor='current-extra'");
    let changed = crate::FcstdCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_archive::archive(
                &changed_document,
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let changed_properties = changed
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let [changed_native] = changed_properties.as_slice() else {
        panic!("one property after changing only the retained native extra")
    };
    assert_ne!(changed_native, native);
    assert_eq!(
        changed_native.values()[0].attributes["Vendor"],
        "current-extra"
    );
    let changed_vector = changed
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.native_ref.as_deref() == Some(changed_native.id.as_str()))
        .unwrap();
    assert_eq!(changed_vector.values, projected.values);
}
