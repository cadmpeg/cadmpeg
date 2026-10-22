// SPDX-License-Identifier: Apache-2.0
//! Original scalar value bytes feed kept fields and exact semantic read ranges.

use cadmpeg_core::decode::u64_from_index;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn scalar_source_edits_change_the_kept_value_without_crediting_opaque_or_xml_markup_bytes() {
    for literal in ["12.5", "42.5"] {
        let document = format!("<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object name='Owner' type='App::Feature'/></Objects><ObjectData Count='1'><Object name='Owner'><Properties Count='2'><Property name='Distance' type='App::PropertyFloat'><Float unit='mm' value='{literal}'/></Property><Property name='Proxy' type='App::PropertyPythonObject'><Python value='AAAB-pickle'/></Property></Properties></Object></ObjectData></Document>");
        let bytes = crate::test_support::test_archive::archive(&document);
        let decoded = crate::FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let native = decoded.ir().native.namespace("fcstd").unwrap();
        let properties = native
            .arena_as::<crate::native::PropertyRecord>("properties")
            .unwrap();
        let property = properties
            .iter()
            .find(|property| property.name == "Distance")
            .unwrap();
        assert_eq!(property.values()[0].attributes["value"], literal);
        let fidelity = decoded.source_fidelity();
        let layout = &fidelity.record_layout()[&property.id];
        let start = document.find(&format!("value='{literal}'")).unwrap() + "value='".len();
        let range = u64_from_index(start)..u64_from_index(start + literal.len());
        let identity_range = |name: &str, raw: &str| {
            let start =
                document.find(&format!("{name}='{raw}'")).unwrap() + name.len() + "='".len();
            u64_from_index(start)..u64_from_index(start + raw.len())
        };
        assert_eq!(
            layout.reads()[layout.stream()],
            [
                identity_range("name", "Distance"),
                identity_range("type", "App::PropertyFloat"),
                range.clone()
            ]
        );
        let stream = &fidelity.b1_spans()[layout.stream()];
        assert!(stream.spans().iter().all(|span| {
            let unread = span.range();
            unread.end <= range.start || unread.start >= range.end
        }));
        let opaque_start = u64_from_index(document.find("AAAB-pickle").unwrap());
        let opaque_end = opaque_start + u64_from_index("AAAB-pickle".len());
        assert!(stream.spans().iter().any(|span| {
            let unread = span.range();
            unread.start <= opaque_start && opaque_end <= unread.end
        }));
        let opaque = properties
            .iter()
            .find(|property| property.name == "Proxy")
            .unwrap();
        let opaque_layout = &fidelity.record_layout()[&opaque.id];
        assert_eq!(
            opaque_layout.reads()[opaque_layout.stream()],
            [
                identity_range("name", "Proxy"),
                identity_range("type", "App::PropertyPythonObject")
            ]
        );
        assert!(fidelity.record_layout()[&property.id]
            .payloads()
            .contains_key("raw_xml"));
        assert_eq!(
            fidelity.record_layout()[&property.id].payloads()["raw_xml"].as_slice(),
            format!("<Property name='Distance' type='App::PropertyFloat'><Float unit='mm' value='{literal}'/></Property>").as_bytes()
        );
    }
}
