// SPDX-License-Identifier: Apache-2.0
//! Part 21 header, implementation-level, and envelope tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]
#![allow(unused_imports)]

use std::fmt::Write as _;
use std::io::Cursor;

use cadmpeg_core::decode::{DecodeMode, InspectOptions};
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};
use cadmpeg_ir::eval::{
    model_curve_point_by_id, model_surface_partials_by_id, model_surface_point_by_id, pcurve_uv,
};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, PcurveGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, SurfaceId};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::{LengthUnit, Units};
use cadmpeg_ir::CadIr;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::ids::StepIdentity;
use crate::test_support::{decode_inline, export};
use crate::{
    write_step, StepCodec, StepError, StepSchema, StepUnsupportedPolicy, StepWriteOptions,
};

#[test]
fn parser_enforces_the_part21_header_contract() {
    let cases = [
        (
            "ISO-10303-21;HEADER;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "HEADER must begin with FILE_DESCRIPTION, FILE_NAME, and FILE_SCHEMA",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_SCHEMA(('AP242'));FILE_NAME('','',(''),(''),'','','');ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "HEADER must begin with FILE_DESCRIPTION, FILE_NAME, and FILE_SCHEMA",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','','','','','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "FILE_NAME has invalid parameters",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242','ap242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "FILE_SCHEMA has invalid or duplicate schema identifiers",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242','AP24\\X\\32'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "FILE_SCHEMA has invalid or duplicate schema identifiers",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(());ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "FILE_SCHEMA has invalid or duplicate schema identifiers",
        ),
    ];

    for (source, message) in cases {
        let error = crate::parse::parse(source.as_bytes()).expect_err("invalid header");
        assert!(
            error.to_string().contains(message),
            "expected {message:?}, got {error}"
        );
    }
}

#[test]
fn parser_validates_header_string_bounds_timestamps_and_schema_identifiers() {
    fn source(file_name: &str, schema: &str, extra: &str) -> String {
        format!(
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME({file_name});FILE_SCHEMA(({schema}));{extra}ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;"
        )
    }

    let valid = source(
        "'name','2026-02-28T23:59:59.123+02:30',('author'),('organization'),'preprocessor','',''",
        "'AP242 { 1 0 10303 442 3 1 4 }'",
        "SCHEMA_POPULATION((('part.step','2026-02-28T00:00:00Z','YWJjZA==')));",
    );
    crate::parse::parse(valid.as_bytes()).expect("valid header metadata");

    let schema_oid = source(
        "'name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','',''",
        "' AUTOMOTIVE_DESIGN_CC2 { 1 2 10303 214 1 1 5 4 } '",
        "",
    );
    crate::parse::parse(schema_oid.as_bytes()).expect("schema object identifier");

    let named_schema_oid = source(
        "'name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','',''",
        "' AUTOMOTIVE_DESIGN_CC2 { iso standard 10303 part(214) version(1) } '",
        "",
    );
    crate::parse::parse(named_schema_oid.as_bytes()).expect("named schema object identifier");

    let invalid = [
        source(
            "'name','2026-02-30T23:59:59',('author'),('organization'),'preprocessor','',''",
            "'AP242'",
            "",
        ),
        source(
            "'name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','',''",
            "'AP242 { 1 invalid_ }'",
            "",
        ),
        source(
            "'name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','',''",
            "'AP242 { }'",
            "",
        ),
        source(
            "'name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','',''",
            "'AP242 { 1 }'",
            "",
        ),
        source(
            "'name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','',''",
            "'AP242 { 3 0 }'",
            "",
        ),
        source(
            "'name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','',''",
            "'AP242 { 0 40 }'",
            "",
        ),
        source(
            "'name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','',''",
            "'AP242 { 01 0 }'",
            "",
        ),
        source(
            "'name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','',''",
            "'AP-242'",
            "",
        ),
        source(
            "'name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','',''",
            "'AP242'",
            "SCHEMA_POPULATION((('part.step','2026-02-28T00:00:00Z','not*base64')));",
        ),
    ];
    for source in invalid {
        assert!(crate::parse::parse(source.as_bytes()).is_err());
    }

    let long_description = "x".repeat(257);
    let long_description_source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('{long_description}'),'4;2');FILE_NAME('name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;"
    );
    assert!(crate::parse::parse(long_description_source.as_bytes()).is_err());

    let long_schema = "A".repeat(1025);
    let long_schema_source = source(
        "'name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','',''",
        &format!("'{long_schema}'"),
        "",
    );
    assert!(crate::parse::parse(long_schema_source.as_bytes()).is_err());

    let malformed_data_schema = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA('main',('AP242 { 1 invalid }'));#1=ITEM();ENDSEC;END-ISO-10303-21;";
    assert!(crate::parse::parse(malformed_data_schema.as_bytes()).is_err());
}

