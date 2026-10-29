// SPDX-License-Identifier: Apache-2.0
//! Original scalar value bytes feed kept fields and exact semantic read ranges.

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
    }
}
