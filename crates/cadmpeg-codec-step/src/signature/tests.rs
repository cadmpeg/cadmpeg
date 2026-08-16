// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]
#![allow(unused_imports)]
use super::validate_detached_cms;

#[test]
fn accepts_a_minimal_detached_signed_data_envelope() {
    let cms = [
        0x30, 0x5a, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x02, 0xa0, 0x4d,
        0x30, 0x4b, 0x02, 0x01, 0x01, 0x31, 0x0d, 0x30, 0x0b, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01,
        0x65, 0x03, 0x04, 0x02, 0x01, 0x30, 0x0b, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d,
        0x01, 0x07, 0x01, 0x31, 0x2a, 0x30, 0x28, 0x02, 0x01, 0x01, 0x30, 0x05, 0x30, 0x00, 0x02,
        0x01, 0x01, 0x30, 0x0b, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01,
        0x30, 0x0d, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01, 0x05, 0x00,
        0x04, 0x00,
    ];
    assert!(validate_detached_cms(&cms).is_ok());
}

#[test]
fn rejects_embedded_content() {
    let cms = [
        0x30, 0x5c, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x02, 0xa0, 0x4f,
        0x30, 0x4d, 0x02, 0x01, 0x01, 0x31, 0x0d, 0x30, 0x0b, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01,
        0x65, 0x03, 0x04, 0x02, 0x01, 0x30, 0x0d, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d,
        0x01, 0x07, 0x01, 0xa0, 0x02, 0x04, 0x00, 0x31, 0x2a, 0x30, 0x28, 0x02, 0x01, 0x01, 0x30,
        0x05, 0x30, 0x00, 0x02, 0x01, 0x01, 0x30, 0x0b, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65,
        0x03, 0x04, 0x02, 0x01, 0x30, 0x0d, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01,
        0x01, 0x01, 0x05, 0x00, 0x04, 0x00,
    ];
    assert!(validate_detached_cms(&cms).is_err());
}

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
fn parser_retains_multiple_signature_sections_after_exchange_terminator() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('signatures'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\nENDSEC;SIGNATURE;MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\nENDSEC;";
    let (exchange, diagnostics) = crate::parse::parse(source).expect("multiple signatures");

    assert!(diagnostics.is_empty());
    assert_eq!(exchange.signatures.len(), 2);
    assert!(source[exchange.signatures[0].clone()]
        .windows(b"MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=".len())
        .any(|bytes| bytes == b"MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA="));
    assert!(source[exchange.signatures[1].clone()]
        .windows(b"MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=".len())
        .any(|bytes| bytes == b"MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA="));
}

#[test]
fn parser_accepts_signature_edge_separators_and_keeps_each_boundary() {
    let payload = "MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=";
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('signatures'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;/* decoy ENDSEC; */\\N\\{payload}\\F\\/* trailing ENDSEC; */EN\nDSEC;SIGNATURE;\\F\\{payload}\\N\\ENDSEC;"
    );
    let (exchange, diagnostics) = crate::parse::parse(source.as_bytes()).expect("edge separators");

    assert!(diagnostics.is_empty());
    assert_eq!(exchange.signature_sections.len(), 2);
    assert_eq!(exchange.signatures.len(), 2);
    assert!(source.as_bytes()[exchange.signatures[0].clone()].ends_with(b"DSEC;"));
    assert!(source.as_bytes()[exchange.signatures[0].clone()]
        .windows(b"EN\nDSEC;".len())
        .any(|bytes| bytes == b"EN\nDSEC;"));
    assert!(source.as_bytes()[exchange.signatures[1].clone()].ends_with(b"ENDSEC;"));
    assert_eq!(
        exchange.signature_sections[0].signed.end,
        exchange.signatures[0].start
    );
    assert_eq!(
        exchange.signature_sections[1].signed.end,
        exchange.signatures[1].start
    );
    assert_eq!(exchange.signatures[1].start, exchange.signatures[0].end);
    assert_eq!(exchange.signature_sections[0].cms.len(), 92);
    assert_eq!(exchange.signature_sections[1].cms.len(), 92);
}

#[test]
fn parser_does_not_treat_unseparated_endsec_text_as_a_signature_boundary() {
    let payload = "MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=";
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('signature'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;{payload}ENDSEC;AAAA\nENDSEC;"
    );
    let error = crate::parse::parse(source.as_bytes()).expect_err("unseparated ENDSEC text");

    assert!(matches!(
        error,
        crate::parse::ParseError::Lex(crate::lex::LexError { message, .. })
            if message == "invalid SIGNATURE base64 padding"
    ));
}

