// SPDX-License-Identifier: Apache-2.0
//! Well-formed annotation documents retain strict registered carrier grammars.

use crate::test_support::test_archive::archive;
use crate::FcstdCodec;
use cadmpeg_core::CodecError;
use cadmpeg_ir::{Codec, DecodeFailure, DecodeOptions};
use std::io::Cursor;

fn assert_carrier_refused(document: &str) {
    roxmltree::Document::parse(document).expect("well-formed document XML");
    assert!(matches!(
        FcstdCodec.decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        ),
        Err(DecodeFailure::Codec(CodecError::Malformed(_)))
    ));
}

#[test]
fn well_formed_rich_annotation_refuses_uppercase_string_attribute() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="TechDraw::DrawRichAnno" name="Rich"/></Objects>
<ObjectData Count="1"><Object name="Rich"><Properties Count="3"><Property name="AnnoText" type="App::PropertyString"><String Value="BAD"/></Property><Property name="X" type="App::PropertyDistance"><Float value="10"/></Property><Property name="Y" type="App::PropertyDistance"><Float value="20"/></Property></Properties></Object></ObjectData></Document>"#;
    assert_carrier_refused(document);
}

#[test]
fn well_formed_dimension_refuses_duplicate_format_attribute_spellings() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="TechDraw::DrawViewDimension" name="Dimension"/></Objects>
<ObjectData Count="1"><Object name="Dimension"><Properties Count="3"><Property name="FormatSpec" type="App::PropertyString"><String value="A" Value="B"/></Property><Property name="X" type="App::PropertyDistance"><Float value="10"/></Property><Property name="Y" type="App::PropertyDistance"><Float value="20"/></Property></Properties></Object></ObjectData></Document>"#;
    assert_carrier_refused(document);
}
