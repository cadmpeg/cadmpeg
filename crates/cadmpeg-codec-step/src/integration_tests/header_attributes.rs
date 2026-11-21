// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::StepCodec;

fn source(header: &str) -> String {
    format!(
        "ISO-10303-21;\nHEADER;\n{header}\nENDSEC;\nDATA;\n#1=CARTESIAN_POINT('',(0.,0.,0.));\nENDSEC;\nEND-ISO-10303-21;\n"
    )
}

/// The decoded source attributes whose keys name a header record.
fn header_attributes(header: &str) -> BTreeMap<String, String> {
    let result = StepCodec::default()
        .decode(
            &mut Cursor::new(source(header).into_bytes()),
            &DecodeOptions::default(),
        )
        .expect("decode");
    result
        .ir()
        .source
        .as_ref()
        .expect("source metadata")
        .attributes
        .iter()
        .filter(|(key, _)| key.as_str().starts_with("file_"))
        .map(|(key, value)| (key.as_str().to_owned(), value.clone()))
        .collect()
}

fn expected(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

#[test]
fn solidworks_header_fields_become_source_attributes() {
    let attributes = header_attributes(
        "FILE_DESCRIPTION (( 'STEP AP214' ),\n    '2;1' );\n\
         FILE_NAME ('Bracket.STEP',\n    '2024-03-05T14:22:10',\n    ( 'jdoe' ),\n    ( 'Acme' ),\n    \
         'SwSTEP 2.0',\n    'SolidWorks 2023',\n    '' );\n\
         FILE_SCHEMA (( 'AUTOMOTIVE_DESIGN' ));",
    );
    assert_eq!(
        attributes,
        expected(&[
            ("file_description.description.0", "STEP AP214"),
            ("file_description.implementation_level", "2;1"),
            ("file_name.name", "Bracket.STEP"),
            ("file_name.time_stamp", "2024-03-05T14:22:10"),
            ("file_name.author.0", "jdoe"),
            ("file_name.organization.0", "Acme"),
            ("file_name.preprocessor_version", "SwSTEP 2.0"),
            ("file_name.originating_system", "SolidWorks 2023"),
            ("file_schema.schema_identifiers.0", "AUTOMOTIVE_DESIGN"),
        ])
    );
}

#[test]
fn other_producer_header_keeps_every_list_member_and_the_authorization() {
    let attributes = header_attributes(
        "FILE_DESCRIPTION(('Open CASCADE Model','second line'),'2;1');\n\
         FILE_NAME('Open CASCADE Shape Model','2025-01-02T03:04:05',('Author A','Author B'),\
         ('Org'),'Open CASCADE STEP processor 7.8','Open CASCADE 7.8.1','Approved');\n\
         FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }','AUTOMOTIVE_DESIGN'));",
    );
    assert_eq!(
        attributes,
        expected(&[
            ("file_description.description.0", "Open CASCADE Model"),
            ("file_description.description.1", "second line"),
            ("file_description.implementation_level", "2;1"),
            ("file_name.name", "Open CASCADE Shape Model"),
            ("file_name.time_stamp", "2025-01-02T03:04:05"),
            ("file_name.author.0", "Author A"),
            ("file_name.author.1", "Author B"),
            ("file_name.organization.0", "Org"),
            (
                "file_name.preprocessor_version",
                "Open CASCADE STEP processor 7.8"
            ),
            ("file_name.originating_system", "Open CASCADE 7.8.1"),
            ("file_name.authorization", "Approved"),
            (
                "file_schema.schema_identifiers.0",
                "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }"
            ),
            ("file_schema.schema_identifiers.1", "AUTOMOTIVE_DESIGN"),
        ])
    );
}

#[test]
fn empty_and_omitted_header_strings_state_no_attribute() {
    let attributes = header_attributes(
        "FILE_DESCRIPTION((''),'2;1');\n\
         FILE_NAME('','',(''),(''),$,$,$);\n\
         FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));",
    );
    assert_eq!(
        attributes,
        expected(&[
            ("file_description.implementation_level", "2;1"),
            ("file_schema.schema_identifiers.0", "CONFIG_CONTROL_DESIGN"),
        ])
    );
}

#[test]
fn header_strings_decode_part_21_escapes() {
    let attributes = header_attributes(
        "FILE_DESCRIPTION(('it''s'),'2;1');\n\
         FILE_NAME('caf\\X2\\00E9\\X0\\','',('O''Brien'),('back\\\\slash'),'','Sys\\S\\D','');\n\
         FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));",
    );
    assert_eq!(attributes["file_description.description.0"], "it's");
    assert_eq!(attributes["file_name.name"], "caf\u{e9}");
    assert_eq!(attributes["file_name.author.0"], "O'Brien");
    assert_eq!(attributes["file_name.organization.0"], "back\\slash");
    assert_eq!(attributes["file_name.originating_system"], "Sys\u{c4}");
}

#[test]
fn header_strings_leave_out_control_characters() {
    let attributes = header_attributes(
        "FILE_DESCRIPTION(('a\\X\\0Ab\\X\\09c'),'2;1');\n\
         FILE_NAME('n','',(''),(''),'','\\X2\\0000001B\\X0\\Sys\\X2\\007F\\X0\\','');\n\
         FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));",
    );
    assert_eq!(attributes["file_description.description.0"], "abc");
    assert_eq!(attributes["file_name.originating_system"], "Sys");
}

#[test]
fn long_header_strings_and_lists_are_kept_whole() {
    let long = "x".repeat(10_000);
    let authors = (0..100)
        .map(|index| format!("'a{index}'"))
        .collect::<Vec<_>>()
        .join(",");
    let attributes = header_attributes(&format!(
        "FILE_DESCRIPTION(('d'),'2;1');\n\
         FILE_NAME('n','',({authors}),(''),'','{long}','');\n\
         FILE_SCHEMA(('AUTOMOTIVE_DESIGN','S{long}'));"
    ));
    assert_eq!(attributes["file_name.originating_system"], long);
    assert_eq!(
        attributes["file_schema.schema_identifiers.1"],
        format!("S{long}")
    );
    let author_keys = attributes
        .keys()
        .filter(|key| key.starts_with("file_name.author."))
        .count();
    assert_eq!(author_keys, 100);
    assert_eq!(attributes["file_name.author.64"], "a64");
    assert_eq!(attributes["file_name.author.99"], "a99");
}
