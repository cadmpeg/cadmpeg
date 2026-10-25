// SPDX-License-Identifier: Apache-2.0
//! Public decode keeps vector facts while unrelated bytes stay unrepresented.

use cadmpeg_core::decode::u64_from_index;
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
    assert!(decoded
        .ir()
        .model
        .attributes
        .iter()
        .all(|attribute| attribute.native_ref.as_deref() != Some(native.id.as_str())));
    assert_eq!(
        ["valueX", "valueY", "valueZ"].map(|key| native.values()[0].attributes[key].as_str()),
        ["1.25", "-2", "3"]
    );
    let fidelity = decoded.source_fidelity();
    let layout = &fidelity.record_layout()[&native.id];
    let range = |key: &str, raw: &str| {
        let start = document.find(&format!("{key}='{raw}'")).unwrap() + key.len() + "='".len();
        u64_from_index(start)..u64_from_index(start + raw.len())
    };
    let key = |key: &str, raw: &str| {
        let at = document.find(&format!("{key}='{raw}'")).unwrap();
        u64_from_index(at)..u64_from_index(at + key.len())
    };
    assert_eq!(
        layout.reads()[layout.stream()],
        [
            key("name", "ExtraVector"),
            range("name", "ExtraVector"),
            key("type", "App::PropertyVector"),
            range("type", "App::PropertyVector"),
            {
                let at = document.find("<PropertyVector ").unwrap() + 1;
                u64_from_index(at)..u64_from_index(at + "PropertyVector".len())
            },
            key("valueX", "1.25"),
            range("valueX", "1.25"),
            key("valueY", "-2"),
            range("valueY", "-2"),
            key("valueZ", "3"),
            range("valueZ", "3"),
            key("Vendor", "opaque-extra"),
            range("Vendor", "opaque-extra")
        ]
    );
    assert_eq!(
        crate::test_support::source_fidelity::bytes(fidelity, &native.id),
        property.as_bytes()
    );
    // This attribute is held verbatim in the typed native value, so its
    // actual source producer must read it even though it is not a coordinate.
    let held_extra = range("Vendor", "opaque-extra");
    assert!(fidelity.b1_spans()[layout.stream()]
        .spans()
        .iter()
        .all(|span| {
            let unread = span.range();
            unread.end <= held_extra.start || held_extra.end <= unread.start
        }));
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
    assert_eq!(
        ["valueX", "valueY", "valueZ"]
            .map(|key| changed_native.values()[0].attributes[key].as_str()),
        ["1.25", "-2", "3"]
    );
}

#[test]
fn vector_with_a_real_neutral_owner_keeps_coordinates_when_native_extras_change() {
    let document = "<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object name='Owner' type='App::Part'/></Objects><ObjectData Count='1'><Object name='Owner'><Properties Count='1'><Property name='ExtraVector' type='App::PropertyVector'><PropertyVector valueX='1.25' valueY='-2' valueZ='3' Vendor='opaque-extra'/></Property></Properties></Object></ObjectData></Document>";
    let decode = |xml: &str| {
        crate::FcstdCodec
            .decode(
                &mut Cursor::new(crate::test_support::test_archive::archive(xml)),
                &DecodeOptions::default(),
            )
            .unwrap()
    };
    let before = decode(document);
    let after = decode(&document.replace("opaque-extra", "current-extra"));
    for decoded in [&before, &after] {
        let [attribute] = decoded.ir().model.attributes.as_slice() else {
            panic!("one owned vector attribute")
        };
        assert_eq!(attribute.name, "ExtraVector");
        let [cadmpeg_ir::attributes::AttributeValue::Vector(coordinates)] =
            attribute.values.as_slice()
        else {
            panic!("finite vector")
        };
        assert_eq!(
            coordinates
                .iter()
                .map(|coordinate| coordinate.get())
                .collect::<Vec<_>>(),
            [1.25, -2., 3.]
        );
        assert!(
            matches!(&attribute.target, cadmpeg_ir::attributes::AttributeTarget::Record { arena, id } if arena.as_str() == "product_definitions" && decoded.ir().model.contains_record(*arena, id))
        );
    }
    assert_eq!(before.ir().model.attributes, after.ir().model.attributes);
}
