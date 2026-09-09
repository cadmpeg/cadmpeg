// SPDX-License-Identifier: Apache-2.0
//! Header metadata must not determine whether DATA geometry survives.

use crate::parse::{parse, ParseDiagnosticKind};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn header_metadata_defects_preserve_data_and_source_records() {
    let description = "FILE_DESCRIPTION(('test'),'2;1');";
    let name = "FILE_NAME('','',(''),(''),'','','');";
    let schema = "FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));";
    let cases = [
        format!("{description}{schema}"),
        format!("{schema}{name}{description}"),
        schema.to_string(),
        format!("{description}{name}{name}{schema}"),
        format!("{description}{description}{name}{schema}"),
        format!("FILE_DESCRIPTION($,$);{name}{schema}"),
        format!("{description}FILE_NAME();{schema}"),
        format!("{description}FILE_NAME('','bad date',(''),(''),'','','');{schema}"),
        format!("{description}{name}{schema}SECTION_LANGUAGE();"),
        format!("{description}{name}{schema}SECTION_CONTEXT();"),
        format!("{description}{name}{schema}VENDOR_METADATA('retained');"),
    ];
    for header in cases {
        let source = format!("ISO-10303-21;HEADER;{header}ENDSEC;DATA;#1=CARTESIAN_POINT('point',(1.,2.,3.));ENDSEC;END-ISO-10303-21;");
        let (exchange, diagnostics) = parse(source.as_bytes()).expect("recoverable metadata");
        assert_eq!(exchange.records.len(), 1, "{header}");
        assert_eq!(
            exchange.records[&1].partials.first().name,
            "CARTESIAN_POINT"
        );
        assert!(
            diagnostics.iter().any(
                |diagnostic| diagnostic.kind == ParseDiagnosticKind::HeaderMetadataNoncanonical
            ),
            "{header}"
        );
        for record in &exchange.header {
            assert!(source[record.offset..].starts_with(&record.name));
        }
    }
}

#[test]
fn reordered_schema_diagnostic_uses_the_schema_offset() {
    let source = b"ISO-10303-21;HEADER;FILE_SCHEMA(('AP242 { 3 40 }'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (_, diagnostics) = parse(source).expect("schema name survives invalid OID");
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.kind == ParseDiagnosticKind::SchemaObjectIdentifierOutOfRange)
        .expect("OID diagnostic");
    assert!(source[diagnostic.offset..].starts_with(b"FILE_SCHEMA"));
}

#[test]
fn header_recovery_does_not_hide_lost_framing_or_ambiguous_schema() {
    for header in [
        "FILE_SCHEMA(('AP242'));FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));",
        "FILE_SCHEMA(('AP242'));FILE_NAME('unterminated);",
        "FILE_SCHEMA($);",
    ] {
        let source =
            format!("ISO-10303-21;HEADER;{header}ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;");
        assert!(parse(source.as_bytes()).is_err(), "{header}");
    }
}

#[test]
fn missing_file_name_keeps_geometry_and_retains_header_bytes() {
    let original = include_str!("../../../tests/fixtures/ap214_sheet.p21");
    let source = original
        .lines()
        .filter(|line| !line.starts_with("FILE_NAME"))
        .collect::<Vec<_>>()
        .join("\n");
    let codec = crate::StepCodec::default();
    let expected = codec
        .decode(&mut Cursor::new(original), &DecodeOptions::default())
        .unwrap();
    let recovered = codec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .unwrap();
    assert!(!expected.ir().model.faces.is_empty());
    assert_eq!(recovered.ir().model, expected.ir().model);
    assert!(recovered
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::StepLossCode::HeaderMetadataNoncanonical.kind()));
    let offset = source.find("FILE_DESCRIPTION").unwrap();
    let id = crate::ids::header(offset);
    let retained = recovered
        .source_fidelity()
        .retained_record(id.as_str())
        .unwrap();
    // The implementation-level string itself contains a semicolon. Compare
    // against the complete physical header line instead of splitting tokens.
    let record = source[offset..].lines().next().unwrap();
    assert_eq!(retained.data(), Some(record.as_bytes()));
}