#[test]
fn parser_rejects_noncanonical_or_invalid_schema_identifiers() {
    let cases = [
        ("' AP242 ','AP242'", "'AP242'", "trimmed duplicate"),
        ("'9AP242'", "'9AP242'", "leading digit"),
        ("'_AP242'", "'_AP242'", "leading underscore"),
        (
            "'AP242 { 1 0 10303 442 -1 1 4 }'",
            "'AP242'",
            "negative OID component",
        ),
    ];
    let mut admitted = Vec::new();
    for (schema, data_schema, description) in cases {
        let source = format!(
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(({schema}));ENDSEC;DATA('main',({data_schema}));#1=ITEM();ENDSEC;END-ISO-10303-21;"
        );
        match crate::parse::parse(source.as_bytes()) {
            Ok(_) => admitted.push(description),
            Err(error) => assert!(
                error
                    .to_string()
                    .contains("FILE_SCHEMA has invalid or duplicate schema identifiers"),
                "{description}: unexpected error: {error}"
            ),
        }
    }
    assert!(
        admitted.is_empty(),
        "admitted invalid identifiers: {admitted:?}"
    );
}

#[test]
fn parser_retains_unset_file_name_tail_metadata() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'',$,$);FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("unset producer metadata");
    assert!(matches!(
        exchange.header[1].parameters[6],
        crate::parse::Value::Omitted
    ));
}

#[test]
fn parser_allows_an_empty_data_population_in_edition_three() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;END-ISO-10303-21;";
    crate::parse::parse(source).expect("edition three permits no DATA section");
}

#[test]
fn parser_enforces_legacy_implementation_level_restrictions() {
    let cases = [
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;END-ISO-10303-21;",
            "historical implementation levels require one DATA section",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA('section');#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "2;1 forbids DATA section parameters",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "2;1 forbids ANCHOR and REFERENCE sections",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;YWJjZA==ENDSEC;",
            "2;1 forbids SIGNATURE sections",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'3;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "3;1 forbids ANCHOR and REFERENCE sections",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'3;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));SCHEMA_POPULATION(('all'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "3;1 forbids SCHEMA_POPULATION in HEADER",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'3;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;END-ISO-10303-21;",
            "historical implementation levels require one DATA section",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'3;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;YWJjZA==ENDSEC;",
            "3;1 forbids SIGNATURE sections",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'1;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "FILE_DESCRIPTION has an unsupported implementation level",
        ),
    ];

    for (source, message) in cases {
        let error = crate::parse::parse(source.as_bytes()).expect_err("invalid level");
        assert!(
            error.to_string().contains(message),
            "expected {message:?}, got {error}"
        );
    }
}

#[test]
fn parser_accepts_historical_implementation_level_spellings() {
    for level in ["1", "2", "2;1", "2;2", "3;1", "3;2"] {
        let source = format!(
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'{level}');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;"
        );
        crate::parse::parse(source.as_bytes()).expect("historical implementation level");
    }
}

#[test]
fn parser_enforces_edition_three_conformance_classes() {
    let cases = [
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<item>=#1;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "4;1 forbids ANCHOR and REFERENCE sections",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));SCHEMA_POPULATION((('part.step',$,$)));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "4;1 forbids SCHEMA_POPULATION in HEADER",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;YWJjZA==ENDSEC;",
            "4;1 forbids SIGNATURE sections",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;@10=<part.step#value>;ENDSEC;DATA;#1=ITEM(@10);ENDSEC;END-ISO-10303-21;",
            "this implementation level forbids value instances and EXPRESS constants",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(#PI);ENDSEC;END-ISO-10303-21;",
            "this implementation level forbids value instances and EXPRESS constants",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'3;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(<external>);ENDSEC;END-ISO-10303-21;",
            "resource values are only valid in edition-3 anchor items",
        ),
    ];

    for (source, message) in cases {
        let error = crate::parse::parse(source.as_bytes()).expect_err("invalid conformance class");
        assert!(
            error.to_string().contains(message),
            "expected {message:?}, got {error}"
        );
    }

    let class_three = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;3');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;@10=<part.step#value>;ENDSEC;DATA;#1=ITEM(@10,#PI);ENDSEC;END-ISO-10303-21;";
    crate::parse::parse(class_three).expect("class three value occurrences");
}

#[test]
fn parser_allows_multiple_schema_identifiers_at_legacy_level() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('CONFIG_CONTROL_DESIGN','GEOMETRIC_VALIDATION_PROPERTIES_MIM'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    crate::parse::parse(source).expect("2;1 permits multiple schema identifiers");
}

