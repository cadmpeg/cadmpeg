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
    let projected = decoded
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.native_ref.as_deref() == Some(native.id.as_str()))
        .expect("the current decoded vector attribute");
    assert_eq!(projected.name, "ExtraVector");
    let [cadmpeg_ir::attributes::AttributeValue::Vector(coordinates)] = projected.values.as_slice()
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
    let fidelity = decoded.source_fidelity();
    let layout = &fidelity.record_layout()[&native.id];
    let range = |key: &str, raw: &str| {
        let start = document.find(&format!("{key}='{raw}'")).unwrap() + key.len() + "='".len();
        u64_from_index(start)..u64_from_index(start + raw.len())
    };
    assert_eq!(
        layout.reads()[layout.stream()],
        [
            range("name", "ExtraVector"),
            range("type", "App::PropertyVector"),
            range("valueX", "1.25"),
            range("valueY", "-2"),
            range("valueZ", "3")
        ]
    );
    assert_eq!(
        crate::test_support::source_fidelity::bytes(fidelity, &native.id),
        property.as_bytes()
    );
    let opaque = range("Vendor", "opaque-extra");
    assert!(fidelity.b1_spans()[layout.stream()]
        .spans()
        .iter()
        .any(|span| {
            let unread = span.range();
            unread.start <= opaque.start && opaque.end <= unread.end
        }));
}
