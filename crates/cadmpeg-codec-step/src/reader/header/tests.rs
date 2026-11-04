// SPDX-License-Identifier: Apache-2.0
//! `FILE_NAME` projection tests.

use std::io::Cursor;

use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::StepCodec;

fn document_strings(result: &cadmpeg_ir::codec::DecodeResult, name: &str) -> Vec<String> {
    result
        .ir()
        .model
        .attributes
        .iter()
        .filter(|attribute| attribute.target == AttributeTarget::Document && attribute.name == name)
        .map(|attribute| {
            let [Some(cadmpeg_ir::attributes::AttributeValue::String(value))] =
                attribute.values.as_slice()
            else {
                panic!("document attribute {name} holds one string");
            };
            value.clone()
        })
        .collect()
}

#[test]
fn file_name_fields_project_as_document_attributes() {
    let source = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('part','2026-07-14T00:00:00',('anne','beth'),('org'),'cadmpeg-step','origin','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT('',(1.,2.,3.));ENDSEC;END-ISO-10303-21;";
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode header");
    assert_eq!(document_strings(&result, "Label"), ["part"]);
    assert_eq!(document_strings(&result, "author"), ["anne"]);
    assert_eq!(document_strings(&result, "CreatedBy"), ["anne"]);
    assert_eq!(document_strings(&result, "LastModifiedBy"), ["beth"]);
    assert_eq!(document_strings(&result, "organization"), ["org"]);
    assert_eq!(document_strings(&result, "Company"), ["org"]);
    assert_eq!(
        document_strings(&result, "generation_date"),
        ["2026-07-14T00:00:00"]
    );
    assert_eq!(
        document_strings(&result, "LastModifiedDate"),
        ["2026-07-14T00:00:00"]
    );
    assert_eq!(document_strings(&result, "system_id"), ["origin"]);
}

#[test]
fn empty_file_name_fields_project_as_empty_attributes() {
    let source = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('rims','2026-07-14T00:00:00',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT('',(1.,2.,3.));ENDSEC;END-ISO-10303-21;";
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode header");
    assert_eq!(document_strings(&result, "Label"), ["rims"]);
    assert_eq!(document_strings(&result, "author"), [""]);
    assert_eq!(document_strings(&result, "system_id"), [""]);
}