#[test]
fn parser_validates_optional_header_entities_and_data_section_targets() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));SCHEMA_POPULATION((('part.step',$,$)));FILE_POPULATION('AP242','INCLUDE_ALL_COMPATIBLE',('main'));SECTION_LANGUAGE('main','eng');SECTION_CONTEXT('main',('design'));!VENDOR(('metadata'));ENDSEC;DATA('main',('AP242'));#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("valid optional header entities");
    assert_eq!(exchange.data[0].records, vec![1]);
    assert_eq!(exchange.header[7].name, "!VENDOR");

    let invalid = [
        (
            "SCHEMA_POPULATION((('part.step',$)));",
            "SCHEMA_POPULATION has invalid parameters",
        ),
        (
            "FILE_POPULATION('AP214','INCLUDE_ALL_COMPATIBLE',('main'));",
            "FILE_POPULATION has invalid parameters",
        ),
        (
            "SECTION_LANGUAGE('missing','eng');",
            "header section reference names an unknown DATA section",
        ),
        (
            "SECTION_LANGUAGE('main','english');",
            "SECTION_LANGUAGE has invalid parameters",
        ),
        (
            "!VENDOR(('metadata'));SECTION_CONTEXT('main',('design'));",
            "built-in HEADER entities must precede user-defined entities",
        ),
    ];
    for (extra, message) in invalid {
        let source = format!(
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));{extra}ENDSEC;DATA('main',('AP242'));#1=ITEM();ENDSEC;END-ISO-10303-21;"
        );
        let error = crate::parse::parse(source.as_bytes()).expect_err("invalid header entity");
        assert!(
            error.to_string().contains(message),
            "expected {message:?}, got {error}"
        );
    }

    let legacy = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));SCHEMA_POPULATION((('part.step',$,$)));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    crate::parse::parse(legacy).expect("2;1 permits SCHEMA_POPULATION");
}

#[test]
fn parser_enforces_data_section_parameter_shape_and_multiplicity() {
    let cases = [
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;DATA;#2=ITEM();ENDSEC;END-ISO-10303-21;",
            "multiple DATA sections require section parameters",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;DATA('section',('AP242'));#2=ITEM();ENDSEC;END-ISO-10303-21;",
            "multiple DATA sections require section parameters",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA('section',('AP242'));#1=ITEM();ENDSEC;DATA('section',('AP242'));#2=ITEM();ENDSEC;END-ISO-10303-21;",
            "DATA section names must be unique",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA('section');#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "DATA section parameters must contain a name and one schema",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA('section',('AP214'));#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "DATA section schema is not listed in FILE_SCHEMA",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242','AP214'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "an unnamed DATA section requires one FILE_SCHEMA identifier",
        ),
    ];

    for (source, message) in cases {
        let error = crate::parse::parse(source.as_bytes()).expect_err("invalid DATA section");
        assert!(
            error.to_string().contains(message),
            "expected {message:?}, got {error}"
        );
    }

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242','AP214'));ENDSEC;DATA('section-1',('AP242'));#1=ITEM();ENDSEC;DATA('section-2',('AP214'));#2=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("valid named DATA sections");
    assert_eq!(exchange.data.len(), 2);
}

#[test]
fn codec_uses_the_first_schema_identifier_for_exact_edition_selection() {
    let cases = [
        (
            "'AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }','AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 4 1 4 }'",
            "edition 1",
        ),
        (
            "'AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 14 1 4 }'",
            "edition unspecified",
        ),
        (
            "'AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { iso standard 10303 part(442) version(3) }'",
            "edition unspecified",
        ),
        (
            "'OTHER_SCHEMA { 1 0 10303 442 4 1 4 }'",
            "edition unspecified",
        ),
        (
            "'ap242_managed_model_based_3d_engineering_mim_lf { 1 0 10303 442 4 1 4 }'",
            "edition 3",
        ),
    ];

    for (identifiers, expected_edition) in cases {
        let first_identifier = identifiers.split(',').next().expect("first schema");
        let source = format!(
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(({identifiers}));ENDSEC;DATA('section',({first_identifier}));#1=ITEM();ENDSEC;END-ISO-10303-21;"
        );
        let summary = StepCodec::default()
            .inspect(
                &mut Cursor::new(source.as_bytes()),
                &InspectOptions::default(),
            )
            .expect("inspect schema identifiers");
        assert!(
            summary
                .notes
                .iter()
                .any(|note| note.ends_with(expected_edition)),
            "expected {expected_edition} in {:?}",
            summary.notes
        );
    }
}

#[test]
fn codec_refuses_out_of_envelope_encodings_by_name() {
    let codec = StepCodec::default();
    let cases: &[(&[u8], &str)] = &[
        (
            b"\x89HDF\r\n\x1a\ncontent",
            "STEP Part 26 binary/HDF5 encoding",
        ),
        (
            b"<?xml version='1.0'?><iso_10303_28/>",
            "STEP Part 28 XML encoding",
        ),
        (
            b"<?xml version='1.0'?><business_object_model/>",
            "AP242 BO-Model XML sidecar",
        ),
    ];
    for &(bytes, reason) in cases {
        let error = codec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::NotImplemented(message) if message == reason)
        );
    }
    assert_eq!(
        codec.detect(b"<?xml version='1.0'?><iso_10303_28/>"),
        Confidence::Medium
    );
}