#[test]
fn parser_exposes_the_detached_signature_contract() {
    let source = b" /* leading trivia */ ISO-10303-21;HEADER;FILE_DESCRIPTION(('signature'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\nENDSEC;";
    let (exchange, diagnostics) = crate::parse::parse(source).expect("signature contract");

    assert!(diagnostics.is_empty());
    let section = &exchange.signature_sections[0];
    let signed = &source[section.signed.clone()];
    assert!(signed.starts_with(b"ISO-10303-21;"));
    assert!(signed.ends_with(b"END-ISO-10303-21;"));
    assert_eq!(
        &source[section.payload.clone()],
        b"MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\n"
    );
    assert_eq!(section.cms.len(), 92);
    let signed_alphabet = section
        .signed_alphabet_bytes(source)
        .expect("signed source range");
    assert!(!signed_alphabet.contains(&b'\n'));
    assert!(signed_alphabet.ends_with(b"END-ISO-10303-21;"));
    assert_eq!(
        &source[section.span.clone()],
        b"SIGNATURE;MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\nENDSEC;"
    );
}

#[test]
fn signature_method_and_parameters_are_inside_cms_payload() {
    let source = include_bytes!("tests/data/sg01_signature_method_selection.p21");
    let (exchange, diagnostics) = crate::parse::parse(source).expect("signature method witness");

    assert!(diagnostics.is_empty());
    assert_eq!(exchange.signature_sections.len(), 1);
    let section = &exchange.signature_sections[0];
    assert_eq!(
        &source[section.span.clone()],
        b"SIGNATURE;\nMFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\nENDSEC;"
    );
    assert_eq!(
        &source[section.span.start..section.payload.start],
        b"SIGNATURE;"
    );
    assert_eq!(&source[section.payload.end..section.span.end], b"ENDSEC;");
    assert_eq!(section.cms.len(), 92);
}

#[test]
fn parser_projects_each_signature_to_preceding_alphabet_bytes() {
    let source = include_bytes!("tests/data/sg02_signed_byte_sequence.p21");
    let (exchange, diagnostics) = crate::parse::parse(source).expect("signed byte witness");

    assert!(diagnostics.is_empty());
    assert_eq!(exchange.signature_sections.len(), 2);
    let first = &exchange.signature_sections[0];
    let second = &exchange.signature_sections[1];
    assert_eq!(first.signed.start, 0);
    assert_eq!(first.signed.end, first.span.start);
    assert_eq!(second.signed.start, 0);
    assert_eq!(second.signed.end, second.span.start);
    assert_eq!(
        first.signed_alphabet_bytes(source).expect("first signed bytes"),
        b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('SG-02 signed byte sequence witness'),'4;2');FILE_NAME('sg02_signed_byte_sequence','2026-08-16T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=PRODUCT('P1','Part','',());#2=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));#3=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));#4=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#2,#3)) REPRESENTATION_CONTEXT('model','3D'));ENDSEC;END-ISO-10303-21;/*FIRST*/\\N\\"
    );
    let mut expected_second = first
        .signed_alphabet_bytes(source)
        .expect("first signed bytes")
        .clone();
    expected_second.extend_from_slice(
        b"SIGNATURE;MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=ENDSEC;/*SECOND*/\\F\\",
    );
    assert_eq!(
        second
            .signed_alphabet_bytes(source)
            .expect("second signed bytes"),
        expected_second
    );
    assert_eq!(
        &source[first.span.clone()],
        b"SIGNATURE;\nMFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\nENDSEC;"
    );
}

#[test]
fn parser_ignores_controls_inside_signature_terminators() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('signature'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\nEN\nDSEC;";
    let (exchange, _) = crate::parse::parse(source).expect("split signature terminator");
    assert_eq!(exchange.signatures.len(), 1);
}

#[test]
fn parser_rejects_invalid_signature_base64() {
    for (payload, expected_message) in [
        ("YWJjZA==!", "invalid SIGNATURE base64 padding"),
        ("YWJjZA==AAAA", "invalid SIGNATURE base64 padding"),
        ("YWJjZA=", "SIGNATURE base64 content has incomplete quantum"),
        ("YWJj ZA==", "invalid SIGNATURE base64 character"),
    ] {
        let source = format!(
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('signature'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;{payload}\nENDSEC;"
        );
        let error = crate::parse::parse(source.as_bytes()).expect_err("invalid signature");
        assert!(matches!(
            error,
            crate::parse::ParseError::Lex(crate::lex::LexError { message, .. })
                if message == expected_message
        ));
    }
}
