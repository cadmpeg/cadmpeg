// SPDX-License-Identifier: Apache-2.0
//! Public decode keeps vector facts while unrelated bytes stay unrepresented.

use cadmpeg_ir::codec::write::Encoder;
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
    // The Owner object has no neutral home, so the vector stays native-only.
    assert!(decoded
        .ir()
        .model
        .attributes
        .iter()
        .all(|attribute| attribute.native_ref.as_deref() != Some(native.id.as_str())));
    let coordinates = |property: &crate::native::PropertyRecord| {
        property.values()[0]
            .attributes
            .get("valueX")
            .zip(property.values()[0].attributes.get("valueY"))
            .zip(property.values()[0].attributes.get("valueZ"))
            .map(|((x, y), z)| (x.clone(), y.clone(), z.clone()))
    };
    assert_eq!(
        coordinates(native).as_ref().map(|(x, y, z)| (
            x.as_str(),
            y.as_str(),
            z.as_str()
        )),
        Some(("1.25", "-2", "3"))
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
    assert!(changed
        .ir()
        .model
        .attributes
        .iter()
        .all(|attribute| attribute.native_ref.as_deref() != Some(changed_native.id.as_str())));
    assert_eq!(coordinates(changed_native), coordinates(native));
    let plan = crate::FcstdCodec
        .plan(
            cadmpeg_ir::codec::write::EncodeInput::new(changed.ir(), None),
            cadmpeg_ir::codec::write::target::TargetRequest::Inherit,
        )
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes.as_slice()), &DecodeOptions::default())
        .unwrap();
    let output_properties = output
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let output_native = output_properties
        .iter()
        .find(|property| property.name == "ExtraVector")
        .expect("kept vector property after the preserving write");
    assert_eq!(output_native.name, "ExtraVector");
    assert_eq!(
        output_native.values()[0].attributes["Vendor"],
        "current-extra"
    );
    assert_eq!(coordinates(output_native), coordinates(native));
}
